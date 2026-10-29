//! Manga OCR. Each text node's bbox (or each column of a vertical block)
//! is cropped and read; the recognised text is written back via
//! `UpdateNode`.

use anyhow::Result;
use async_trait::async_trait;
use image::DynamicImage;
use koharu_core::{NodeDataPatch, NodePatch, Op, TextDataPatch};
use koharu_ml::manga_ocr::MangaOcr;

use crate::pipeline::artifacts::Artifact;
use crate::pipeline::engine::{Engine, EngineCtx, EngineInfo};
use crate::pipeline::engines::ocr_columns::{TextColors, join_columns, ocr_crops};
use crate::pipeline::engines::support::{
    load_source_image, segment_mask, text_node_to_region, text_nodes,
};

pub struct Model(MangaOcr);

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
        // Vertical blocks are read column by column (see `ocr_columns`).
        let per_block: Vec<Vec<DynamicImage>> = texts
            .iter()
            .map(|(_, transform, text)| {
                let region = text_node_to_region(transform, text);
                let colors = text.font_prediction.as_ref().map(|p| TextColors {
                    fill: p.text_color,
                    outline: (p.stroke_width_px > 0.5).then_some(p.stroke_color),
                });
                ocr_crops(&image, &rgb, mask.as_ref(), colors, &region)
            })
            .collect();
        let crops: Vec<DynamicImage> = per_block.iter().flatten().cloned().collect();
        let mut read = self.0.inference(&crops)?.into_iter();
        let recognised: Vec<String> = per_block
            .iter()
            .map(|parts| join_columns(read.by_ref().take(parts.len()).collect()))
            .collect();

        let mut ops = Vec::with_capacity(texts.len());
        for ((node_id, _, _), text) in texts.iter().zip(recognised) {
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

inventory::submit! {
    EngineInfo {
        id: "manga-ocr",
        name: "Manga OCR",
        needs: &[Artifact::TextBoxes],
        produces: &[Artifact::OcrText],
        load: |runtime, cpu| Box::pin(async move {
            let m = MangaOcr::load(runtime, cpu).await?;
            Ok(Box::new(Model(m)) as Box<dyn Engine>)
        }),
    }
}
