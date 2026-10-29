//! Grow detected text boxes over lettering they cut off.
//!
//! Detectors sometimes stop a box a glyph or two short of the end of a
//! column ("…営業や" with "さかい" left below the box). The text then is
//! neither masked for inpainting nor read by OCR. The segmentation
//! network's raw mask covers all lettering on the page, so a box edge that
//! cuts through mask ink is pushed out until the ink ends.

use image::GrayImage;

/// Box as `[x1, y1, x2, y2]`, `x2` / `y2` exclusive.
pub type PixelBox = [u32; 4];

/// Growth per side is capped at this share of the box's size on that axis.
const MAX_GROWTH_SHARE: f32 = 0.6;

/// `bx` grown over the mask ink that crosses its edges, never into one of
/// `others`. `threshold`: mask values above it are ink.
pub fn grow_box(mask: &GrayImage, threshold: u8, bx: PixelBox, others: &[PixelBox]) -> PixelBox {
    let (w, h) = mask.dimensions();
    let [mut x1, mut y1, mut x2, mut y2] = bx;
    let [ox1, oy1, ox2, oy2] = bx;
    if x2 <= x1 || y2 <= y1 || x2 > w || y2 > h {
        return bx;
    }
    let max_dx = ((ox2 - ox1) as f32 * MAX_GROWTH_SHARE) as u32;
    let max_dy = ((oy2 - oy1) as f32 * MAX_GROWTH_SHARE) as u32;
    // Empty rows / columns allowed between two inked ones (between glyphs).
    let tolerance = (((ox2 - ox1).min(oy2 - oy1)) as f32 * 0.25).clamp(4.0, 24.0) as u32;

    let ink = |x: u32, y: u32| mask.get_pixel(x, y)[0] > threshold;
    let row_inked = |y: u32, x1: u32, x2: u32| (x1..x2).any(|x| ink(x, y));
    let col_inked = |x: u32, y1: u32, y2: u32| (y1..y2).any(|y| ink(x, y));
    // Whether a box would run into another one.
    let blocked = |b: PixelBox| {
        others
            .iter()
            .any(|o| b[0] < o[2] && o[0] < b[2] && b[1] < o[3] && o[1] < b[3])
    };

    for _ in 0..3 {
        let before = [x1, y1, x2, y2];

        // Down.
        if y2 < h && row_inked(y2, x1, x2) {
            let (mut y, mut gap, mut end) = (y2, 0, y2);
            while y < h && y - oy2 < max_dy && gap <= tolerance {
                if row_inked(y, x1, x2) {
                    end = y + 1;
                    gap = 0;
                } else {
                    gap += 1;
                }
                y += 1;
            }
            while end > y2 && blocked([x1, y1, x2, end]) {
                end -= 1;
            }
            y2 = end;
        }
        // Up.
        if y1 > 0 && row_inked(y1 - 1, x1, x2) {
            let (mut y, mut gap, mut start) = (y1, 0, y1);
            while y > 0 && oy1 - (y - 1) <= max_dy && gap <= tolerance {
                y -= 1;
                if row_inked(y, x1, x2) {
                    start = y;
                    gap = 0;
                } else {
                    gap += 1;
                }
            }
            while start < y1 && blocked([x1, start, x2, y2]) {
                start += 1;
            }
            y1 = start;
        }
        // Right.
        if x2 < w && col_inked(x2, y1, y2) {
            let (mut x, mut gap, mut end) = (x2, 0, x2);
            while x < w && x - ox2 < max_dx && gap <= tolerance {
                if col_inked(x, y1, y2) {
                    end = x + 1;
                    gap = 0;
                } else {
                    gap += 1;
                }
                x += 1;
            }
            while end > x2 && blocked([x1, y1, end, y2]) {
                end -= 1;
            }
            x2 = end;
        }
        // Left.
        if x1 > 0 && col_inked(x1 - 1, y1, y2) {
            let (mut x, mut gap, mut start) = (x1, 0, x1);
            while x > 0 && ox1 - (x - 1) <= max_dx && gap <= tolerance {
                x -= 1;
                if col_inked(x, y1, y2) {
                    start = x;
                    gap = 0;
                } else {
                    gap += 1;
                }
            }
            while start < x1 && blocked([start, y1, x2, y2]) {
                start += 1;
            }
            x1 = start;
        }

        if [x1, y1, x2, y2] == before {
            break;
        }
    }
    [x1, y1, x2, y2]
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Luma;

    fn paint(mask: &mut GrayImage, x: std::ops::Range<u32>, y: std::ops::Range<u32>) {
        for yy in y {
            for xx in x.clone() {
                mask.put_pixel(xx, yy, Luma([255]));
            }
        }
    }

    #[test]
    fn a_column_cut_short_is_followed_to_its_end() {
        let mut mask = GrayImage::new(200, 400);
        // Glyphs 30 px tall with 4 px between, from y=20 to y=288.
        for g in 0..8 {
            paint(&mut mask, 50..80, 20 + g * 34..50 + g * 34);
        }
        // Box stops halfway through the 6th glyph.
        let grown = grow_box(&mask, 60, [45, 15, 85, 200], &[]);
        assert_eq!(grown, [45, 15, 85, 288]);
    }

    #[test]
    fn growth_stops_at_gaps_other_boxes_and_the_cap() {
        let mut mask = GrayImage::new(200, 400);
        paint(&mut mask, 50..80, 20..150);
        // Separate text 40 px below the column end: not part of the block.
        paint(&mut mask, 50..80, 190..230);
        assert_eq!(grow_box(&mask, 60, [45, 15, 85, 150], &[])[3], 150);

        // Ink running on below the box, but another box starts at y=170.
        let mut mask = GrayImage::new(200, 400);
        paint(&mut mask, 50..80, 20..300);
        let grown = grow_box(&mask, 60, [45, 15, 85, 150], &[[40, 170, 90, 260]]);
        assert_eq!(grown[3], 170);

        // Without the other box, growth is capped at 60% of the height.
        let grown = grow_box(&mask, 60, [45, 15, 85, 150], &[]);
        assert_eq!(grown[3], 150 + 81);
    }

    #[test]
    fn untouched_edges_stay_put() {
        let mut mask = GrayImage::new(200, 400);
        paint(&mut mask, 50..80, 20..150);
        assert_eq!(
            grow_box(&mask, 60, [40, 10, 90, 160], &[]),
            [40, 10, 90, 160]
        );
    }
}
