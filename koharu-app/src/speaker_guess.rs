//! Automatic speaker guessing for one page.
//!
//! Sends the page's OCR text (in block order) plus the character profiles to
//! the loaded LLM and stores the answers as *suggested* speakers. Blocks whose
//! speaker the user set themselves are never changed; the user reviews the
//! suggestions and confirms or corrects them.
//!
//! Uses the same `translate_texts` channel as translation (numbered tags in,
//! numbered tags out), so it works with every LLM backend without a separate
//! provider API. Machine-translation providers (DeepL, Google, Caiyun) ignore
//! the instructions and simply yield no usable labels.

use anyhow::{Result, bail};
use koharu_core::{NodeId, NodeKind, PageId};

use crate::llm;
use crate::session::ProjectSession;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GuessOutcome {
    /// Blocks with OCR text that were open for guessing (unknown or guessed).
    pub considered: usize,
    /// Blocks that received a (new or changed) suggested speaker.
    pub assigned: usize,
}

/// Text blocks on `page` with non-empty OCR text, in page order (the same
/// order translation uses for its numbered tags).
fn text_blocks(session: &ProjectSession, page: PageId) -> Result<Vec<(NodeId, String)>> {
    let scene = session.scene.read();
    let Some(page) = scene.page(page) else {
        bail!("page not found: {page}");
    };
    Ok(page
        .nodes
        .iter()
        .filter_map(|(id, node)| match &node.kind {
            NodeKind::Text(t) => t
                .text
                .as_ref()
                .filter(|s| !s.trim().is_empty())
                .map(|s| (*id, s.clone())),
            _ => None,
        })
        .collect())
}

pub async fn guess_page_speakers(
    session: &ProjectSession,
    llm: &llm::Model,
    page: PageId,
    target_language: Option<&str>,
) -> Result<GuessOutcome> {
    let blocks = text_blocks(session, page)?;
    let book = session.characters();
    if book.characters.is_empty() {
        bail!("add characters before guessing speakers");
    }
    let considered = blocks
        .iter()
        .filter(|(id, _)| book.is_open_for_guess(*id))
        .count();
    if considered == 0 {
        return Ok(GuessOutcome::default());
    }

    let ids: Vec<NodeId> = blocks.iter().map(|(id, _)| *id).collect();
    let texts: Vec<String> = blocks.into_iter().map(|(_, text)| text).collect();
    let Some(prompt) = book.speaker_guess_prompt(&ids) else {
        bail!("add characters before guessing speakers");
    };
    let labels = llm
        .translate_texts(&texts, target_language, Some(prompt.as_str()))
        .await?;

    let (assigned, _) = session.update_characters(|book| Ok(apply_guesses(book, &ids, &labels)))?;
    Ok(GuessOutcome {
        considered,
        assigned,
    })
}

/// Store `labels[i]` as the suggested speaker of `ids[i]` where allowed.
/// Returns how many blocks changed.
fn apply_guesses(
    book: &mut koharu_core::CharacterBook,
    ids: &[NodeId],
    labels: &[String],
) -> usize {
    let mut changed = 0;
    for (id, label) in ids.iter().zip(labels) {
        if !book.is_open_for_guess(*id) {
            continue;
        }
        let character = book.parse_speaker_label(label);
        // An unusable answer leaves an earlier guess in place rather than
        // wiping it; only an explicit "?" clears a guess.
        if character.is_none() && label.trim() != "?" {
            continue;
        }
        if book.suggest(*id, character) && character.is_some() {
            changed += 1;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use koharu_core::{Character, CharacterBook, CharacterId};
    use uuid::Uuid;

    use super::*;

    fn node(n: u128) -> NodeId {
        NodeId(Uuid::from_u128(n))
    }

    fn book() -> CharacterBook {
        let mut book = CharacterBook::default();
        book.set_characters(vec![
            Character {
                id: CharacterId(Uuid::from_u128(101)),
                name: "Captain".into(),
                description: "gruff, short sentences".into(),
            },
            Character {
                id: CharacterId(Uuid::from_u128(102)),
                name: "Narrator".into(),
                description: String::new(),
            },
        ]);
        book
    }

    #[test]
    fn applies_labels_as_suggestions_only_where_open() {
        let mut book = book();
        let (captain, narrator) = (book.characters[0].id, book.characters[1].id);
        book.assign(&[node(2)], Some(narrator)).unwrap();

        let labels = vec!["S1".to_string(), "S1".to_string(), "S2".to_string()];
        let changed = apply_guesses(&mut book, &[node(1), node(2), node(3)], &labels);

        assert_eq!(changed, 2);
        assert_eq!(book.speaker_of(node(1)).map(|c| c.id), Some(captain));
        assert!(book.is_suggested(node(1)));
        // User-confirmed block untouched.
        assert_eq!(book.speaker_of(node(2)).map(|c| c.id), Some(narrator));
        assert!(!book.is_suggested(node(2)));
        assert_eq!(book.speaker_of(node(3)).map(|c| c.id), Some(narrator));
    }

    #[test]
    fn garbage_keeps_previous_guess_but_question_mark_clears_it() {
        let mut book = book();
        let captain = book.characters[0].id;
        book.suggest(node(1), Some(captain));
        book.suggest(node(2), Some(captain));

        let labels = vec!["(translated text)".to_string(), "?".to_string()];
        apply_guesses(&mut book, &[node(1), node(2)], &labels);

        assert_eq!(book.speaker_of(node(1)).map(|c| c.id), Some(captain));
        assert!(book.speaker_of(node(2)).is_none());
    }
}
