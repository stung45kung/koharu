//! Project typesetting defaults (`.khrproj/typesetting.toml`).
//!
//! - `GET /typesetting` — current defaults
//! - `PUT /typesetting` — replace them (send `{}` to clear)
//!
//! The project default *font* is `ProjectMeta.style.defaultFont`, changed
//! through the normal `updateProjectMeta` op.

use axum::Json;
use axum::extract::State;
use koharu_core::TypesettingDefaults;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::AppState;
use crate::error::{ApiError, ApiResult};

const MAX_FONT_SIZE_PX: f32 = 500.0;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::default().routes(routes!(get_typesetting, put_typesetting))
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
