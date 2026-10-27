//! Series library — character profiles + glossary shared between projects.
//!
//! - `GET /library`         — saved presets
//! - `POST /library/save`   — save the open project's set under a name
//! - `POST /library/apply`  — merge a preset into the open project
//! - `POST /library/delete` — remove a preset
//!
//! Presets live in `<app data>/library/` (see `koharu_app::library`).

use axum::Json;
use axum::extract::State;
use koharu_app::library::{self, MergeOutcome, PresetSummary};
use koharu_core::CharacterBook;
use serde::{Deserialize, Serialize};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::AppState;
use crate::error::{ApiError, ApiResult};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::default()
        .routes(routes!(list_presets))
        .routes(routes!(save_preset))
        .routes(routes!(apply_preset))
        .routes(routes!(delete_preset))
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PresetNameRequest {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplyPresetRequest {
    pub name: String,
    /// Also replace the project's typesetting defaults (size / colour) with
    /// the preset's, when it has them.
    #[serde(default)]
    pub apply_typesetting: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplyPresetResponse {
    pub outcome: MergeOutcome,
    pub book: CharacterBook,
}

#[utoipa::path(get, path = "/library", responses((status = 200, body = Vec<PresetSummary>)))]
async fn list_presets() -> ApiResult<Json<Vec<PresetSummary>>> {
    let dir = library::default_dir();
    let list = tokio::task::spawn_blocking(move || library::list(&dir))
        .await
        .map_err(|e| ApiError::internal(e.into()))?;
    Ok(Json(list.iter().map(|p| p.summary()).collect()))
}

#[utoipa::path(
    post,
    path = "/library/save",
    request_body = PresetNameRequest,
    responses((status = 200, body = PresetSummary))
)]
async fn save_preset(
    State(app): State<AppState>,
    Json(req): Json<PresetNameRequest>,
) -> ApiResult<Json<PresetSummary>> {
    let session = app
        .current_session()
        .ok_or_else(|| ApiError::bad_request("no project open"))?;
    let name = library::clean_name(&req.name).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let preset = library::from_project(
        name,
        &session.characters(),
        &session.glossary(),
        &session.typesetting(),
    );
    library::save(&library::default_dir(), &preset).map_err(ApiError::internal)?;
    Ok(Json(preset.summary()))
}

#[utoipa::path(
    post,
    path = "/library/apply",
    request_body = ApplyPresetRequest,
    responses((status = 200, body = ApplyPresetResponse))
)]
async fn apply_preset(
    State(app): State<AppState>,
    Json(req): Json<ApplyPresetRequest>,
) -> ApiResult<Json<ApplyPresetResponse>> {
    let session = app
        .current_session()
        .ok_or_else(|| ApiError::bad_request("no project open"))?;
    let preset = library::get(&library::default_dir(), &req.name)
        .ok_or_else(|| ApiError::bad_request(format!("preset {:?} not found", req.name)))?;

    let mut outcome = MergeOutcome::default();
    // Characters go through the session so the merge is one undo step.
    let (_, book) = session
        .update_characters(|book| {
            let mut characters = book.characters.clone();
            library::merge_characters(&mut characters, &preset.characters, &mut outcome);
            book.set_characters(characters);
            Ok(())
        })
        .map_err(ApiError::internal)?;

    let mut glossary = session.glossary();
    library::merge_glossary(&mut glossary, &preset.glossary, &mut outcome);
    if outcome.glossary_added + outcome.glossary_updated > 0 {
        session.set_glossary(glossary).map_err(ApiError::internal)?;
    }

    if req.apply_typesetting
        && let Some(typesetting) = preset.typesetting
    {
        session
            .set_typesetting(typesetting)
            .map_err(ApiError::internal)?;
        outcome.typesetting_applied = true;
    }

    Ok(Json(ApplyPresetResponse { outcome, book }))
}

#[utoipa::path(
    post,
    path = "/library/delete",
    request_body = PresetNameRequest,
    responses((status = 200, body = bool))
)]
async fn delete_preset(Json(req): Json<PresetNameRequest>) -> ApiResult<Json<bool>> {
    let deleted =
        library::delete(&library::default_dir(), &req.name).map_err(ApiError::internal)?;
    Ok(Json(deleted))
}
