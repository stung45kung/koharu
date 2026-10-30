//! PaddleOCR-VL. Vision-language OCR driven by llama.cpp + mtmd.
//!
//! Each text node on the page is cropped out of the source image, passed
//! through the multimodal model, and the recognised text is written back
//! via `UpdateNode { TextDataPatch { text } }`.

use std::sync::Mutex;

use anyhow::Result;
use async_trait::async_trait;
use koharu_core::Op;
use koharu_llm::paddleocr_vl::{PaddleOcrVl, PaddleOcrVlTask};

use crate::app::shared_llama_backend;
use crate::pipeline::artifacts::Artifact;
use crate::pipeline::engine::{Engine, EngineCtx, EngineInfo};
use crate::pipeline::engines::ocr_columns::{
    TextColors, join_columns, ocr_crops, shrink_for_reading,
};
use crate::pipeline::engines::support::{
    load_source_image, ocr_text_op, segment_mask, target_text_nodes, text_node_to_region,
};

const MAX_NEW_TOKENS: usize = 256;

pub struct Model(Mutex<PaddleOcrVl>);

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
        // Vertical blocks are read column by column (see `ocr_columns`).
        let per_block: Vec<Vec<_>> = texts
            .iter()
            .map(|(_, transform, text)| {
                let region = text_node_to_region(transform, text);
                let colors = text.font_prediction.as_ref().map(|p| TextColors {
                    fill: p.text_color,
                    outline: (p.stroke_width_px > 0.5).then_some(p.stroke_color),
                });
                // Large glyphs (high-resolution pages) make the model read
                // across columns: shrink to its comfortable size.
                let glyph = text.font_prediction.as_ref().map(|p| p.font_size_px);
                ocr_crops(&image, &rgb, mask.as_ref(), colors, &region)
                    .into_iter()
                    .map(|c| shrink_for_reading(c, glyph))
                    .collect::<Vec<_>>()
            })
            .collect();
        let regions: Vec<_> = per_block.iter().flatten().cloned().collect();

        let outputs = {
            let mut ocr = self
                .0
                .lock()
                .map_err(|_| anyhow::anyhow!("PaddleOCR mutex poisoned"))?;
            ocr.inference_images(&regions, PaddleOcrVlTask::Ocr, MAX_NEW_TOKENS)?
        };
        let mut read = outputs.into_iter().map(|out| out.text);
        let outputs: Vec<String> = per_block
            .iter()
            .map(|parts| join_columns(read.by_ref().take(parts.len()).collect()))
            .collect();

        let mut ops = Vec::with_capacity(texts.len());
        for ((node_id, _, _), out) in texts.iter().zip(outputs) {
            ops.push(ocr_text_op(ctx.page, *node_id, out));
        }
        Ok(ops)
    }
}

inventory::submit! {
    EngineInfo {
        id: "paddle-ocr-vl-1.6",
        name: "PaddleOCR-VL",
        // The segmentation mask and font colours (when produced in the same
        // run) guide the column split, so OCR runs after them.
        needs: &[
            Artifact::TextBoxes,
            Artifact::SegmentMask,
            Artifact::FontPredictions,
        ],
        produces: &[Artifact::OcrText],
        load: |runtime, cpu| Box::pin(async move {
            let backend = shared_llama_backend(runtime)?;
            let m = PaddleOcrVl::load(runtime, cpu, backend).await?;
            Ok(Box::new(Model(Mutex::new(m))) as Box<dyn Engine>)
        }),
    }
}
