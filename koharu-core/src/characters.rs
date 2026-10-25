//! Per-project character profiles and text-block speaker assignments.
//!
//! Stored beside the scene in `.khrproj/characters.toml` (not inside
//! `scene.bin`), so projects stay readable by Koharu builds that don't know
//! about this feature. Assignments reference `NodeId`s; entries whose node no
//! longer exists are simply ignored.

use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::scene::NodeId;

/// Stable id for a character. Renaming a character never changes it.
#[derive(
    Copy,
    Clone,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize,
    JsonSchema,
    ToSchema,
)]
#[serde(transparent)]
pub struct CharacterId(pub Uuid);

impl CharacterId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for CharacterId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for CharacterId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Character {
    pub id: CharacterId,
    pub name: String,
    /// Free-form speech-style notes (pronouns, register, verbal tics, ...).
    #[serde(default)]
    pub description: String,
}

pub const CHARACTER_BOOK_VERSION: u32 = 1;

/// Max characters of a single description sent to the model.
pub const MAX_DESCRIPTION_CHARS: usize = 1500;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CharacterBook {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub characters: Vec<Character>,
    /// Text node → speaking character. Absent = unknown speaker.
    #[serde(default)]
    pub speakers: IndexMap<NodeId, CharacterId>,
}

fn default_version() -> u32 {
    CHARACTER_BOOK_VERSION
}

impl Default for CharacterBook {
    fn default() -> Self {
        Self {
            version: CHARACTER_BOOK_VERSION,
            characters: Vec::new(),
            speakers: IndexMap::new(),
        }
    }
}

impl CharacterBook {
    pub fn is_empty(&self) -> bool {
        self.characters.is_empty() && self.speakers.is_empty()
    }

    pub fn character(&self, id: CharacterId) -> Option<&Character> {
        self.characters.iter().find(|c| c.id == id)
    }

    /// Speaker of `node`, if it is assigned to a character that still exists.
    pub fn speaker_of(&self, node: NodeId) -> Option<&Character> {
        self.speakers.get(&node).and_then(|id| self.character(*id))
    }

    /// Replace the character list. Assignments pointing at removed
    /// characters are dropped (those blocks become "unknown speaker").
    pub fn set_characters(&mut self, characters: Vec<Character>) {
        self.characters = characters;
        let alive: std::collections::HashSet<CharacterId> =
            self.characters.iter().map(|c| c.id).collect();
        self.speakers.retain(|_, id| alive.contains(id));
    }

    /// Assign (or clear, with `None`) the speaker of several nodes at once.
    /// Unknown character ids are rejected.
    pub fn assign(
        &mut self,
        nodes: &[NodeId],
        character: Option<CharacterId>,
    ) -> Result<(), UnknownCharacter> {
        match character {
            Some(id) => {
                if self.character(id).is_none() {
                    return Err(UnknownCharacter(id));
                }
                for node in nodes {
                    self.speakers.insert(*node, id);
                }
            }
            None => {
                for node in nodes {
                    self.speakers.shift_remove(node);
                }
            }
        }
        Ok(())
    }

