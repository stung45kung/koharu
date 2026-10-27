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
use std::collections::HashMap;

use koharu_core::{CharacterBook, CharacterId, NodeId, NodeKind, Page, PageId, Scene, Transform};

use crate::llm;
use crate::session::ProjectSession;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GuessOutcome {
    /// Blocks with OCR text that were open for guessing (unknown or guessed).
    pub considered: usize,
    /// Blocks that received a (new or changed) suggested speaker.
    pub assigned: usize,
}

/// A text block with OCR text, plus a short description of where it sits.
struct Block {
    id: NodeId,
    text: String,
    position: String,
}

/// Text blocks on `page` with non-empty OCR text, in page order (the same
/// order translation uses for its numbered tags).
fn text_blocks(page: &Page) -> Vec<Block> {
    let (w, h) = (page.width.max(1) as f32, page.height.max(1) as f32);
    page.nodes
        .iter()
        .filter_map(|(id, node)| match &node.kind {
            NodeKind::Text(t) => t
                .text
                .as_ref()
                .filter(|s| !s.trim().is_empty())
                .map(|s| Block {
                    id: *id,
                    text: s.clone(),
                    position: describe_position(&node.transform, w, h),
                }),
            _ => None,
        })
        .collect()
}

/// e.g. "top right (x 82%, y 10%)": a coarse 3x3 region plus the centre in
/// percent, enough for the model to tell which lines sit together.
fn describe_position(t: &Transform, page_w: f32, page_h: f32) -> String {
    let cx = ((t.x + t.width / 2.0) / page_w).clamp(0.0, 1.0);
    let cy = ((t.y + t.height / 2.0) / page_h).clamp(0.0, 1.0);
    let row = match cy {
        y if y < 1.0 / 3.0 => "top",
        y if y < 2.0 / 3.0 => "middle",
        _ => "bottom",
    };
    let col = match cx {
        x if x < 1.0 / 3.0 => "left",
        x if x < 2.0 / 3.0 => "center",
        _ => "right",
    };
    format!("{row} {col} (x {:.0}%, y {:.0}%)", cx * 100.0, cy * 100.0)
}

/// Confirmed example lines per character sent to speaker guessing.
const EXAMPLES_PER_CHARACTER: usize = 5;

/// Lines the user confirmed for each character (set by hand or accepted),
/// taken from the pages nearest to `page` (the page itself excluded: its
/// confirmed blocks are already listed as known). These show the model how
/// each character talks in this particular work.
fn confirmed_examples(
    scene: &Scene,
    page: PageId,
    book: &CharacterBook,
) -> HashMap<CharacterId, Vec<String>> {
    let mut out: HashMap<CharacterId, Vec<String>> = HashMap::new();
    let Some(current) = scene.pages.get_index_of(&page) else {
        return out;
    };
    let mut order: Vec<usize> = (0..scene.pages.len()).filter(|i| *i != current).collect();
    // Nearest pages first; on ties the earlier page.
    order.sort_by_key(|i| (i.abs_diff(current), *i));
    for i in order {
        let Some((_, p)) = scene.pages.get_index(i) else {
            continue;
        };
        for block in text_blocks(p) {
            if book.is_suggested(block.id) {
                continue;
            }
            let Some(id) = book.speakers.get(&block.id) else {
                continue;
            };
            if book.character(*id).is_none() {
                continue;
            }
            let lines = out.entry(*id).or_default();
            let text = block.text.split_whitespace().collect::<Vec<_>>().join(" ");
            if lines.len() < EXAMPLES_PER_CHARACTER && !lines.contains(&text) {
                lines.push(text);
            }
        }
        if book.characters.iter().all(|c| {
            out.get(&c.id)
                .is_some_and(|l| l.len() >= EXAMPLES_PER_CHARACTER)
        }) {
            break;
        }
    }
    out
}

/// Number of trailing lines of the previous page sent as context.
const PREVIOUS_PAGE_LINES: usize = 8;

