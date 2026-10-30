//! Split vertical text blocks into columns before OCR.
//!
//! OCR models handed a whole multi-column vertical block tend to drop a
//! column or read straight across the columns (e.g. "へへ…/似合ってるでぇ/
//! 嬢ちゃん" came back as "嬢似へちゃんてるでえ"). The page's text
//! segmentation mask (narrowed to the lettering's fill colour) shows where
//! the strokes are: a block is cut at the gaps between its columns and the
//! columns are read right to left. (Cutting long columns into chunks was
//! tried and read worse.)

use image::{DynamicImage, GrayImage, Luma, RgbImage, imageops::FilterType};
use koharu_ml::comic_text_detector::crop_text_block_bbox;
use koharu_ml::types::{TextDirection, TextRegion};

/// Mask value counted as a text pixel.
const INK: u8 = 128;
/// Columns narrower than this share of the widest one are ruby (furigana)
/// or specks and are left out.
const MIN_COLUMN_SHARE: f32 = 0.45;
/// Gaps narrower than this share of the widest column belong to the column
/// (strokes of one glyph can leave thin empty stripes).
const MIN_GAP_SHARE: f32 = 0.05;
/// A pixel column is part of a text column when it holds at least this
/// share of the typical ink per pixel column; the faint ink between two
/// close columns (stray outline or dots) stays below it.
const MIN_INK_SHARE: f32 = 0.15;

/// A column's `x1`, `x2` and its runs of glyphs (see [`glyph_runs`]).
type ColumnRuns = (u32, u32, Vec<(u32, u32)>);

/// Ink box of one column, unpadded: `x1..x2`, `top..bottom` (exclusive).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Column {
    x1: u32,
    x2: u32,
    top: u32,
    bottom: u32,
}

/// Colours of a block's lettering, from the font detector.
#[derive(Clone, Copy, Debug)]
pub struct TextColors {
    pub fill: [u8; 3],
    /// Outline colour, when the lettering has one.
    pub outline: Option<[u8; 3]>,
}

/// Crops to OCR for `block`, in reading order: for a vertical block, its
/// columns right to left; otherwise (or when no columns can be told
/// apart) the whole block.
///
/// Columns are found in an ink map: pixels of the segmentation `mask`
/// that also have the lettering's fill colour. Outlined lettering fills
/// the gaps between columns in the mask itself, so the fill colour is what
/// separates them; the plain mask is the fallback.
pub fn ocr_crops(
    image: &DynamicImage,
    rgb: &RgbImage,
    mask: Option<&GrayImage>,
    colors: Option<TextColors>,
    block: &TextRegion,
) -> Vec<DynamicImage> {
    let whole = || vec![crop_text_block_bbox(image, block)];
    let mask = mask.filter(|m| m.dimensions() == rgb.dimensions());
    let vertical = match block.source_direction {
        Some(direction) => direction == TextDirection::Vertical,
        None => block.height > block.width * 1.2,
    };
    tracing::debug!(
        x = block.x,
        y = block.y,
        w = block.width,
        h = block.height,
        vertical,
        has_mask = mask.is_some(),
        ?colors,
        "ocr column split"
    );
    if !vertical || block.rotation_deg.is_some_and(|r| r.abs() > 3.0) {
        return whole();
    }
    let usable = |columns: &[Column]| columns.len() >= 2;

    let mut found = None;
    for ink in [
        colors.and_then(|c| colour_ink(rgb, mask, c, block)),
        // In case the detector swapped fill and outline.
        colors
            .and_then(|c| {
                c.outline.map(|o| TextColors {
                    fill: o,
                    outline: Some(c.fill),
                })
            })
            .and_then(|c| colour_ink(rgb, mask, c, block)),
        mask.cloned(),
    ]
    .into_iter()
    .flatten()
    {
        let columns = vertical_columns(&ink, block);
        tracing::debug!(columns = ?columns, "ocr column split attempt");
        if usable(&columns) {
            found = Some((ink, columns));
            break;
        }
    }
    // A single column: the whole block is as good and safer if the ink map
    // missed a stroke.
    let Some((ink, columns)) = found else {
        return whole();
    };
    let (w, h) = ink.dimensions();
    // Padding never reaches past the middle of the gap to a neighbouring
    // column, so no edge of a neighbour's glyph ends up in the crop (a
    // model would read it as an extra character).
    let crops: Vec<DynamicImage> = columns
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let pad = ((c.x2 - c.x1) as f32 * 0.2).ceil().max(2.0) as u32;
            let left_limit = i
                .checked_sub(1)
                .map(|j| columns[j].x2 + (c.x1 - columns[j].x2) / 2)
                .unwrap_or(0);
            let right_limit = columns
                .get(i + 1)
                .map(|n| c.x2 + (n.x1 - c.x2).div_ceil(2))
                .unwrap_or(w);
            let x1 = c.x1.saturating_sub(pad).max(left_limit);
            let y1 = c.top.saturating_sub(pad);
            let x2 = (c.x2 + pad).min(w).min(right_limit);
            let y2 = (c.bottom + pad).min(h);
            image.crop_imm(x1, y1, x2 - x1, y2 - y1)
        })
        .collect();
    crops.into_iter().rev().collect()
}

