//! Comic Text Detector (segmentation-only). Needs text boxes from another
//! detector; produces a refined `Mask { Segment }` layer.

use anyhow::Result;
use async_trait::async_trait;
use image::DynamicImage;
use koharu_core::{MaskRole, NodePatch, Op, Transform};
use koharu_ml::comic_text_detector::{ComicTextDetector, refine_segmentation_mask};
use koharu_ml::types::TextRegion;

use crate::pipeline::artifacts::Artifact;
use crate::pipeline::engine::{Engine, EngineCtx, EngineInfo};
use crate::pipeline::engines::grow_boxes::{PixelBox, grow_box};
use crate::pipeline::engines::support::{
    load_source_image, text_node_to_region, text_nodes, upsert_mask_blob,
};

/// Raw mask values above this are lettering (the detector's own cut-off).
const MASK_THRESHOLD: u8 = 60;

pub struct Model(ComicTextDetector);

#[async_trait]
impl Engine for Model {
    async fn run(&self, ctx: EngineCtx<'_>) -> Result<Vec<Op>> {
        let image = load_source_image(ctx.scene, ctx.page, ctx.blobs)?;
        let prob_mask = self.0.inference_segmentation(&image)?;

        // Grow detected boxes over lettering they cut off (blocks the user
        // drew or moved keep their box), so it is masked and read too.
        let nodes = text_nodes(ctx.scene, ctx.page);
        let boxes: Vec<PixelBox> = nodes
            .iter()
            .map(|(_, t, _)| {
                let (w, h) = prob_mask.dimensions();
                let x1 = (t.x.max(0.0).floor() as u32).min(w);
                let y1 = (t.y.max(0.0).floor() as u32).min(h);
                let x2 = ((t.x + t.width).ceil().max(0.0) as u32).clamp(x1, w);
                let y2 = ((t.y + t.height).ceil().max(0.0) as u32).clamp(y1, h);
                [x1, y1, x2, y2]
            })
            .collect();
        let mut ops = Vec::new();
        let mut regions: Vec<TextRegion> = Vec::with_capacity(nodes.len());
        for (i, (id, transform, text)) in nodes.iter().enumerate() {
            let mut region = text_node_to_region(transform, text);
            let rotated = text.rotation_deg.is_some_and(|r| r.abs() > 3.0)
                || transform.rotation_deg.abs() > 0.5;
            if !text.lock_layout_box && !rotated {
                let others: Vec<PixelBox> = boxes
                    .iter()
                    .enumerate()
                    .filter(|(j, _)| *j != i)
                    .map(|(_, b)| *b)
                    .collect();
                let grown = grow_box(&prob_mask, MASK_THRESHOLD, boxes[i], &others);
                if grown != boxes[i] {
                    let [x1, y1, x2, y2] = grown;
                    let new_transform = Transform {
                        x: transform.x.min(x1 as f32),
                        y: transform.y.min(y1 as f32),
                        width: 0.0,
                        height: 0.0,
                        rotation_deg: transform.rotation_deg,
                    };
                    let right = (transform.x + transform.width).max(x2 as f32);
                    let bottom = (transform.y + transform.height).max(y2 as f32);
                    let new_transform = Transform {
                        width: right - new_transform.x,
                        height: bottom - new_transform.y,
                        ..new_transform
                    };
                    region.x = new_transform.x;
                    region.y = new_transform.y;
                    region.width = new_transform.width;
                    region.height = new_transform.height;
                    ops.push(Op::UpdateNode {
                        page: ctx.page,
                        id: *id,
                        patch: NodePatch {
                            transform: Some(new_transform),
                            ..Default::default()
                        },
                        prev: NodePatch::default(),
                    });
                }
            }
            regions.push(region);
        }

        let mask = refine_segmentation_mask(&image, &prob_mask, &regions);
        let mask_blob = ctx.blobs.put_webp(&DynamicImage::ImageLuma8(mask))?;

        ops.push(upsert_mask_blob(
            ctx.scene,
            ctx.page,
            MaskRole::Segment,
            mask_blob,
        ));
        Ok(ops)
    }
}

inventory::submit! {
    EngineInfo {
        id: "comic-text-detector-seg",
        name: "Comic Text Detector (Segmentation)",
        needs: &[Artifact::TextBoxes],
        produces: &[Artifact::SegmentMask],
        load: |runtime, cpu| Box::pin(async move {
            let m = ComicTextDetector::load_segmentation_only(runtime, cpu).await?;
            Ok(Box::new(Model(m)) as Box<dyn Engine>)
        }),
    }
}