/// `(speaker name or "?", line)` for the last lines of the page before
/// `page`, using whatever speakers are known there (confirmed or guessed).
fn previous_page_context(
    scene: &Scene,
    page: PageId,
    book: &CharacterBook,
) -> Vec<(String, String)> {
    let Some(index) = scene.pages.get_index_of(&page) else {
        return Vec::new();
    };
    let Some((_, prev)) = index.checked_sub(1).and_then(|i| scene.pages.get_index(i)) else {
        return Vec::new();
    };
    let blocks = text_blocks(prev);
    let skip = blocks.len().saturating_sub(PREVIOUS_PAGE_LINES);
    blocks
        .into_iter()
        .skip(skip)
        .map(|b| {
            let speaker = book
                .speaker_of(b.id)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| if book.is_other(b.id) { "X" } else { "?" }.to_string());
            (speaker, b.text)
        })
        .collect()
}

pub async fn guess_page_speakers(
    session: &ProjectSession,
    llm: &llm::Model,
    page: PageId,
    target_language: Option<&str>,
) -> Result<GuessOutcome> {
    let book = session.characters();
    if book.characters.is_empty() {
        bail!("add characters before guessing speakers");
    }
    let (blocks, previous, examples) = {
        let scene = session.scene.read();
        let Some(current) = scene.page(page) else {
            bail!("page not found: {page}");
        };
        (
            text_blocks(current),
            previous_page_context(&scene, page, &book),
            confirmed_examples(&scene, page, &book),
        )
    };
    let considered = blocks
        .iter()
        .filter(|b| book.is_open_for_guess(b.id))
        .count();
    if considered == 0 {
        return Ok(GuessOutcome::default());
    }

    let ids: Vec<NodeId> = blocks.iter().map(|b| b.id).collect();
    let layout: Vec<String> = blocks.iter().map(|b| b.position.clone()).collect();
    let texts: Vec<String> = blocks.into_iter().map(|b| b.text).collect();
    let Some(prompt) = book.speaker_guess_prompt(&ids, &layout, &previous, &examples) else {
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
fn bare(label: &str) -> &str {
    label
        .trim()
        .trim_matches(|c: char| c == '*' || c == '`' || c == '.' || c == '"')
}

/// `X`: a character not in the list.
fn is_other_label(label: &str) -> bool {
    bare(label).eq_ignore_ascii_case("x")
}

/// `?` (unsure) or `X` (not in the list), possibly decorated.
fn is_no_listed_speaker(label: &str) -> bool {
    bare(label) == "?" || is_other_label(label)
}

fn apply_guesses(book: &mut CharacterBook, ids: &[NodeId], labels: &[String]) -> usize {
    let mut changed = 0;
    for (id, label) in ids.iter().zip(labels) {
        if !book.is_open_for_guess(*id) {
            continue;
        }
        // "X": someone not in the list → the reserved "other" speaker.
        let character = if is_other_label(label) {
            Some(CharacterId::OTHER)
        } else {
            book.parse_speaker_label(label)
        };
        // An unusable answer leaves an earlier guess in place rather than
        // wiping it; only an explicit "?" (unsure) clears a guess.
        if character.is_none() && !is_no_listed_speaker(label) {
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

    #[test]
    fn x_and_question_mark_mean_no_listed_speaker() {
        assert!(is_no_listed_speaker("X"));
        assert!(is_no_listed_speaker(" x. "));
        assert!(is_no_listed_speaker("?"));
        assert!(!is_no_listed_speaker("S1"));
        assert!(!is_no_listed_speaker("garbage"));
    }

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
                ..Default::default()
            },
            Character {
                id: CharacterId(Uuid::from_u128(102)),
                name: "Narrator".into(),
                description: String::new(),
                ..Default::default()
            },
        ]);
        book
    }

    #[test]
    fn describes_block_position_in_page_thirds() {
        let t = Transform {
            x: 800.0,
            y: 50.0,
            width: 100.0,
            height: 100.0,
            rotation_deg: 0.0,
        };
        assert_eq!(
            describe_position(&t, 1000.0, 1000.0),
            "top right (x 85%, y 10%)"
        );
        let t = Transform {
            x: 0.0,
            y: 900.0,
            width: 100.0,
            height: 100.0,
            rotation_deg: 0.0,
        };
        assert_eq!(
            describe_position(&t, 1000.0, 1000.0),
            "bottom left (x 5%, y 95%)"
        );
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
