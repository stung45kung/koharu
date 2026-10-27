//! Central library of characters and glossary terms shared by all projects.
//!
//! - `GET /library`         — every saved character and term
//! - `POST /library/save`   — save chosen characters / terms of the open project
//! - `POST /library/apply`  — add chosen library items to the open project
//! - `POST /library/delete` — remove library items
//!
//! Stored in `<app data>/library/library.toml` (see `koharu_app::library`).

use std::collections::HashSet;

use axum::Json;
use axum::extract::State;
use koharu_app::library::{self, Library, MergeOutcome};
use koharu_core::{CharacterBook, CharacterId};
use serde::{Deserialize, Serialize};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::AppState;
use crate::error::{ApiError, ApiResult};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::default()
        .routes(routes!(get_library))
        .routes(routes!(save_to_library))
        .routes(routes!(apply_from_library))
        .routes(routes!(delete_from_library))
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SaveToLibraryRequest {
    /// Group label (e.g. the series name). May be empty.
    #[serde(default)]
    pub series: String,
    /// Project characters to save.
    #[serde(default)]
    pub character_ids: Vec<CharacterId>,
    /// Source terms of project glossary entries to save.
    #[serde(default)]
    pub glossary_sources: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SaveToLibraryResponse {
    pub library: Library,
    pub added: usize,
    pub updated: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LibraryKeysRequest {
    #[serde(default)]
    pub character_keys: Vec<Uuid>,
    #[serde(default)]
    pub term_keys: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplyFromLibraryResponse {
    pub outcome: MergeOutcome,
    pub book: CharacterBook,
}

async fn load() -> ApiResult<Library> {
    tokio::task::spawn_blocking(|| library::load(&library::default_dir()))
        .await
        .map_err(|e| ApiError::internal(e.into()))
}

#[utoipa::path(get, path = "/library", responses((status = 200, body = Library)))]
async fn get_library() -> ApiResult<Json<Library>> {
    Ok(Json(load().await?))
}

#[utoipa::path(
    post,
    path = "/library/save",
    request_body = SaveToLibraryRequest,
    responses((status = 200, body = SaveToLibraryResponse))
)]
async fn save_to_library(
    State(app): State<AppState>,
    Json(req): Json<SaveToLibraryRequest>,
) -> ApiResult<Json<SaveToLibraryResponse>> {
    let session = app
        .current_session()
        .ok_or_else(|| ApiError::bad_request("no project open"))?;
    let series =
        library::clean_series(&req.series).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let ids: HashSet<CharacterId> = req.character_ids.into_iter().collect();
    let characters: Vec<_> = session
        .characters()
        .characters
        .into_iter()
        .filter(|c| ids.contains(&c.id))
        .collect();
    let sources: HashSet<String> = req
        .glossary_sources
        .iter()
        .map(|s| s.trim().to_lowercase())
        .collect();
    let entries: Vec<_> = session
        .glossary()
        .entries
        .into_iter()
        .filter(|e| sources.contains(&e.source.trim().to_lowercase()))
        .collect();
    let mut lib = load().await?;
    let (added, updated) = library::save_from_project(&mut lib, &series, &characters, &entries);
    library::save(&library::default_dir(), &lib).map_err(ApiError::internal)?;
    Ok(Json(SaveToLibraryResponse {
        library: lib,
        added,
        updated,
    }))
}

#[utoipa::path(
    post,
    path = "/library/apply",
    request_body = LibraryKeysRequest,
    responses((status = 200, body = ApplyFromLibraryResponse))
)]
async fn apply_from_library(
    State(app): State<AppState>,
    Json(req): Json<LibraryKeysRequest>,
) -> ApiResult<Json<ApplyFromLibraryResponse>> {
    let session = app
        .current_session()
        .ok_or_else(|| ApiError::bad_request("no project open"))?;
    let lib = load().await?;
    let ck: HashSet<Uuid> = req.character_keys.into_iter().collect();
    let tk: HashSet<Uuid> = req.term_keys.into_iter().collect();
    let characters: Vec<_> = lib
        .characters
        .iter()
        .filter(|c| ck.contains(&c.key))
        .map(|c| c.character.clone())
        .collect();
    let terms: Vec<_> = lib
        .terms
        .iter()
        .filter(|t| tk.contains(&t.key))
        .map(|t| t.entry.clone())
        .collect();

    let mut outcome = MergeOutcome::default();
    // Through the session, so adding characters is one undo step.
    let (_, book) = session
        .update_characters(|book| {
            let mut list = book.characters.clone();
            library::merge_characters(&mut list, &characters, &mut outcome);
            book.set_characters(list);
            Ok(())
        })
        .map_err(ApiError::internal)?;
    let mut glossary = session.glossary();
    library::merge_glossary(&mut glossary, &terms, &mut outcome);
    if outcome.glossary_added + outcome.glossary_updated > 0 {
        session.set_glossary(glossary).map_err(ApiError::internal)?;
    }
    Ok(Json(ApplyFromLibraryResponse { outcome, book }))
}

#[utoipa::path(
    post,
    path = "/library/delete",
    request_body = LibraryKeysRequest,
    responses((status = 200, body = Library))
)]
async fn delete_from_library(Json(req): Json<LibraryKeysRequest>) -> ApiResult<Json<Library>> {
    let mut lib = load().await?;
    if library::remove(&mut lib, &req.character_keys, &req.term_keys) > 0 {
        library::save(&library::default_dir(), &lib).map_err(ApiError::internal)?;
    }
    Ok(Json(lib))
}