fn colour_distance(a: [u8; 3], b: [u8; 3]) -> u32 {
    a.iter()
        .zip(b)
        .map(|(&x, y)| (x as i32 - y as i32).pow(2) as u32)
        .sum()
}

/// Page-sized ink map around `block`: pixels in `mask` (when given) whose
/// colour is the lettering's fill rather than its outline. `None` when the
/// fill colour cannot be told from the outline.
fn colour_ink(
    rgb: &RgbImage,
    mask: Option<&GrayImage>,
    colors: TextColors,
    block: &TextRegion,
) -> Option<GrayImage> {
    // Fill and outline need to differ clearly to separate them.
    const MIN_CONTRAST: u32 = 60 * 60;
    // Pixels this close to the predicted fill colour (the prediction is
    // often some way off; the dark scene showing between outlined columns
    // inside the mask is further).
    const MAX_FILL_DISTANCE: u32 = 85 * 85;
    let outline = colors
        .outline
        .filter(|o| colour_distance(*o, colors.fill) >= MIN_CONTRAST);
    let (w, h) = rgb.dimensions();
    let reach = block.width.max(block.height);
    let x1 = (block.x - reach).max(0.0) as u32;
    let y1 = (block.y - reach).max(0.0) as u32;
    let x2 = ((block.x + block.width + reach).max(0.0) as u32).min(w);
    let y2 = ((block.y + block.height + reach).max(0.0) as u32).min(h);
    let mut ink = GrayImage::new(w, h);
    let mut any = false;
    for y in y1..y2 {
        for x in x1..x2 {
            if mask.is_some_and(|m| m.get_pixel(x, y)[0] < INK) {
                continue;
            }
            let px = rgb.get_pixel(x, y).0;
            let to_fill = colour_distance(px, colors.fill);
            let is_fill = to_fill <= MAX_FILL_DISTANCE
                && outline.is_none_or(|o| to_fill < colour_distance(px, o));
            if is_fill {
                ink.put_pixel(x, y, Luma([255]));
                any = true;
            }
        }
    }
    any.then_some(ink)
}

fn inked(mask: &GrayImage, x: u32, y: u32) -> bool {
    mask.get_pixel(x, y)[0] >= INK
}

