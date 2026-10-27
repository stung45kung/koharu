//! Project typesetting defaults (`.khrproj/typesetting.toml`).
//!
//! - `GET /typesetting` — current defaults
//! - `PUT /typesetting` — replace them (send `{}` to clear)
//! - `POST /typesetting/reset-block-styles` — drop the own style of some
//!   text blocks so they follow their character / the project again
//!
//! The project default *font* is `ProjectMeta.style.defaultFont`, changed
//! through the normal `updateProjectMeta` op.

use axum::Json;
use axum::extract::State;
use koharu_core::{
    NodeDataPatch, NodeId, NodeKind, NodePatch, Op, PageId, TextDataPatch, TypesettingDefaults,
};
use serde::{Deserialize, Serialize};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::AppState;
use crate::error::{ApiError, ApiResult};

const MAX_FONT_SIZE_PX: f32 = 500.0;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::default()
        .routes(routes!(get_typesetting, put_typesetting))
        .routes(routes!(reset_block_styles))
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResetBlockStylesRequest {
    pub page_id: PageId,
    pub node_ids: Vec<NodeId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResetBlockStylesResponse {
    /// Blocks whose own style was removed.
    pub reset: usize,
    /// New scene epoch (`None` when nothing changed).
    pub epoch: Option<u64>,
}

/// Remove the per-block style (font, size, colour, bold/italic, stroke,
/// alignment) of the given text blocks, as one undo step. JSON ops can't
/// express "set style to null", hence this route.
#[utoipa::path(
    post,
    path = "/typesetting/reset-block-styles",
    request_body = ResetBlockStylesRequest,
    responses((status = 200, body = ResetBlockStylesResponse))
)]
async fn reset_block_styles(
    State(app): State<AppState>,
    Json(req): Json<ResetBlockStylesRequest>,
) -> ApiResult<Json<ResetBlockStylesResponse>> {
    let session = app
        .current_session()
        .ok_or_else(|| ApiError::bad_request("no project open"))?;
    let ops = {
        let scene = session.scene.read();
        let page = scene
            .page(req.page_id)
            .ok_or_else(|| ApiError::bad_request(format!("page {} not found", req.page_id)))?;
        req.node_ids
            .iter()
            .filter(|id| {
                page.nodes
                    .get(id)
                    .is_some_and(|n| matches!(&n.kind, NodeKind::Text(t) if t.style.is_some()))
            })
            .map(|id| Op::UpdateNode {
                page: req.page_id,
                id: *id,
                patch: NodePatch {
                    data: Some(NodeDataPatch::Text(TextDataPatch {
                        style: Some(None),
                        ..Default::default()
                    })),
                    transform: None,
                    visible: None,
                },
                prev: NodePatch::default(),
            })
            .collect::<Vec<_>>()
    };
    let reset = ops.len();
    if reset == 0 {
        return Ok(Json(ResetBlockStylesResponse { reset, epoch: None }));
    }
    let epoch = app
        .apply(Op::Batch {
            ops,
            label: "Reset block style".to_string(),
        })
        .map_err(ApiError::internal)?;
    Ok(Json(ResetBlockStylesResponse {
        reset,
        epoch: Some(epoch),
    }))
}

#[utoipa::path(
    get,
    path = "/typesetting",
    responses((status = 200, body = TypesettingDefaults))
)]
async fn get_typesetting(State(app): State<AppState>) -> ApiResult<Json<TypesettingDefaults>> {
    let session = app
        .current_session()
        .ok_or_else(|| ApiError::bad_request("no project open"))?;
    Ok(Json(session.typesetting()))
}

#[utoipa::path(
    put,
    path = "/typesetting",
    request_body = TypesettingDefaults,
    responses((status = 200, body = TypesettingDefaults))
)]
async fn put_typesetting(
    State(app): State<AppState>,
    Json(req): Json<TypesettingDefaults>,
) -> ApiResult<Json<TypesettingDefaults>> {
    let session = app
        .current_session()
        .ok_or_else(|| ApiError::bad_request("no project open"))?;
    if let Some(size) = req.font_size_px
        && !(size.is_finite() && size > 0.0 && size <= MAX_FONT_SIZE_PX)
    {
        return Err(ApiError::bad_request(format!(
            "font size must be between 0 and {MAX_FONT_SIZE_PX} px"
        )));
    }
    session
        .set_typesetting(req.clone())
        .map_err(ApiError::internal)?;
    Ok(Json(req))
}
