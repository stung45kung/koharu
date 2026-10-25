//! Character profiles + text-block speakers for the open project.
//!
//! - `GET /characters`          — the whole book
//! - `PUT /characters`          — replace the character list
//! - `PUT /characters/speakers` — set/clear the speaker of some text nodes
//!
//! Every mutation returns the updated book. Stored in
//! `.khrproj/characters.toml`, outside the undo history.

use std::collections::HashSet;

use axum::Json;
use axum::extract::State;
use koharu_core::{Character, CharacterBook, CharacterId, NodeId};
use serde::{Deserialize, Serialize};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::AppState;
use crate::error::{ApiError, ApiResult};

const MAX_NAME_CHARS: usize = 80;
const MAX_DESCRIPTION_CHARS: usize = 4000;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::default()
        .routes(routes!(get_characters, put_characters))
        .routes(routes!(put_speakers))
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PutCharactersRequest {
    pub characters: Vec<Character>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PutSpeakersRequest {
    pub node_ids: Vec<NodeId>,
    /// `null` → unknown speaker.
    #[serde(default)]
    pub character_id: Option<CharacterId>,
}

#[utoipa::path(get, path = "/characters", responses((status = 200, body = CharacterBook)))]
async fn get_characters(State(app): State<AppState>) -> ApiResult<Json<CharacterBook>> {
    let session = app
        .current_session()
        .ok_or_else(|| ApiError::bad_request("no project open"))?;
    Ok(Json(session.characters()))
}

#[utoipa::path(
    put,
    path = "/characters",
    request_body = PutCharactersRequest,
    responses((status = 200, body = CharacterBook))
)]
async fn put_characters(
    State(app): State<AppState>,
    Json(req): Json<PutCharactersRequest>,
) -> ApiResult<Json<CharacterBook>> {
    let session = app
        .current_session()
        .ok_or_else(|| ApiError::bad_request("no project open"))?;
    let characters = validate_characters(req.characters).map_err(ApiError::bad_request)?;
    let (_, book) = session
        .update_characters(|book| {
            book.set_characters(characters);
            Ok(())
        })
        .map_err(ApiError::internal)?;
    Ok(Json(book))
}

#[utoipa::path(
    put,
    path = "/characters/speakers",
    request_body = PutSpeakersRequest,
    responses((status = 200, body = CharacterBook))
)]
async fn put_speakers(
    State(app): State<AppState>,
    Json(req): Json<PutSpeakersRequest>,
) -> ApiResult<Json<CharacterBook>> {
    let session = app
        .current_session()
        .ok_or_else(|| ApiError::bad_request("no project open"))?;
    if let Some(id) = req.character_id
        && session.characters().character(id).is_none()
    {
        return Err(ApiError::bad_request(format!("unknown character id {id}")));
    }
    let (_, book) = session
        .update_characters(|book| {
            book.assign(&req.node_ids, req.character_id)?;
            Ok(())
        })
        .map_err(ApiError::internal)?;
    Ok(Json(book))
}

fn validate_characters(characters: Vec<Character>) -> Result<Vec<Character>, String> {
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(characters.len());
    for mut c in characters {
        if !seen.insert(c.id) {
            return Err(format!("duplicate character id {}", c.id));
        }
        c.name = c.name.trim().to_string();
        if c.name.is_empty() {
            return Err("character name must not be empty".to_string());
        }
        if c.name.chars().count() > MAX_NAME_CHARS {
            return Err(format!(
                "character name longer than {MAX_NAME_CHARS} characters"
            ));
        }
        c.description = c.description.trim().to_string();
        if c.description.chars().count() > MAX_DESCRIPTION_CHARS {
            return Err(format!(
                "description of {} longer than {MAX_DESCRIPTION_CHARS} characters",
                c.name
            ));
        }
        out.push(c);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn c(n: u128, name: &str) -> Character {
        Character {
            id: CharacterId(Uuid::from_u128(n)),
            name: name.to_string(),
            description: "  speaks politely  ".to_string(),
        }
    }

    #[test]
    fn trims_and_accepts_valid_list() {
        let out = validate_characters(vec![c(1, "  ฮารุ "), c(2, "ผู้บรรยาย")]).unwrap();
        assert_eq!(out[0].name, "ฮารุ");
        assert_eq!(out[0].description, "speaks politely");
    }

    #[test]
    fn rejects_blank_names_and_duplicate_ids() {
        assert!(validate_characters(vec![c(1, "   ")]).is_err());
        assert!(validate_characters(vec![c(1, "a"), c(1, "b")]).is_err());
    }
}
