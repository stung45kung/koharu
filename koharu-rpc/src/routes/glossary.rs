//! Project glossary (`.khrproj/glossary.toml`).
//!
//! - `GET /glossary` — current glossary
//! - `PUT /glossary` — replace it
//! - `POST /glossary/suggest` — ask the LLM for names / terms to add

use axum::Json;
use axum::extract::State;
use koharu_app::glossary_suggest;
use koharu_core::{Glossary, GlossaryEntry};
use serde::{Deserialize, Serialize};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::AppState;
use crate::error::{ApiError, ApiResult};

const MAX_ENTRIES: usize = 2000;
const MAX_FIELD_CHARS: usize = 200;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::default()
        .routes(routes!(get_glossary, put_glossary))
        .routes(routes!(suggest_terms))
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SuggestTermsRequest {
    #[serde(default)]
    pub target_language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SuggestTermsResponse {
    /// Proposed entries (not saved — the user picks which to add).
    pub suggestions: Vec<GlossaryEntry>,
    /// Distinct OCR lines that were sent to the model.
    pub lines_scanned: usize,
    /// Only the first part of a very long project was sent.
    pub truncated: bool,
}

#[utoipa::path(
    post,
    path = "/glossary/suggest",
    request_body = SuggestTermsRequest,
    responses((status = 200, body = SuggestTermsResponse))
)]
async fn suggest_terms(
    State(app): State<AppState>,
    Json(req): Json<SuggestTermsRequest>,
) -> ApiResult<Json<SuggestTermsResponse>> {
    let session = app
        .current_session()
        .ok_or_else(|| ApiError::bad_request("no project open"))?;
    if !app.llm.ready().await {
        return Err(ApiError::bad_request("no LLM loaded"));
    }
    let outcome =
        glossary_suggest::suggest_terms(&session, &app.llm, req.target_language.as_deref())
            .await
            .map_err(|e| ApiError::bad_request(format!("{e:#}")))?;
    Ok(Json(SuggestTermsResponse {
        suggestions: outcome.suggestions,
        lines_scanned: outcome.lines_scanned,
        truncated: outcome.truncated,
    }))
}

#[utoipa::path(get, path = "/glossary", responses((status = 200, body = Glossary)))]
async fn get_glossary(State(app): State<AppState>) -> ApiResult<Json<Glossary>> {
    let session = app
        .current_session()
        .ok_or_else(|| ApiError::bad_request("no project open"))?;
    Ok(Json(session.glossary()))
}

#[utoipa::path(
    put,
    path = "/glossary",
    request_body = Glossary,
    responses((status = 200, body = Glossary))
)]
async fn put_glossary(
    State(app): State<AppState>,
    Json(req): Json<Glossary>,
) -> ApiResult<Json<Glossary>> {
    let session = app
        .current_session()
        .ok_or_else(|| ApiError::bad_request("no project open"))?;
    let glossary = clean(req).map_err(ApiError::bad_request)?;
    session
        .set_glossary(glossary.clone())
        .map_err(ApiError::internal)?;
    Ok(Json(glossary))
}

/// Trim fields, drop rows with no source term, enforce size limits.
fn clean(req: Glossary) -> Result<Glossary, String> {
    if req.entries.len() > MAX_ENTRIES {
        return Err(format!("glossary has more than {MAX_ENTRIES} entries"));
    }
    let mut entries = Vec::with_capacity(req.entries.len());
    for e in req.entries {
        let entry = GlossaryEntry {
            source: e.source.trim().to_string(),
            target: e.target.trim().to_string(),
            note: e.note.trim().to_string(),
        };
        if entry.source.is_empty() {
            continue;
        }
        if [&entry.source, &entry.target, &entry.note]
            .iter()
            .any(|f| f.chars().count() > MAX_FIELD_CHARS)
        {
            return Err(format!(
                "glossary fields are limited to {MAX_FIELD_CHARS} characters"
            ));
        }
        entries.push(entry);
    }
    Ok(Glossary { entries })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_and_drops_rows_without_source() {
        let out = clean(Glossary {
            entries: vec![
                GlossaryEntry {
                    source: " すい ".into(),
                    target: " ซุย ".into(),
                    note: String::new(),
                },
                GlossaryEntry {
                    source: "  ".into(),
                    target: "x".into(),
                    note: String::new(),
                },
            ],
        })
        .unwrap();
        assert_eq!(out.entries.len(), 1);
        assert_eq!(out.entries[0].source, "すい");
        assert_eq!(out.entries[0].target, "ซุย");
    }
}
