//! Split vertical text blocks into columns (and long columns into
//! chunks) before OCR.
//!
//! OCR models handed a whole multi-column vertical block tend to drop a
//! column or read straight across the columns (e.g. "へへ…/似合ってるでぇ/
//! 嬢ちゃん" came back as "嬢似へちゃんてるでえ"), and Manga OCR squeezes
//! every crop into 224×224, so a long column's glyphs end up too small to
//! read. The page's text segmentation mask shows where the strokes are: a
//! block is cut at the gaps between its columns, long columns at the gaps
//! between glyphs, and the pieces are read right to left, top to bottom.

use image::{DynamicImage, GrayImage};
use koharu_ml::comic_text_detector::crop_text_block_bbox;
use koharu_ml::types::{TextDirection, TextRegion};

/// Mask value counted as a text pixel.
const INK: u8 = 128;
/// Columns narrower than this share of the widest one are ruby (furigana)
/// or specks and are left out.
const MIN_COLUMN_SHARE: f32 = 0.45;
/// Gaps narrower than this share of the widest column belong to the column
/// (strokes of one glyph can leave thin empty stripes).
const MIN_GAP_SHARE: f32 = 0.12;
/// A column taller than this many column widths is read in chunks...
const MAX_COLUMN_RATIO: f32 = 6.5;
/// ...of about this many widths (roughly that many glyphs) each.
const CHUNK_RATIO: f32 = 5.0;

/// Ink box of one column, unpadded: `x1..x2`, `top..bottom` (exclusive).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Column {
    x1: u32,
    x2: u32,
    top: u32,
    bottom: u32,
}