/// Columns (left to right) of the text in `block`, found from gaps in the
/// mask's ink per pixel column. Columns the detector box cuts through are
/// followed past the box; text that lies wholly outside it is ignored.
fn vertical_columns(mask: &GrayImage, block: &TextRegion) -> Vec<Column> {
    let (w, h) = mask.dimensions();
    let margin = (block.width.min(block.height) * 0.3).clamp(8.0, 80.0);
    let bx1 = block.x.max(0.0).floor() as u32;
    let by1 = block.y.max(0.0).floor() as u32;
    let bx2 = ((block.x + block.width).ceil().max(0.0) as u32).min(w);
    let by2 = ((block.y + block.height).ceil().max(0.0) as u32).min(h);
    if bx2 <= bx1 + 1 || by2 <= by1 + 1 {
        return Vec::new();
    }
    let sx1 = (block.x - margin).max(0.0).floor() as u32;
    let sx2 = ((block.x + block.width + margin).ceil().max(0.0) as u32).min(w);

    let counts: Vec<u32> = (sx1..sx2)
        .map(|x| (by1..by2).filter(|&y| inked(mask, x, y)).count() as u32)
        .collect();
    let mut nonzero: Vec<u32> = counts.iter().copied().filter(|&c| c > 0).collect();
    if nonzero.is_empty() {
        return Vec::new();
    }
    nonzero.sort_unstable();
    let typical = nonzero[nonzero.len() / 2] as f32;
    let min_ink = ((by2 - by1) as f32 * 0.01)
        .max(typical * MIN_INK_SHARE)
        .max(1.0);
    let ink_in_column: Vec<bool> = counts.iter().map(|&c| c as f32 >= min_ink).collect();

    // Runs of inked pixel columns that reach into the block.
    let mut runs: Vec<(u32, u32)> = Vec::new();
    let mut start = None;
    for (i, &ink) in ink_in_column
        .iter()
        .chain(std::iter::once(&false))
        .enumerate()
    {
        match (ink, start) {
            (true, None) => start = Some(i as u32),
            (false, Some(s)) => {
                let run = (sx1 + s, sx1 + i as u32);
                if run.1 > bx1 && run.0 < bx2 {
                    runs.push(run);
                }
                start = None;
            }
            _ => {}
        }
    }
    let Some(widest) = runs.iter().map(|(a, b)| b - a).max() else {
        return Vec::new();
    };
    let min_gap = (widest as f32 * MIN_GAP_SHARE).max(1.0) as u32;
    let mut merged: Vec<(u32, u32)> = Vec::new();
    for run in runs {
        match merged.last_mut() {
            Some(last) if run.0 - last.1 < min_gap => last.1 = run.1,
            _ => merged.push(run),
        }
    }
    let widest = merged.iter().map(|(a, b)| b - a).max().unwrap_or(0);
    let min_width = (widest as f32 * MIN_COLUMN_SHARE).ceil() as u32;
    merged.retain(|(a, b)| b - a >= min_width);

    // Runs of glyphs down each column. Columns of a block start level, so
    // each column keeps the run starting nearest the block's first glyph;
    // other lettering further along the same strip (a sign in the scene,
    // say) is left out.
    let column_runs: Vec<ColumnRuns> = merged
        .into_iter()
        .map(|(x1, x2)| {
            let rows: Vec<u32> = (by1..by2)
                .filter(|&y| (x1..x2).any(|x| inked(mask, x, y)))
                .collect();
            let max_gap = ((x2 - x1) as f32 * 0.6).ceil() as u32;
            (x1, x2, glyph_runs(&rows, max_gap))
        })
        .collect();
    let Some(text_top) = column_runs
        .iter()
        .filter_map(|(_, _, runs)| runs.first().map(|r| r.0))
        .min()
    else {
        return Vec::new();
    };

    column_runs
        .into_iter()
        .filter_map(|(x1, x2, runs)| {
            let row_inked = |y: u32| (x1..x2).any(|x| inked(mask, x, y));
            let max_gap = ((x2 - x1) as f32 * 0.6).ceil() as u32;
            let (mut top, mut bottom) = runs.into_iter().min_by_key(|r| r.0.abs_diff(text_top))?;
            // Follow the column past the box while its glyphs continue
            // (allowing the gap between two glyphs).
            let mut gap = 0;
            let mut y = top;
            while y > 0 && gap <= max_gap && top - y < margin as u32 * 2 {
                y -= 1;
                if row_inked(y) {
                    top = y;
                    gap = 0;
                } else {
                    gap += 1;
                }
            }
            let mut gap = 0;
            let mut y = bottom;
            while y < h && gap <= max_gap && y - bottom < margin as u32 * 2 {
                if row_inked(y) {
                    bottom = y + 1;
                    gap = 0;
                } else {
                    gap += 1;
                }
                y += 1;
            }
            Some(Column {
                x1,
                x2,
                top,
                bottom,
            })
        })
        .collect()
}

