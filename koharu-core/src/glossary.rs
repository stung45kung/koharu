//! Per-project glossary: fixed translations for names and terms.
//!
//! Stored in `.khrproj/glossary.toml` (outside the scene format). When a
//! page is translated, only entries whose source term occurs in that page's
//! text are sent to the model, so a long glossary doesn't bloat every prompt.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GlossaryEntry {
    /// Term as it appears in the source text (e.g. `すい`).
    pub source: String,
    /// Required translation (e.g. `ซุย`).
    pub target: String,
    /// Optional hint for the model (e.g. "heroine's nickname").
    #[serde(default)]
    pub note: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Glossary {
    #[serde(default)]
    pub entries: Vec<GlossaryEntry>,
}

/// Max entries sent for one request.
pub const MAX_PROMPT_ENTRIES: usize = 60;

impl Glossary {
    /// Entries whose source term occurs in any of `texts`, longest terms
    /// first (so `すいちゃん` is listed before `すい`).
    pub fn matching<'a>(&'a self, texts: &[String]) -> Vec<&'a GlossaryEntry> {
        let mut hits: Vec<&GlossaryEntry> = self
            .entries
            .iter()
            .filter(|e| {
                let term = e.source.trim();
                !term.is_empty()
                    && !e.target.trim().is_empty()
                    && texts.iter().any(|t| t.contains(term))
            })
            .collect();
        hits.sort_by_key(|e| std::cmp::Reverse(e.source.trim().chars().count()));
        hits.truncate(MAX_PROMPT_ENTRIES);
        hits
    }

    /// Prompt section for the glossary terms used in `texts`, or `None`.
    pub fn prompt_section(&self, texts: &[String]) -> Option<String> {
        let hits = self.matching(texts);
        if hits.is_empty() {
            return None;
        }
        let mut out = String::from(
            "Glossary: always translate these terms exactly as given, including inside \
             longer words and with honorifics (keep the rest of the sentence natural):\n",
        );
        for e in hits {
            let line = format!("- {} => {}", flat(&e.source), flat(&e.target));
            let note = flat(&e.note);
            if note.is_empty() {
                out.push_str(&line);
            } else {
                out.push_str(&format!("{line} ({note})"));
            }
            out.push('\n');
        }
        Some(out.trim_end().to_string())
    }
}

fn flat(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(source: &str, target: &str, note: &str) -> GlossaryEntry {
        GlossaryEntry {
            source: source.into(),
            target: target.into(),
            note: note.into(),
        }
    }

    #[test]
    fn only_terms_present_on_the_page_are_sent() {
        let g = Glossary {
            entries: vec![
                entry("すい", "ซุย", ""),
                entry("旅館", "เรียวกัง", "Japanese inn"),
                entry("動物園", "สวนสัตว์", ""),
            ],
        };
        let texts = vec!["すいちゃん…".to_string(), "旅館行く前に".to_string()];
        let section = g.prompt_section(&texts).unwrap();
        assert!(section.contains("- すい => ซุย"));
        assert!(section.contains("- 旅館 => เรียวกัง (Japanese inn)"));
        assert!(!section.contains("動物園"));
    }

    #[test]
    fn longer_terms_come_first_and_blank_entries_are_ignored() {
        let g = Glossary {
            entries: vec![
                entry("すい", "ซุย", ""),
                entry("すいちゃん", "ซุยจัง", ""),
                entry("", "x", ""),
                entry("海", " ", ""),
            ],
        };
        let hits = g.matching(&["すいちゃん、海!".to_string()]);
        let sources: Vec<&str> = hits.iter().map(|e| e.source.as_str()).collect();
        assert_eq!(sources, vec!["すいちゃん", "すい"]);
        assert_eq!(Glossary::default().prompt_section(&["すい".into()]), None);
    }
}