/// Crops to OCR for `block`, in reading order: for a vertical block with
/// text in `mask`, its columns right to left, long ones cut into chunks;
/// otherwise the whole block.
pub fn ocr_crops(
    image: &DynamicImage,
    mask: Option<&GrayImage>,
    block: &TextRegion,
) -> Vec<DynamicImage> {
    let whole = || vec![crop_text_block_bbox(image, block)];
    let Some(mask) = mask.filter(|m| m.dimensions() == (image.width(), image.height())) else {
        return whole();
    };
    let vertical = match block.source_direction {
        Some(direction) => direction == TextDirection::Vertical,
        None => block.height > block.width * 1.2,
    };
    if !vertical || block.rotation_deg.is_some_and(|r| r.abs() > 3.0) {
        return whole();
    }
    let columns = vertical_columns(mask, block);
    let long = |c: &Column| (c.bottom - c.top) as f32 > (c.x2 - c.x1) as f32 * MAX_COLUMN_RATIO;
    // A single short column: the whole block is as good and safer if the
    // mask missed a stroke.
    if columns.is_empty() || (columns.len() == 1 && !long(&columns[0])) {
        return whole();
    }
    let (w, h) = mask.dimensions();
    columns
        .iter()
        .rev()
        .flat_map(|c| chunk_column(mask, *c))
        .map(|c| {
            let pad = ((c.x2 - c.x1) as f32 * 0.2).ceil().max(2.0) as u32;
            let x1 = c.x1.saturating_sub(pad);
            let y1 = c.top.saturating_sub(pad);
            let x2 = (c.x2 + pad).min(w);
            let y2 = (c.bottom + pad).min(h);
            image.crop_imm(x1, y1, x2 - x1, y2 - y1)
        })
        .collect()
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

    let min_ink = ((by2 - by1) as f32 * 0.01).max(1.0) as u32;
    let ink_in_column: Vec<bool> = (sx1..sx2)
        .map(|x| (by1..by2).filter(|&y| inked(mask, x, y)).count() as u32 >= min_ink)
        .collect();

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

    merged
        .into_iter()
        .filter_map(|(x1, x2)| {
            let row_inked = |y: u32| (x1..x2).any(|x| inked(mask, x, y));
            let rows: Vec<u32> = (by1..by2).filter(|&y| row_inked(y)).collect();
            let (mut top, mut bottom) = (*rows.first()?, *rows.last()? + 1);
            // Follow the column past the box while its glyphs continue
            // (allowing the gap between two glyphs).
            let max_gap = ((x2 - x1) as f32 * 0.6).ceil() as u32;
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

/// A long column cut at the emptiest rows near evenly spaced points, so
/// glyphs are not split; a short column as is.
fn chunk_column(mask: &GrayImage, c: Column) -> Vec<Column> {
    let width = (c.x2 - c.x1).max(1) as f32;
    let height = (c.bottom - c.top) as f32;
    if height <= width * MAX_COLUMN_RATIO {
        return vec![c];
    }
    let pieces = (height / (width * CHUNK_RATIO)).ceil() as u32;
    let row_ink = |y: u32| (c.x1..c.x2).filter(|&x| inked(mask, x, y)).count();
    let window = (width * 0.6).ceil() as u32;
    let mut cuts = Vec::with_capacity(pieces as usize + 1);
    cuts.push(c.top);
    for k in 1..pieces {
        let target = c.top + ((c.bottom - c.top) * k) / pieces;
        let lo = target.saturating_sub(window).max(*cuts.last().unwrap() + 1);
        let hi = (target + window).min(c.bottom - 1);
        // Emptiest row, nearest the target on ties.
        let cut = (lo..=hi)
            .min_by_key(|&y| (row_ink(y), y.abs_diff(target)))
            .unwrap_or(target);
        cuts.push(cut);
    }
    cuts.push(c.bottom);
    cuts.windows(2)
        .filter(|pair| pair[1] > pair[0])
        .map(|pair| Column {
            top: pair[0],
            bottom: pair[1],
            ..c
        })
        .collect()
}

/// Join the text read from one block's crops.
pub fn join_columns(parts: Vec<String>) -> String {
    parts.concat()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Luma, RgbImage};

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
        let crops = ocr_crops(&image, Some(&mask), &block(10.0, 10.0, 150.0, 280.0));
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
    fn long_columns_are_cut_between_glyphs() {
        let image = DynamicImage::ImageRgb8(RgbImage::new(100, 600));
        let mut mask = GrayImage::new(100, 600);
        // 14 glyphs of 30 px: 14 * 34 = 476 px tall, ratio ~16.
        glyph_column(&mut mask, 30, 20, 30, 14);
        let crops = ocr_crops(&image, Some(&mask), &block(20.0, 10.0, 50.0, 500.0));
        assert_eq!(crops.len(), 4);
        let column = vertical_columns(&mask, &block(20.0, 10.0, 50.0, 500.0))[0];
        for piece in chunk_column(&mask, column).windows(2) {
            let cut = piece[1].top;
            // Every cut falls in a gap between glyphs.
            assert!((0..14).all(|g| {
                let y = 20 + g * 34;
                cut < y || cut >= y + 30
            }));
        }
    }

    #[test]
    fn furigana_short_single_columns_and_horizontal_blocks_stay_whole() {
        let image = DynamicImage::ImageRgb8(RgbImage::new(200, 300));
        let mut mask = GrayImage::new(200, 300);
        glyph_column(&mut mask, 40, 20, 30, 5);
        paint(&mut mask, 75..82, 30..60); // ruby beside the column
        let one_column = ocr_crops(&image, Some(&mask), &block(30.0, 10.0, 60.0, 200.0));
        assert_eq!(one_column.len(), 1);

        glyph_column(&mut mask, 120, 20, 30, 5);
        let mut horizontal = block(30.0, 10.0, 130.0, 200.0);
        horizontal.source_direction = Some(TextDirection::Horizontal);
        assert_eq!(ocr_crops(&image, Some(&mask), &horizontal).len(), 1);
        assert_eq!(
            ocr_crops(&image, None, &block(30.0, 10.0, 130.0, 200.0)).len(),
            1
        );
    }
}