/// Runs `(first, last + 1)` of `rows` (sorted), top to bottom; rows at
/// most `max_gap` empty rows apart belong to one run.
fn glyph_runs(rows: &[u32], max_gap: u32) -> Vec<(u32, u32)> {
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for &y in rows {
        match runs.last_mut() {
            Some(run) if y - run.1 <= max_gap => run.1 = y + 1,
            _ => runs.push((y, y + 1)),
        }
    }
    runs
}

/// Glyph size (px) vision OCR models read best at. PaddleOCR-VL reads
/// a block right at about 40-60 px per glyph but goes wrong when glyphs
/// are 2-3 times that (it read straight across the columns), which crops
/// from high-resolution pages easily are.
const READING_GLYPH_PX: f32 = 48.0;
/// Without a glyph size, a crop's shorter side is kept at most this long.
const READING_MAX_SHORT_SIDE: u32 = 160;

/// `crop` scaled down so its glyphs are about [`READING_GLYPH_PX`] tall
/// (`glyph_px`: their size in the crop, e.g. from the font detector), or
/// its shorter side at most [`READING_MAX_SHORT_SIDE`]. Never enlarged.
pub fn shrink_for_reading(crop: DynamicImage, glyph_px: Option<f32>) -> DynamicImage {
    let short = crop.width().min(crop.height()).max(1) as f32;
    let scale = match glyph_px.filter(|g| g.is_finite() && *g > 1.0) {
        Some(glyph) => READING_GLYPH_PX / glyph,
        None => READING_MAX_SHORT_SIDE as f32 / short,
    };
    if scale >= 1.0 {
        return crop;
    }
    let w = ((crop.width() as f32 * scale).round() as u32).max(1);
    let h = ((crop.height() as f32 * scale).round() as u32).max(1);
    crop.resize_exact(w, h, image::imageops::FilterType::Triangle)
}

/// Join the text read from one block's crops.
pub fn join_columns(parts: Vec<String>) -> String {
    parts.concat()
}

/// Crops smaller than this (px, shorter side) are enlarged before sending.
pub const MIN_SIDE: u32 = 64;

/// Enlarge a crop whose shorter side is under [`MIN_SIDE`] for LLM reading.
pub fn enlarge_small(crop: DynamicImage) -> DynamicImage {
    let short = crop.width().min(crop.height()).max(1);
    if short >= MIN_SIDE {
        return crop;
    }
    let scale = (MIN_SIDE as f32 / short as f32).ceil() as u32;
    crop.resize(
        crop.width() * scale,
        crop.height() * scale,
        FilterType::Lanczos3,
    )
}

