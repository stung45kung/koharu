//! Checking OCR text with a vision LLM.
//!
//! The `ocr-check` pipeline step sends each block's crop together with the
//! text OCR read from it, and the model answers with the text corrected
//! against the image. This module holds the parts that need no model: the
//! instructions, the guard that throws away answers that rewrite rather
//! than correct, and the op that stores a fix. The engine keeps the OCR
//! text it replaced in the project's `OcrFixes` so the user can see and
//! restore it.

use koharu_core::glossary::MAX_PROMPT_ENTRIES;
use koharu_core::{
    CharacterBook, Glossary, NodeDataPatch, NodeId, NodePatch, Op, PageId, TextDataPatch,
};

/// Names and terms that may appear in the lettering: every character name,
/// then the glossary's source terms; trimmed, without duplicates, at most
/// [`MAX_PROMPT_ENTRIES`]. All of them are sent (not only those matching
/// the OCR text) because a misread name would not match.
pub fn hint_names(book: &CharacterBook, glossary: &Glossary) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let names = book.characters.iter().map(|c| c.name.as_str());
    let terms = glossary.entries.iter().map(|e| e.source.as_str());
    for name in names.chain(terms) {
        let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
        if name.is_empty() || out.contains(&name) {
            continue;
        }
        out.push(name);
        if out.len() == MAX_PROMPT_ENTRIES {
            break;
        }
    }
    out
}

/// Instructions sent with one block's crop.
pub fn check_instructions(ocr_text: &str, hints: &[String]) -> String {
    let mut out = format!(
        "This image is cut from a manga page and shows lettering (vertical Japanese is read \
         top to bottom, columns right to left). An OCR program read it as:\n{}\n\n\
         Compare this OCR result with the image and fix only characters that were misread, \
         missed or added. Do not translate, rephrase, complete or add words, and do not \
         correct the author's spelling or grammar. If the OCR result already matches the \
         image, output it unchanged. Output only the text, with no quotes, notes or \
         explanation.",
        ocr_text.trim()
    );
    if !hints.is_empty() {
        out.push_str("\n\nNames and terms that may appear: ");
        out.push_str(&hints.join(", "));
    }
    out
}

/// The corrected text, or `None` when the answer looks like a rewrite (or
/// is empty) and the OCR text should be kept. `reply` is already cleaned.
pub fn accept_correction(original: &str, reply: &str) -> Option<String> {
    let reply = reply.trim();
    if reply.is_empty() {
        return None;
    }
    let original_len = original.chars().count();
    let reply_len = reply.chars().count();
    if reply_len > original_len * 2 + 4 {
        return None;
    }
    let longer = original_len.max(reply_len);
    if levenshtein(original, reply) * 2 > longer {
        return None;
    }
    Some(reply.to_string())
}

/// Edit distance in `char`s.
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let substitute = prev[j] + usize::from(ca != cb);
            cur[j + 1] = substitute.min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// The op storing `corrected` as the block's text, or `None` when it equals
/// the `current` text.
pub fn correction_op(page: PageId, id: NodeId, current: &str, corrected: &str) -> Option<Op> {
    if corrected == current {
        return None;
    }
    Some(Op::UpdateNode {
        page,
        id,
        patch: NodePatch {
            data: Some(NodeDataPatch::Text(TextDataPatch {
                text: Some(Some(corrected.to_string())),
                ..Default::default()
            })),
            transform: None,
            visible: None,
        },
        prev: NodePatch::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use koharu_core::{Character, CharacterId, GlossaryEntry};

    fn text_patch(op: Op) -> TextDataPatch {
        let Op::UpdateNode { patch, .. } = op else {
            panic!("expected UpdateNode");
        };
        let Some(NodeDataPatch::Text(t)) = patch.data else {
            panic!("expected a text patch");
        };
        t
    }

    #[test]
    fn accepts_small_fix() {
        assert_eq!(
            accept_correction("あリがとう", "ありがとう").as_deref(),
            Some("ありがとう")
        );
    }

    #[test]
    fn rejects_empty() {
        assert_eq!(accept_correction("ありがとう", ""), None);
        assert_eq!(accept_correction("ありがとう", "  "), None);
    }

    #[test]
    fn rejects_rewrite() {
        assert_eq!(accept_correction("ありがとう", "さようなら"), None);
    }

    #[test]
    fn rejects_explanation() {
        assert_eq!(
            accept_correction("はい", "The text in the image reads: はい"),
            None
        );
    }

    #[test]
    fn short_blocks() {
        assert_eq!(accept_correction("あ", "い"), None);
        assert_eq!(accept_correction("…", "……").as_deref(), Some("……"));
    }

    #[test]
    fn correction_op_none_when_same() {
        assert!(correction_op(PageId::new(), NodeId::new(), "はい", "はい").is_none());
    }

    #[test]
    fn correction_op_sets_the_text() {
        let op = correction_op(PageId::new(), NodeId::new(), "あリがとう", "ありがとう").unwrap();
        assert_eq!(text_patch(op).text, Some(Some("ありがとう".to_string())));
    }

    #[test]
    fn hint_names_caps_and_dedups() {
        let book = CharacterBook {
            characters: ["ルルイエ", "コハル"]
                .into_iter()
                .map(|name| Character {
                    id: CharacterId::new(),
                    name: name.to_string(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        let mut entries = vec![GlossaryEntry {
            source: "コハル".to_string(),
            ..Default::default()
        }];
        entries.extend((0..70).map(|i| GlossaryEntry {
            source: format!("term{i}"),
            ..Default::default()
        }));
        let glossary = Glossary { entries };
        let hints = hint_names(&book, &glossary);
        assert_eq!(hints.len(), 60);
        assert_eq!(&hints[..3], ["ルルイエ", "コハル", "term0"]);
        assert_eq!(hints.iter().filter(|h| *h == "コハル").count(), 1);
    }

    #[test]
    fn instructions_include_text_and_hints() {
        let with = check_instructions("あリがとう", &["コハル".to_string()]);
        assert!(with.contains("あリがとう"));
        assert!(with.contains("Names and terms that may appear: コハル"));
        let without = check_instructions("あリがとう", &[]);
        assert!(!without.contains("Names and terms that may appear"));
    }
}
