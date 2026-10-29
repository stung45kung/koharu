//! OCR by the loaded LLM. Each column of a vertical block (or the whole
//! block) is cropped like for Manga OCR and sent as an image to the LLM
//! provider, which must accept images (e.g. Gemma in LM Studio with its
//! `mmproj` vision file).

use anyhow::Result;
use async_trait::async_trait;
use image::{DynamicImage, imageops::FilterType};
use koharu_core::{NodeDataPatch, NodePatch, Op, TextDataPatch};

use crate::pipeline::artifacts::Artifact;
use crate::pipeline::engine::{Engine, EngineCtx, EngineInfo};
use crate::pipeline::engines::ocr_columns::{TextColors, join_columns, ocr_crops};
use crate::pipeline::engines::support::{
    load_source_image, segment_mask, text_node_to_region, text_nodes,
};

const INSTRUCTIONS: &str = "This image is cut from a manga page and shows lettering \
(one column or block of text). Transcribe the text exactly as written, in reading order \
(vertical Japanese: top to bottom, columns right to left). Keep punctuation such as \
！？… as written. Do not translate, explain, add romanization, quotes or any other words. \
Output only the transcribed text; if there is no text, output nothing.";

/// Crops smaller than this (px, shorter side) are enlarged before sending.
const MIN_SIDE: u32 = 64;

pub struct Model;

#[async_trait]
impl Engine for Model {
    async fn run(&self, ctx: EngineCtx<'_>) -> Result<Vec<Op>> {
        let texts = text_nodes(ctx.scene, ctx.page);
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let image = load_source_image(ctx.scene, ctx.page, ctx.blobs)?;
        let rgb = image.to_rgb8();
        let mask = segment_mask(&ctx)?;
        let per_block: Vec<Vec<DynamicImage>> = texts
            .iter()
            .map(|(_, transform, text)| {
                let region = text_node_to_region(transform, text);
                let colors = text.font_prediction.as_ref().map(|p| TextColors {
                    fill: p.text_color,
                    outline: (p.stroke_width_px > 0.5).then_some(p.stroke_color),
                });
                ocr_crops(&image, &rgb, mask.as_ref(), colors, &region)
                    .into_iter()
                    .map(enlarge_small)
                    .collect()
            })
            .collect();
        let crops: Vec<DynamicImage> = per_block.iter().flatten().cloned().collect();
        let mut read = ctx
            .llm
            .read_images(&crops, INSTRUCTIONS)
            .await?
            .into_iter()
            .map(|reply| clean_reply(&reply));

        let mut ops = Vec::with_capacity(texts.len());
        for ((node_id, _, _), parts) in texts.iter().zip(&per_block) {
            let text = join_columns(read.by_ref().take(parts.len()).collect());
            ops.push(Op::UpdateNode {
                page: ctx.page,
                id: *node_id,
                patch: NodePatch {
                    data: Some(NodeDataPatch::Text(TextDataPatch {
                        text: Some(Some(text)),
                        ..Default::default()
                    })),
                    transform: None,
                    visible: None,
                },
                prev: NodePatch::default(),
            });
        }
        Ok(ops)
    }
}

fn enlarge_small(crop: DynamicImage) -> DynamicImage {
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
fn clean_reply(reply: &str) -> String {
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

inventory::submit! {
    EngineInfo {
        id: "llm-vision-ocr",
        name: "LLM Vision OCR (loaded LLM, e.g. LM Studio)",
        needs: &[Artifact::TextBoxes],
        produces: &[Artifact::OcrText],
        load: |_runtime, _cpu| Box::pin(async move { Ok(Box::new(Model) as Box<dyn Engine>) }),
    }
}

#[cfg(test)]
mod tests {
    use super::clean_reply;

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
