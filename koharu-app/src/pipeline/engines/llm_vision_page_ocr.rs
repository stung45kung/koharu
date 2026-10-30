//! OCR of a whole page by the loaded LLM in one request. The page is sent
//! with every text block framed in red and numbered, and the model answers
//! one `[n] text` line per block. Seeing the whole page (glyphs at a
//! comfortable size, the scene around them) lets a vision model such as
//! Gemma read blocks it gets wrong from tight crops.

use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;
use image::{DynamicImage, Rgb, RgbImage, imageops::FilterType};
use koharu_core::{NodeDataPatch, NodePatch, Op, TextDataPatch};

use crate::pipeline::artifacts::Artifact;
use crate::pipeline::engine::{Engine, EngineCtx, EngineInfo};
use crate::pipeline::engines::support::{load_source_image, target_text_nodes};

/// Longest side of the image sent (the model shrinks larger ones anyway,
/// and the labels are drawn for this size).
const MAX_SIDE: u32 = 2048;
const FRAME: Rgb<u8> = Rgb([230, 0, 0]);
const LABEL_BG: Rgb<u8> = Rgb([255, 255, 255]);
const LABEL_FG: Rgb<u8> = Rgb([0, 0, 0]);

fn instructions(count: usize) -> String {
    format!(
        "This is a manga page. Red boxes with white number labels mark the text blocks 1 to \
{count}. For each box, transcribe exactly the text inside it, in reading order (vertical \
Japanese: top to bottom, columns right to left). Ignore text outside the boxes and the \
number labels themselves. Copy the characters as drawn: do not add, drop, repeat, correct \
or complete anything, and do not translate or explain. Answer with exactly {count} lines, \
one per box in number order, each starting with the box number in square brackets, for \
example:\n[1] text of box 1\n[2] text of box 2\nIf a box has no text, write its number \
alone."
    )
}

pub struct Model;

#[async_trait]
impl Engine for Model {
    async fn run(&self, ctx: EngineCtx<'_>) -> Result<Vec<Op>> {
        // Only the requested blocks when some are named (OCR one block).
        let texts = target_text_nodes(&ctx);
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let image = load_source_image(ctx.scene, ctx.page, ctx.blobs)?;
        let boxes: Vec<[f32; 4]> = texts
            .iter()
            .map(|(_, t, _)| [t.x, t.y, t.width, t.height])
            .collect();
        let marked = DynamicImage::ImageRgb8(mark_blocks(&image, &boxes));
        let reply = ctx
            .llm
            .read_images(&[marked], &instructions(texts.len()))
            .await?
            .pop()
            .unwrap_or_default();
        let mut read = parse_numbered(&reply);

        Ok(texts
            .iter()
            .enumerate()
            .map(|(i, (node_id, _, _))| Op::UpdateNode {
                page: ctx.page,
                id: *node_id,
                patch: NodePatch {
                    data: Some(NodeDataPatch::Text(TextDataPatch {
                        text: Some(Some(read.remove(&(i + 1)).unwrap_or_default())),
                        ..Default::default()
                    })),
                    transform: None,
                    visible: None,
                },
                prev: NodePatch::default(),
            })
            .collect())
    }
}

/// The page (shrunk to [`MAX_SIDE`]) with each box `[x, y, w, h]` framed
/// and labelled 1, 2, … in order.
fn mark_blocks(image: &DynamicImage, boxes: &[[f32; 4]]) -> RgbImage {
    let long = image.width().max(image.height()).max(1);
    let scale = (MAX_SIDE as f32 / long as f32).min(1.0);
    let mut page = if scale < 1.0 {
        image
            .resize(
                ((image.width() as f32 * scale).round() as u32).max(1),
                ((image.height() as f32 * scale).round() as u32).max(1),
                FilterType::Triangle,
            )
            .to_rgb8()
    } else {
        image.to_rgb8()
    };
    let (w, h) = page.dimensions();
    let long = w.max(h);
    let thickness = (long / 600).max(2);
    let cell = (long / 300).max(3);

    for (i, b) in boxes.iter().enumerate() {
        let x1 = (b[0] * scale).max(0.0) as u32;
        let y1 = (b[1] * scale).max(0.0) as u32;
        let x2 = (((b[0] + b[2]) * scale).max(0.0) as u32).min(w.saturating_sub(1));
        let y2 = (((b[1] + b[3]) * scale).max(0.0) as u32).min(h.saturating_sub(1));
        if x2 <= x1 || y2 <= y1 {
            continue;
        }
        for t in 0..thickness {
            let (ax, ay) = (x1.saturating_sub(t), y1.saturating_sub(t));
            let (bx, by) = ((x2 + t).min(w - 1), (y2 + t).min(h - 1));
            for x in ax..=bx {
                page.put_pixel(x, ay, FRAME);
                page.put_pixel(x, by, FRAME);
            }
            for y in ay..=by {
                page.put_pixel(ax, y, FRAME);
                page.put_pixel(bx, y, FRAME);
            }
        }
        // Label above the top-left corner, or inside it at the page top.
        let digits = (i + 1).to_string();
        let label_w = cell * (4 * digits.len() as u32 + 1);
        let label_h = cell * 7;
        let lx = x1.min(w.saturating_sub(label_w));
        let ly = if y1 >= label_h + thickness {
            y1 - label_h - thickness
        } else {
            y1 + thickness
        };
        fill(&mut page, lx, ly, label_w, label_h, FRAME);
        fill(
            &mut page,
            lx + 1,
            ly + 1,
            label_w.saturating_sub(2),
            label_h.saturating_sub(2),
            LABEL_BG,
        );
        for (k, ch) in digits.chars().enumerate() {
            draw_digit(
                &mut page,
                ch,
                lx + cell + k as u32 * 4 * cell,
                ly + cell,
                cell,
            );
        }
    }
    page
}

