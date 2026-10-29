//! Split vertical text blocks into columns before OCR.
//!
//! OCR models handed a whole multi-column vertical block tend to drop a
//! column or read straight across the columns (e.g. "へへ…/似合ってるでぇ/
//! 嬢ちゃん" came back as "嬢似へちゃんてるでえ"). The page's text
//! segmentation mask shows where the strokes are, so a block is cut at the
//! gaps between its columns and each column is read on its own, right to
//! left.

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

/// Crops to OCR for `block`, in reading order: one per column for a
/// vertical block with several columns in `mask`, else the whole block.
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
    if columns.len() < 2 {
        return whole();
    }
    columns
        .into_iter()
        .rev()
        .map(|[x1, y1, x2, y2]| image.crop_imm(x1, y1, x2 - x1, y2 - y1))
        .collect()
}

/// Column boxes `[x1, y1, x2, y2]` (left to right, padded) of the text in
/// `block`, found from gaps in the mask's ink per pixel column.
fn vertical_columns(mask: &GrayImage, block: &TextRegion) -> Vec<[u32; 4]> {
    let (w, h) = mask.dimensions();
    let bx1 = block.x.max(0.0).floor() as u32;
    let by1 = block.y.max(0.0).floor() as u32;
    let bx2 = ((block.x + block.width).ceil().max(0.0) as u32).min(w);
    let by2 = ((block.y + block.height).ceil().max(0.0) as u32).min(h);
    if bx2 <= bx1 + 1 || by2 <= by1 + 1 {
        return Vec::new();
    }

    let min_ink = ((by2 - by1) as f32 * 0.01).max(1.0) as u32;
    let inked: Vec<bool> = (bx1..bx2)
        .map(|x| {
            (by1..by2)
                .filter(|&y| mask.get_pixel(x, y)[0] >= INK)
                .count() as u32
                >= min_ink
        })
        .collect();

    // Runs of inked pixel columns.
    let mut runs: Vec<(u32, u32)> = Vec::new();
    let mut start = None;
    for (i, &ink) in inked.iter().chain(std::iter::once(&false)).enumerate() {
        match (ink, start) {
            (true, None) => start = Some(i as u32),
            (false, Some(s)) => {
                runs.push((bx1 + s, bx1 + i as u32));
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
            // Vertical extent of this column's ink.
            let rows: Vec<u32> = (by1..by2)
                .filter(|&y| (x1..x2).any(|x| mask.get_pixel(x, y)[0] >= INK))
                .collect();
            let (&top, &bottom) = (rows.first()?, rows.last()?);
            let pad = ((x2 - x1) as f32 * 0.2).ceil().max(2.0) as u32;
            Some([
                x1.saturating_sub(pad),
                top.saturating_sub(pad),
                (x2 + pad).min(w),
                (bottom + 1 + pad).min(h),
            ])
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
        // Columns of different heights with gaps between them; a gap
        // inside the middle column (between glyphs) stays in it.
        paint(&mut mask, 20..50, 20..200);
        paint(&mut mask, 70..100, 20..280);
        paint(&mut mask, 120..150, 20..80);
        paint(&mut mask, 70..100, 100..110);
        let crops = ocr_crops(&image, Some(&mask), &block(10.0, 10.0, 150.0, 280.0));
        assert_eq!(crops.len(), 3);
        // Rightmost (shortest) column first.
        assert!(crops[0].height() < crops[1].height());
        assert!(crops[2].height() < crops[1].height());
    }

    #[test]
    fn furigana_and_horizontal_blocks_are_not_split() {
        let image = DynamicImage::ImageRgb8(RgbImage::new(200, 300));
        let mut mask = GrayImage::new(200, 300);
        paint(&mut mask, 40..70, 20..200);
        paint(&mut mask, 75..82, 30..60); // ruby beside the column
        let one_column = ocr_crops(&image, Some(&mask), &block(30.0, 10.0, 60.0, 200.0));
        assert_eq!(one_column.len(), 1);

        paint(&mut mask, 120..150, 20..200);
        let mut horizontal = block(30.0, 10.0, 130.0, 200.0);
        horizontal.source_direction = Some(TextDirection::Horizontal);
        assert_eq!(ocr_crops(&image, Some(&mask), &horizontal).len(), 1);
        assert_eq!(
            ocr_crops(&image, None, &block(30.0, 10.0, 130.0, 200.0)).len(),
            1
        );
    }
}
