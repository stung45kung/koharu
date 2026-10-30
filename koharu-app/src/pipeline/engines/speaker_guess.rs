//! `speaker-guess` pipeline step: suggest a speaker for each text block with
//! the loaded LLM (see `crate::speaker_guess`). Sits between OCR and
//! translation via the `Speakers` artifact.
//!
//! Writes the project's character book, not the scene, so it returns no ops.
//! It never fails the page: with no characters or no LLM it is skipped, and
//! a guessing error is only logged, so translation/inpaint/render still run.

use anyhow::Result;
use async_trait::async_trait;
use koharu_core::Op;

use crate::pipeline::artifacts::Artifact;
use crate::pipeline::engine::{Engine, EngineCtx, EngineInfo};
use crate::speaker_guess::guess_page_speakers;

pub struct Model;

#[async_trait]
impl Engine for Model {
    async fn run(&self, ctx: EngineCtx<'_>) -> Result<Vec<Op>> {
        if ctx.characters.characters.is_empty() {
            tracing::debug!(page = %ctx.page, "speaker-guess skipped: no characters");
            return Ok(Vec::new());
        }
        if !ctx.llm.ready().await {
            tracing::info!(page = %ctx.page, "speaker-guess skipped: no LLM loaded");
            return Ok(Vec::new());
        }
        match guess_page_speakers(
            ctx.session,
            ctx.llm,
            ctx.page,
            ctx.options.target_language.as_deref(),
        )
        .await
        {
            Ok(outcome) => tracing::info!(
                page = %ctx.page,
                considered = outcome.considered,
                assigned = outcome.assigned,
                "speakers guessed"
            ),
            Err(err) => tracing::warn!(page = %ctx.page, "speaker-guess failed: {err:#}"),
        }
        Ok(Vec::new())
    }
}

inventory::submit! {
    EngineInfo {
        id: "speaker-guess",
        name: "Guess speakers (LLM)",
        // `OcrChecked` runs `ocr-check` (when selected) first.
        needs: &[Artifact::OcrText, Artifact::OcrChecked],
        produces: &[Artifact::Speakers],
        load: |_runtime, _cpu| Box::pin(async move {
            Ok(Box::new(Model) as Box<dyn Engine>)
        }),
    }
}
