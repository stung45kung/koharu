//! `ocr-check` pipeline step: the loaded vision LLM compares each block's
//! OCR text with the block's crop and fixes misread characters (see
//! `crate::ocr_check`). Runs after OCR and before speaker guessing and
//! translation via the `OcrChecked` artifact.
//!
//! The OCR text each fix replaced goes to the project's `OcrFixes`
//! (`ocr_fixes.toml`), not the scene.
//!
//! It never fails the page: with no LLM it is skipped, and a reading error
//! (e.g. a model without vision) is only logged, keeping the fixes made so
//! far, so translation/inpaint/render still run.

use std::sync::atomic::Ordering;

use anyhow::Result;
use async_trait::async_trait;
use koharu_core::Op;
use koharu_ml::comic_text_detector::crop_text_block_bbox;

use crate::ocr_check::{accept_correction, check_instructions, correction_op, hint_names};
use crate::pipeline::artifacts::Artifact;
use crate::pipeline::engine::{Engine, EngineCtx, EngineInfo};
use crate::pipeline::engines::ocr_columns::{clean_reply, enlarge_small, shrink_for_reading};
use crate::pipeline::engines::support::{
    load_source_image, target_text_nodes, text_node_to_region,
};

pub struct Model;

#[async_trait]
impl Engine for Model {
    async fn run(&self, ctx: EngineCtx<'_>) -> Result<Vec<Op>> {
        if !ctx.llm.ready().await {
            tracing::info!(page = %ctx.page, "ocr-check skipped: no LLM loaded");
            return Ok(Vec::new());
        }
        let texts: Vec<_> = target_text_nodes(&ctx)
            .into_iter()
            .filter(|(_, _, t)| t.text.as_deref().is_some_and(|s| !s.trim().is_empty()))
            .collect();
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let image = load_source_image(ctx.scene, ctx.page, ctx.blobs)?;
        let hints = hint_names(ctx.characters, &ctx.session.glossary());

        let mut ops = Vec::new();
        let mut fixed = Vec::new();
        for (node_id, transform, text) in texts {
            if ctx.cancel.load(Ordering::Relaxed) {
                break;
            }
            let ocr_text = text.text.as_deref().unwrap_or_default();
            let region = text_node_to_region(transform, text);
            let glyph = text.font_prediction.as_ref().map(|p| p.font_size_px);
            let crop = enlarge_small(shrink_for_reading(
                crop_text_block_bbox(&image, &region),
                glyph,
            ));
            let reply = match ctx
                .llm
                .read_image(&crop, &check_instructions(ocr_text, &hints))
                .await
            {
                Ok(reply) => clean_reply(&reply),
                Err(err) => {
                    tracing::warn!(page = %ctx.page, "ocr-check stopped: {err:#}");
                    break;
                }
            };
            match accept_correction(ocr_text, &reply) {
                Some(corrected) => {
                    if let Some(op) = correction_op(ctx.page, node_id, ocr_text, &corrected) {
                        ops.push(op);
                        fixed.push((node_id, ocr_text.to_string()));
                    }
                }
                None => tracing::warn!(
                    page = %ctx.page,
                    block = %node_id,
                    "ocr-check kept the OCR text: the answer looked like a rewrite"
                ),
            }
        }
        // The OCR text each fix replaced, for the badge and "restore".
        if !fixed.is_empty()
            && let Err(err) = ctx.session.update_ocr_fixes(|f| {
                for (node, original) in fixed {
                    f.record(node, original);
                }
            })
        {
            tracing::warn!(page = %ctx.page, "ocr-check could not save the OCR text: {err:#}");
        }
        Ok(ops)
    }
}

inventory::submit! {
    EngineInfo {
        id: "ocr-check",
        name: "Check OCR (loaded vision LLM, e.g. LM Studio)",
        needs: &[Artifact::TextBoxes, Artifact::OcrText],
        produces: &[Artifact::OcrChecked],
        load: |_runtime, _cpu| Box::pin(async move {
            Ok(Box::new(Model) as Box<dyn Engine>)
        }),
    }
}