/// The text of a reply: no code fences, wrapping quotes or line breaks
/// (columns are joined without spaces).
pub fn clean_reply(reply: &str) -> String {
    let text = reply.trim().trim_matches('`').trim();
    let text = text
        .strip_prefix('「')
        .and_then(|t| t.strip_suffix('」'))
        .unwrap_or(text);
    let text = text
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .unwrap_or(text);
    text.lines().map(str::trim).collect::<Vec<_>>().concat()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Luma, Rgb, RgbImage};

    fn rgb(image: &DynamicImage) -> RgbImage {
        image.to_rgb8()
    }

    fn paint(mask: &mut GrayImage, x: std::ops::Range<u32>, y: std::ops::Range<u32>) {
        for yy in y {
            for xx in x.clone() {
                mask.put_pixel(xx, yy, Luma([255]));
            }
        }
    }

    /// A column of `glyphs` square glyphs `size` px wide with 4 px between.
    fn glyph_column(mask: &mut GrayImage, x: u32, top: u32, size: u32, glyphs: u32) {
        for g in 0..glyphs {
            let y = top + g * (size + 4);
            paint(mask, x..x + size, y..y + size);
        }
    }

    fn block(x: f32, y: f32, width: f32, height: f32) -> TextRegion {
        TextRegion {
            x,
            y,
            width,
            height,
            source_direction: Some(TextDirection::Vertical),
            ..Default::default()
        }
    }

    #[test]
    fn three_columns_are_read_right_to_left() {
        let image = DynamicImage::ImageRgb8(RgbImage::new(200, 300));
        let mut mask = GrayImage::new(200, 300);
        glyph_column(&mut mask, 20, 20, 30, 4);
        glyph_column(&mut mask, 70, 20, 30, 5);
        glyph_column(&mut mask, 120, 20, 30, 2);
        let crops = ocr_crops(
            &image,
            &rgb(&image),
            Some(&mask),
            None,
            &block(10.0, 10.0, 150.0, 280.0),
        );
        assert_eq!(crops.len(), 3);
        // Rightmost (shortest) column first.
        assert!(crops[0].height() < crops[2].height());
        assert!(crops[2].height() < crops[1].height());
    }

    #[test]
    fn a_column_cut_by_the_box_edge_is_followed() {
        let mut mask = GrayImage::new(300, 300);
        glyph_column(&mut mask, 40, 20, 30, 5);
        glyph_column(&mut mask, 90, 20, 30, 3);
        // The box ends 10 px into the right column.
        let columns = vertical_columns(&mask, &block(30.0, 10.0, 70.0, 200.0));
        assert_eq!(columns.len(), 2);
        assert_eq!((columns[1].x1, columns[1].x2), (90, 120));
        // Text wholly outside the box is not picked up.
        glyph_column(&mut mask, 200, 20, 30, 3);
        assert_eq!(
            vertical_columns(&mask, &block(30.0, 10.0, 70.0, 200.0)).len(),
            2
        );
    }

    #[test]
    fn furigana_short_single_columns_and_horizontal_blocks_stay_whole() {
        let image = DynamicImage::ImageRgb8(RgbImage::new(200, 300));
        let mut mask = GrayImage::new(200, 300);
        glyph_column(&mut mask, 40, 20, 30, 5);
        paint(&mut mask, 75..82, 30..60); // ruby beside the column
        let one_column = ocr_crops(
            &image,
            &rgb(&image),
            Some(&mask),
            None,
            &block(30.0, 10.0, 60.0, 200.0),
        );
        assert_eq!(one_column.len(), 1);

        glyph_column(&mut mask, 120, 20, 30, 5);
        let mut horizontal = block(30.0, 10.0, 130.0, 200.0);
        horizontal.source_direction = Some(TextDirection::Horizontal);
        assert_eq!(
            ocr_crops(&image, &rgb(&image), Some(&mask), None, &horizontal).len(),
            1
        );
        assert_eq!(
            ocr_crops(
                &image,
                &rgb(&image),
                None,
                None,
                &block(30.0, 10.0, 130.0, 200.0)
            )
            .len(),
            1
        );
    }

    #[test]
    fn outlined_lettering_is_split_by_its_fill_colour() {
        // Navy glyphs in thick white outlines: the outlines of the two
        // columns touch, so the mask (fill + outline) has no gap.
        let navy = Rgb([30, 70, 120]);
        let mut page = RgbImage::from_pixel(200, 300, Rgb([90, 120, 60]));
        let mut mask = GrayImage::new(200, 300);
        for (x, glyphs) in [(40u32, 5u32), (90, 3)] {
            for g in 0..glyphs {
                let y = 30 + g * 34;
                for yy in y - 6..y + 36 {
                    for xx in x - 10..x + 40 {
                        page.put_pixel(xx, yy, Rgb([255, 255, 255]));
                        mask.put_pixel(xx, yy, Luma([255]));
                    }
                }
            }
        }
        for (x, glyphs) in [(40u32, 5u32), (90, 3)] {
            for g in 0..glyphs {
                let y = 30 + g * 34;
                for yy in y..y + 30 {
                    for xx in x..x + 30 {
                        page.put_pixel(xx, yy, navy);
                    }
                }
            }
        }
        let image = DynamicImage::ImageRgb8(page);
        let b = block(30.0, 20.0, 110.0, 200.0);
        assert_eq!(
            ocr_crops(&image, &rgb(&image), Some(&mask), None, &b).len(),
            1
        );
        let colors = TextColors {
            fill: [30, 70, 120],
            outline: Some([255, 255, 255]),
        };
        let crops = ocr_crops(&image, &rgb(&image), Some(&mask), Some(colors), &b);
        assert_eq!(crops.len(), 2);
        assert!(crops[0].height() < crops[1].height()); // right column first
    }

    #[test]
    fn faint_ink_between_close_columns_does_not_join_them() {
        let image = DynamicImage::ImageRgb8(RgbImage::new(200, 300));
        let mut mask = GrayImage::new(200, 300);
        glyph_column(&mut mask, 20, 20, 30, 5);
        glyph_column(&mut mask, 52, 20, 30, 5);
        // A few stray pixels in the 2 px gap between the columns.
        paint(&mut mask, 50..52, 40..45);
        let crops = ocr_crops(
            &image,
            &rgb(&image),
            Some(&mask),
            None,
            &block(10.0, 10.0, 90.0, 200.0),
        );
        assert_eq!(crops.len(), 2);
    }

    #[test]
    fn padding_stops_halfway_to_the_next_column() {
        let mut mask = GrayImage::new(200, 300);
        glyph_column(&mut mask, 20, 20, 30, 5);
        glyph_column(&mut mask, 54, 20, 30, 5);
        let image = DynamicImage::ImageRgb8(RgbImage::new(200, 300));
        let crops = ocr_crops(
            &image,
            &rgb(&image),
            Some(&mask),
            None,
            &block(10.0, 10.0, 90.0, 200.0),
        );
        assert_eq!(crops.len(), 2);
        // Gap 50..54: the left column's crop ends at 52, the right one's
        // starts there; each is 30 px of glyph + 2 px of gap + 6 px pad.
        assert_eq!(crops[0].width(), 2 + 30 + 6);
        assert_eq!(crops[1].width(), 6 + 30 + 2);
    }

    #[test]
    fn other_lettering_further_down_the_column_is_left_out() {
        let mut mask = GrayImage::new(200, 400);
        glyph_column(&mut mask, 20, 20, 30, 5);
        glyph_column(&mut mask, 60, 20, 30, 3);
        // A sign below the right column, inside the box, taller than the
        // column's own text.
        paint(&mut mask, 65..85, 210..330);
        let columns = vertical_columns(&mask, &block(10.0, 10.0, 90.0, 340.0));
        assert_eq!(columns.len(), 2);
        assert_eq!((columns[1].top, columns[1].bottom), (20, 20 + 2 * 34 + 30));
    }

    #[test]
    fn replies_are_reduced_to_the_text() {
        assert_eq!(clean_reply("「へへ…」\n"), "へへ…");
        assert_eq!(
            clean_reply("```\nウチは昼、\n持ち帰りの\n```"),
            "ウチは昼、持ち帰りの"
        );
        assert_eq!(clean_reply("  似合ってるでぇ "), "似合ってるでぇ");
    }
}