fn fill(page: &mut RgbImage, x: u32, y: u32, w: u32, h: u32, colour: Rgb<u8>) {
    let (pw, ph) = page.dimensions();
    for yy in y..(y + h).min(ph) {
        for xx in x..(x + w).min(pw) {
            page.put_pixel(xx, yy, colour);
        }
    }
}

/// 3×5 bitmap digit at `(x, y)`, each dot `cell` px square.
fn draw_digit(page: &mut RgbImage, ch: char, x: u32, y: u32, cell: u32) {
    const FONT: [[u8; 5]; 10] = [
        [0b111, 0b101, 0b101, 0b101, 0b111],
        [0b010, 0b110, 0b010, 0b010, 0b111],
        [0b111, 0b001, 0b111, 0b100, 0b111],
        [0b111, 0b001, 0b111, 0b001, 0b111],
        [0b101, 0b101, 0b111, 0b001, 0b001],
        [0b111, 0b100, 0b111, 0b001, 0b111],
        [0b111, 0b100, 0b111, 0b101, 0b111],
        [0b111, 0b001, 0b001, 0b001, 0b001],
        [0b111, 0b101, 0b111, 0b101, 0b111],
        [0b111, 0b101, 0b111, 0b001, 0b111],
    ];
    let Some(d) = ch.to_digit(10) else { return };
    for (row, bits) in FONT[d as usize].iter().enumerate() {
        for col in 0..3u32 {
            if bits & (0b100 >> col) != 0 {
                fill(
                    page,
                    x + col * cell,
                    y + row as u32 * cell,
                    cell,
                    cell,
                    LABEL_FG,
                );
            }
        }
    }
}

/// `[n] text` lines of a reply (also `n:` / `n.`), lines without a number
/// joined to the previous block. Quotes and code fences are dropped.
fn parse_numbered(reply: &str) -> HashMap<usize, String> {
    let mut out: HashMap<usize, String> = HashMap::new();
    let mut current = None;
    for line in reply.lines() {
        let line = line.trim().trim_matches('`').trim();
        if line.is_empty() {
            continue;
        }
        if let Some((n, rest)) = numbered(line) {
            current = Some(n);
            out.insert(n, clean(rest));
        } else if let Some(n) = current {
            out.entry(n).or_default().push_str(&clean(line));
        }
    }
    out
}

fn numbered(line: &str) -> Option<(usize, &str)> {
    let (digits, rest) = if let Some(inner) = line.strip_prefix('[') {
        let end = inner.find(']')?;
        (&inner[..end], &inner[end + 1..])
    } else {
        let end = line.find(|c: char| !c.is_ascii_digit())?;
        let rest = line[end..].strip_prefix([':', '.', ')'])?;
        (&line[..end], rest)
    };
    let n = digits.trim().parse().ok().filter(|n| *n > 0)?;
    Some((n, rest.trim()))
}

fn clean(text: &str) -> String {
    let text = text.trim();
    let text = text
        .strip_prefix('「')
        .and_then(|t| t.strip_suffix('」'))
        .unwrap_or(text);
    let text = text
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .unwrap_or(text);
    text.trim().to_string()
}

inventory::submit! {
    EngineInfo {
        id: "llm-vision-ocr-page",
        name: "LLM Vision OCR, whole page (loaded LLM, e.g. LM Studio)",
        needs: &[Artifact::TextBoxes],
        produces: &[Artifact::OcrText],
        load: |_runtime, _cpu| Box::pin(async move { Ok(Box::new(Model) as Box<dyn Engine>) }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbered_lines_are_matched_to_blocks() {
        let reply = "```\n[1] へへ…似合ってるでぇ\n嬢ちゃん\n[2] 「ウチは昼、持ち帰りの」\n3. 作ったもん\n[4]\n```";
        let read = parse_numbered(reply);
        assert_eq!(read[&1], "へへ…似合ってるでぇ嬢ちゃん");
        assert_eq!(read[&2], "ウチは昼、持ち帰りの");
        assert_eq!(read[&3], "作ったもん");
        assert_eq!(read[&4], "");
        assert!(!read.contains_key(&5));
    }

    #[test]
    fn blocks_are_framed_and_labelled() {
        let image = DynamicImage::ImageRgb8(RgbImage::from_pixel(400, 300, Rgb([200, 200, 200])));
        let marked = mark_blocks(&image, &[[50.0, 60.0, 100.0, 80.0], [0.0, 0.0, 40.0, 40.0]]);
        assert_eq!(marked.dimensions(), (400, 300));
        assert_eq!(*marked.get_pixel(100, 60), FRAME);
        assert_eq!(*marked.get_pixel(50, 100), FRAME);
        // Label of block 1 above its box; block 2's label inside (page top).
        assert!((0..60).any(|y| *marked.get_pixel(55, y) == LABEL_BG));
        assert!((0..40).any(|y| *marked.get_pixel(5, y) == LABEL_BG));

        let big = DynamicImage::ImageRgb8(RgbImage::new(4096, 3000));
        assert_eq!(mark_blocks(&big, &[]).width(), 2048);
    }
}
