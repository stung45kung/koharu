//! Draft a character's speech cues from the lines the user confirmed for it.
//!
//! The confirmed lines go to the loaded LLM as one block through the same
//! `translate_texts` channel as translation, with a system prompt that turns
//! the "translation" into a short description of how the character talks.
//! The result is only a suggestion: the user edits it and saves.

use std::collections::HashSet;

use anyhow::{Result, bail};
use koharu_core::{CharacterId, NodeKind};
use koharu_llm::Language;

use crate::llm;
use crate::session::ProjectSession;

/// Most lines sent (the most recent pages' lines are not preferred: the
/// whole project is read in order).
const MAX_LINES: usize = 40;
const MAX_LINE_CHARS: usize = 120;
/// Fewest confirmed lines needed for a useful summary.
pub const MIN_LINES: usize = 2;
const MAX_CUES_CHARS: usize = 600;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CuesOutcome {
    pub speech_cues: String,
    pub lines_used: usize,
}

/// Confirmed (not guessed) lines of `character`, in project order, deduped.
fn confirmed_lines(session: &ProjectSession, character: CharacterId) -> Vec<String> {
    let book = session.characters();
    let scene = session.scene.read();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for page in scene.pages.values() {
        for (id, node) in &page.nodes {
            let NodeKind::Text(t) = &node.kind else {
                continue;
            };
            if book.speakers.get(id) != Some(&character) || book.is_suggested(*id) {
                continue;
            }
            let Some(text) = t.text.as_deref() else {
                continue;
            };
            let line: String = text
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(MAX_LINE_CHARS)
                .collect();
            if !line.is_empty() && seen.insert(line.clone()) {
                out.push(line);
                if out.len() >= MAX_LINES {
                    return out;
                }
            }
        }
    }
    out
}

fn system_prompt(name: &str, language: &str) -> String {
    format!(
        "This is NOT a translation task. Block [1] lists lines that the manga character \
         \"{name}\" says in the original Japanese, one per line.\n\
         Describe briefly how this character talks, so that someone reading a new line can \
         tell whether {name} said it: how they refer to themselves, how they address others, \
         typical sentence endings, politeness level, verbal tics and tone. Quote the Japanese \
         forms exactly as they appear in the lines (e.g. 私, あんた, 〜のよ).\n\
         Write the description in {language}, as at most 3 short lines, without examples of \
         whole sentences and without any introduction.\n\
         Answer inside block [1] only."
    )
}

/// Ask the LLM for speech cues of `character` from its confirmed lines.
pub async fn suggest(
    session: &ProjectSession,
    llm: &llm::Model,
    character: CharacterId,
    target_language: Option<&str>,
) -> Result<CuesOutcome> {
    let Some(name) = session
        .characters()
        .character(character)
        .map(|c| c.name.clone())
    else {
        bail!("unknown character");
    };
    let lines = confirmed_lines(session, character);
    if lines.len() < MIN_LINES {
        bail!(
            "confirm at least {MIN_LINES} blocks spoken by {name} first (found {})",
            lines.len()
        );
    }
    let language = target_language
        .and_then(Language::parse)
        .unwrap_or(Language::English)
        .to_string();
    let prompt = system_prompt(&name, &language);
    let answer = llm
        .translate_texts(&[lines.join("\n")], target_language, Some(prompt.as_str()))
        .await?;
    let cues: String = answer
        .into_iter()
        .next()
        .unwrap_or_default()
        .trim()
        .chars()
        .take(MAX_CUES_CHARS)
        .collect();
    if cues.is_empty() {
        bail!("the model returned no description");
    }
    Ok(CuesOutcome {
        speech_cues: cues,
        lines_used: lines.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_names_the_character_and_language() {
        let p = system_prompt("Ruruie", "Thai");
        assert!(p.contains("\"Ruruie\""));
        assert!(p.contains("in Thai"));
    }
}
