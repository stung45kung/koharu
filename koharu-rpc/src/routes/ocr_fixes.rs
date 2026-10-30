//! OCR text replaced by the LLM OCR check (`.khrproj/ocr_fixes.toml`).
//!
//! - `GET /ocr-fixes` — the OCR text each fixed block had before the check
//! - `POST /ocr-fixes/forget` — drop the entries of some blocks (after the
//!   user restored the OCR text, which the UI applies as a normal op)

use axum::Json;
use axum::extract::State;
use koharu_core::{NodeId, OcrFixes};
use serde::{Deserialize, Serialize};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::AppState;
use crate::error::{ApiError, ApiResult};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::default()
        .routes(routes!(get_ocr_fixes))
        .routes(routes!(forget_ocr_fixes))
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ForgetOcrFixesRequest {
    pub node_ids: Vec<NodeId>,
}

#[utoipa::path(get, path = "/ocr-fixes", responses((status = 200, body = OcrFixes)))]
async fn get_ocr_fixes(State(app): State<AppState>) -> ApiResult<Json<OcrFixes>> {
    let session = app
        .current_session()
        .ok_or_else(|| ApiError::bad_request("no project open"))?;
    Ok(Json(session.ocr_fixes()))
}

#[utoipa::path(
    post,
    path = "/ocr-fixes/forget",
    request_body = ForgetOcrFixesRequest,
    responses((status = 200, body = OcrFixes))
)]
async fn forget_ocr_fixes(
    State(app): State<AppState>,
    Json(req): Json<ForgetOcrFixesRequest>,
) -> ApiResult<Json<OcrFixes>> {
    let session = app
        .current_session()
        .ok_or_else(|| ApiError::bad_request("no project open"))?;
    let fixes = session
        .update_ocr_fixes(|f| f.forget(&req.node_ids))
        .map_err(ApiError::internal)?;
    Ok(Json(fixes))
}