    /// Prompt section describing who speaks each tagged block, or `None`
    /// when no block in this request has a known speaker (then the prompt
    /// is left exactly as it was before this feature existed).
    ///
    /// `blocks[i]` must be the node sent as tag `[i + 1]`.
    pub fn speaker_prompt_section(&self, blocks: &[NodeId]) -> Option<String> {
        let speakers: Vec<Option<&Character>> =
            blocks.iter().map(|id| self.speaker_of(*id)).collect();
        if speakers.iter().all(Option::is_none) {
            return None;
        }

        // Only profiles actually used in this request, in first-use order.
        let mut used: Vec<&Character> = Vec::new();
        for c in speakers.iter().flatten().copied() {
            if !used.iter().any(|u| u.id == c.id) {
                used.push(c);
            }
        }
        let label = |c: &Character| -> String {
            let pos = used.iter().position(|u| u.id == c.id).unwrap_or(0);
            format!("S{}", pos + 1)
        };

        let mut out = String::new();
        out.push_str(
            "Speaker information for this request. Use a speaker's profile only to choose \
             voice, register, self-reference and address terms, sentence endings, and word \
             choice for that speaker's blocks. Never add events, names, relationships, \
             emotions, or facts from a profile that the source text does not state. If a \
             profile conflicts with the source text, follow the source text. Output only the \
             translations with their original tags; do not output speaker labels.\n",
        );
        out.push_str("\nSpeaker profiles:\n");
        for &c in &used {
            let name = one_line(&c.name);
            let desc = one_line(&truncate_chars(c.description.trim(), MAX_DESCRIPTION_CHARS));
            if desc.is_empty() {
                out.push_str(&format!("- {}: {}\n", label(c), name));
            } else {
                out.push_str(&format!("- {}: {} — {}\n", label(c), name, desc));
            }
        }
        out.push_str("\nBlock speakers:\n");
        for (i, speaker) in speakers.iter().copied().enumerate() {
            match speaker {
                Some(c) => out.push_str(&format!("[{}] {}\n", i + 1, label(c))),
                None => out.push_str(&format!(
                    "[{}] unknown speaker (do not assume a voice beyond the source text)\n",
                    i + 1
                )),
            }
        }
        Some(out.trim_end().to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnknownCharacter(pub CharacterId);

impl std::fmt::Display for UnknownCharacter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown character id {}", self.0)
    }
}

impl std::error::Error for UnknownCharacter {}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_chars(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((idx, _)) => s[..idx].to_string(),
        None => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(n: u128) -> NodeId {
        NodeId(Uuid::from_u128(n))
    }

    fn character(n: u128, name: &str, desc: &str) -> Character {
        Character {
            id: CharacterId(Uuid::from_u128(1000 + n)),
            name: name.to_string(),
            description: desc.to_string(),
        }
    }

    fn book() -> CharacterBook {
        let mut book = CharacterBook::default();
        book.set_characters(vec![
            character(1, "Akazukin", "ใช้ ข้า/เจ้า พูดแบบหญิงสูงศักดิ์โบราณ"),
            character(2, "ผู้บรรยาย", ""),
        ]);
        book
    }

    #[test]
    fn no_section_when_no_block_has_a_speaker() {
        let book = book();
        assert_eq!(book.speaker_prompt_section(&[node(1), node(2)]), None);
    }

    #[test]
    fn section_maps_tags_to_used_speakers_only() {
        let mut book = book();
        let akazukin = book.characters[0].id;
        book.assign(&[node(1), node(3)], Some(akazukin)).unwrap();

        let section = book
            .speaker_prompt_section(&[node(1), node(2), node(3)])
            .unwrap();

        assert!(section.contains("- S1: Akazukin — ใช้ ข้า/เจ้า"));
        assert!(
            !section.contains("ผู้บรรยาย"),
            "unused profile leaked: {section}"
        );
        assert!(section.contains("[1] S1\n"));
        assert!(section.contains("[2] unknown speaker"));
        assert!(section.ends_with("[3] S1"));
    }

    #[test]
    fn labels_follow_request_order_not_book_order() {
        let mut book = book();
        let (akazukin, narrator) = (book.characters[0].id, book.characters[1].id);
        book.assign(&[node(1)], Some(narrator)).unwrap();
        book.assign(&[node(2)], Some(akazukin)).unwrap();

        // Translating only block 2 must label it S1 and omit the narrator.
        let section = book.speaker_prompt_section(&[node(2)]).unwrap();
        assert!(section.contains("- S1: Akazukin"));
        assert!(section.contains("[1] S1"));
        assert!(!section.contains("ผู้บรรยาย"));
    }

    #[test]
    fn deleting_character_clears_its_assignments() {
        let mut book = book();
        let akazukin = book.characters[0].id;
        book.assign(&[node(1)], Some(akazukin)).unwrap();
        let keep = vec![book.characters[1].clone()];
        book.set_characters(keep);
        assert!(book.speakers.is_empty());
        assert_eq!(book.speaker_prompt_section(&[node(1)]), None);
    }

    #[test]
    fn assign_rejects_unknown_character_and_none_clears() {
        let mut book = book();
        let bogus = CharacterId(Uuid::from_u128(9));
        assert_eq!(
            book.assign(&[node(1)], Some(bogus)),
            Err(UnknownCharacter(bogus))
        );

        let akazukin = book.characters[0].id;
        book.assign(&[node(1), node(2)], Some(akazukin)).unwrap();
        book.assign(&[node(1)], None).unwrap();
        assert!(book.speaker_of(node(1)).is_none());
        assert!(book.speaker_of(node(2)).is_some());
    }

    #[test]
    fn multiline_names_and_long_descriptions_are_flattened() {
        let mut book = CharacterBook::default();
        let long = "ก".repeat(MAX_DESCRIPTION_CHARS + 50);
        book.set_characters(vec![character(1, "A\nB", &format!("line1\nline2 {long}"))]);
        let id = book.characters[0].id;
        book.assign(&[node(1)], Some(id)).unwrap();
        let section = book.speaker_prompt_section(&[node(1)]).unwrap();
        let profile_line = section.lines().find(|l| l.starts_with("- S1")).unwrap();
        assert!(profile_line.starts_with("- S1: A B — line1 line2 "));
        assert!(profile_line.chars().count() < MAX_DESCRIPTION_CHARS + 30);
    }
}
