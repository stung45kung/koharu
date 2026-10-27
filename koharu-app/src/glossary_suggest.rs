//! Ask the loaded LLM which names / special terms of the project should go
//! into the glossary.
//!
//! The OCR text of every page is sent as one block through the same
//! `translate_texts` channel as translation (like speaker guessing), with a
//! system prompt that turns the "translation" into a term list:
//! `source => rendering ; kind`, one per line. The user reviews the list
//! before anything is added.

use std::collections::HashSet;

use anyhow::{Result, bail};
use koharu_core::{GlossaryEntry, NodeKind, Scene};
use koharu_llm::Language;

use crate::llm;
use crate::session::ProjectSession;

/// Max characters of script sent to the model (fits small local contexts).
const MAX_SCRIPT_CHARS: usize = 8000;
/// Max suggestions returned.
const MAX_SUGGESTIONS: usize = 60;
const MAX_FIELD_CHARS: usize = 200;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SuggestOutcome {
    pub suggestions: Vec<GlossaryEntry>,
    /// Distinct OCR lines that were sent.
    pub lines_scanned: usize,
    /// The project had more text than fits; only the first part was sent.
    pub truncated: bool,
}

/// Distinct OCR lines of the whole project in page order, capped at
/// `MAX_SCRIPT_CHARS`.
fn script_lines(scene: &Scene) -> (Vec<String>, bool) {
    let mut seen = HashSet::new();
    let mut lines = Vec::new();
    let mut total = 0usize;
    for page in scene.pages.values() {
        for node in page.nodes.values() {
            let NodeKind::Text(t) = &node.kind else {
                continue;
            };
            let Some(text) = t.text.as_deref() else {
                continue;
            };
            let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if line.is_empty() || !seen.insert(line.clone()) {
                continue;
            }
            let len = line.chars().count() + 1;
            if total + len > MAX_SCRIPT_CHARS {
                return (lines, true);
            }
            total += len;
            lines.push(line);
        }
    }
    (lines, false)
}

fn system_prompt(target: &str, known: &[String]) -> String {
    let known = if known.is_empty() {
        String::new()
    } else {
        format!(
            "\nAlready in the glossary (do not list again): {}\n",
            known.join(", ")
        )
    };
    format!(
        "You are building a glossary for translating a manga into {target}. This is NOT a \
         translation task. Block [1] is the manga's script (one speech bubble per line).\n\
         List the proper nouns and special terms that must be translated the same way every \
         time: character names and nicknames, titles, places, organisations, spells, \
         techniques, items, invented words. Skip ordinary words and phrases.\n\
         Write each term exactly as it appears in the script, and give the {target} rendering \
         a professional translator would use (transliterate names naturally).\n{known}\n\
         Answer inside block [1] only, one term per line, in this exact format:\n\
         source => {target} rendering ; kind\n\
         where kind is one of: name, place, organisation, technique, item, title, term.\n\
         List at most {MAX_SUGGESTIONS} terms, most important first. No other text."
    )
}

fn clip(s: &str) -> String {
    s.chars().take(MAX_FIELD_CHARS).collect()
}

/// Parse `source => target ; kind` lines. Tolerates bullets, numbering,
/// `->` / `=` arrows and a missing kind; drops terms in `known` (lowercase)
/// and duplicates.
pub fn parse_suggestions(text: &str, known: &HashSet<String>) -> Vec<GlossaryEntry> {
    let mut seen = known.clone();
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw
            .trim()
            .trim_start_matches(['-', '*', '•', '·'])
            .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ')')
            .trim();
        let Some((source, rest)) = ["=>", "->", "→", "="]
            .iter()
            .find_map(|arrow| line.split_once(arrow))
        else {
            continue;
        };
        let (target, note) = match rest.split_once([';', '；']) {
            Some((t, n)) => (t, n),
            None => (rest, ""),
        };
        let unquote = |s: &str| {
            s.trim()
                .trim_matches(['"', '\'', '「', '」', '“', '”', '`'])
                .trim()
                .to_string()
        };
        let source = unquote(source);
        let target = unquote(target);
        let note = unquote(note);
        if source.is_empty() || target.is_empty() || source.chars().count() > MAX_FIELD_CHARS {
            continue;
        }
        if !seen.insert(source.to_lowercase()) {
            continue;
        }
        out.push(GlossaryEntry {
            source,
            target: clip(&target),
            note: clip(&note),
        });
        if out.len() >= MAX_SUGGESTIONS {
            break;
        }
    }
    out
}

/// Suggest glossary entries for the open project.
pub async fn suggest_terms(
    session: &ProjectSession,
    llm: &llm::Model,
    target_language: Option<&str>,
) -> Result<SuggestOutcome> {
    let (lines, truncated) = {
        let scene = session.scene.read();
        script_lines(&scene)
    };
    if lines.is_empty() {
        bail!("no OCR text yet: run OCR first");
    }
    let target = target_language
        .and_then(Language::parse)
        .unwrap_or(Language::English);
    let target_name = target.to_string();
    let glossary = session.glossary();
    let known_list: Vec<String> = glossary
        .entries
        .iter()
        .map(|e| e.source.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let known: HashSet<String> = known_list.iter().map(|s| s.to_lowercase()).collect();
    let prompt = system_prompt(&target_name, &known_list);
    let script = lines.join("\n");
    let answer = llm
        .translate_texts(&[script], target_language, Some(prompt.as_str()))
        .await?;
    let text = answer.into_iter().next().unwrap_or_default();
    Ok(SuggestOutcome {
        suggestions: parse_suggestions(&text, &known),
        lines_scanned: lines.len(),
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lines_in_several_shapes() {
        let text = "ルルイエ => รุรุเอะ ; name\n\
                    - 魔法少女 -> สาวน้อยเวทมนตร์\n\
                    2. 「黒の塔」 → หอคอยดำ ； place\n\
                    just chatter\n\
                    ルルイエ => ซ้ำ ; name\n\
                    => empty source\n";
        let out = parse_suggestions(text, &HashSet::new());
        let pairs: Vec<(&str, &str, &str)> = out
            .iter()
            .map(|e| (e.source.as_str(), e.target.as_str(), e.note.as_str()))
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("ルルイエ", "รุรุเอะ", "name"),
                ("魔法少女", "สาวน้อยเวทมนตร์", ""),
                ("黒の塔", "หอคอยดำ", "place"),
            ]
        );
    }

    #[test]
    fn skips_terms_already_in_the_glossary() {
        let known: HashSet<String> = ["ルルイエ".to_string()].into_iter().collect();
        let out = parse_suggestions("ルルイエ => x ; name\n剣 => ดาบ ; item", &known);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].source, "剣");
    }

    #[test]
    fn prompt_mentions_known_terms_and_format() {
        let p = system_prompt("Thai", &["ルルイエ".into()]);
        assert!(p.contains("source => Thai rendering ; kind"));
        assert!(p.contains("do not list again): ルルイエ"));
    }
}
