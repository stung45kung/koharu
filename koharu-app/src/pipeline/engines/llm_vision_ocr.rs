//! OCR by the loaded LLM. Each column of a vertical block (or the whole
//! block) is cropped like for Manga OCR and sent as an image to the LLM
//! provider, which must accept images (e.g. Gemma in LM Studio with its
//! `mmproj` vision file).

use anyhow::Result;
use async_trait::async_trait;
use image::DynamicImage;
use koharu_core::Op;

use koharu_ml::comic_text_detector::crop_text_block_bbox;

use crate::pipeline::artifacts::Artifact;
use crate::pipeline::engine::{Engine, EngineCtx, EngineInfo};
use crate::pipeline::engines::ocr_columns::{
    TextColors, clean_reply, enlarge_small, join_columns, ocr_crops, shrink_for_reading,
};
use crate::pipeline::engines::support::{
    load_source_image, ocr_text_op, segment_mask, target_text_nodes, text_node_to_region,
};

const INSTRUCTIONS: &str = "This image is cut from a manga page and shows lettering \
(one column or a block of text). Transcribe exactly the characters that are visible, in \
reading order (vertical Japanese: top to bottom, columns right to left). Copy every \
character as drawn: do not add, drop, repeat or correct characters, do not complete words, \
and keep punctuation such as ！？… only where it is drawn. Do not translate, explain, add \
romanization, quotes or any other words. Output only the transcribed text; if there is no \
text, output nothing.";

/// `columns`: send each column of a vertical block on its own; otherwise
/// the whole block in one image.
pub struct Model {
    columns: bool,
}

#[async_trait]
impl Engine for Model {
    async fn run(&self, ctx: EngineCtx<'_>) -> Result<Vec<Op>> {
        // Only the requested blocks when some are named (OCR one block).
        let texts = target_text_nodes(&ctx);
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
                let crops = if self.columns {
                    ocr_crops(&image, &rgb, mask.as_ref(), colors, &region)
                } else {
                    vec![crop_text_block_bbox(&image, &region)]
                };
                let glyph = text.font_prediction.as_ref().map(|p| p.font_size_px);
                crops
                    .into_iter()
                    .map(|c| enlarge_small(shrink_for_reading(c, glyph)))
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
            ops.push(ocr_text_op(ctx.page, *node_id, text));
        }
        Ok(ops)
    }
}

inventory::submit! {
    EngineInfo {
        id: "llm-vision-ocr",
        name: "LLM Vision OCR, column by column (loaded LLM, e.g. LM Studio)",
        // The segmentation mask and font colours (when produced in the same
        // run) guide the column split, so OCR runs after them.
        needs: &[
            Artifact::TextBoxes,
            Artifact::SegmentMask,
            Artifact::FontPredictions,
        ],
        produces: &[Artifact::OcrText],
        load: |_runtime, _cpu| {
            Box::pin(async move { Ok(Box::new(Model { columns: true }) as Box<dyn Engine>) })
        },
    }
}

inventory::submit! {
    EngineInfo {
        id: "llm-vision-ocr-block",
        name: "LLM Vision OCR, whole block (loaded LLM, e.g. LM Studio)",
        // The segmentation mask and font colours (when produced in the same
        // run) guide the column split, so OCR runs after them.
        needs: &[
            Artifact::TextBoxes,
            Artifact::SegmentMask,
            Artifact::FontPredictions,
        ],
        produces: &[Artifact::OcrText],
        load: |_runtime, _cpu| {
            Box::pin(async move { Ok(Box::new(Model { columns: false }) as Box<dyn Engine>) })
        },
    }
}
