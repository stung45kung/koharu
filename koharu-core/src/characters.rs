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

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Character {
    pub id: CharacterId,
    pub name: String,
    /// Free-form speech-style notes (pronouns, register, verbal tics, ...).
    #[serde(default)]
    pub description: String,
    /// Font for this character's blocks (PostScript name or Google
    /// `Family:Weight` query), used when the block has no font of its own.
    #[serde(default)]
    pub font: Option<String>,
    /// Text colour (RGBA) for this character's blocks, used when the block
    /// has no style of its own.
    #[serde(default)]
    pub color: Option<[u8; 4]>,
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
    /// Nodes in `speakers` whose speaker was guessed automatically and not
    /// yet confirmed by the user. Guessing never overwrites a node that is
    /// assigned but not listed here.
    #[serde(default)]
    pub suggested: Vec<NodeId>,
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
            suggested: Vec::new(),
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
        self.prune_suggested();
    }

    /// True when `node`'s speaker came from an unconfirmed guess.
    pub fn is_suggested(&self, node: NodeId) -> bool {
        self.suggested.contains(&node)
    }

    /// True when the user has not fixed `node`'s speaker: it is unknown, or
    /// only guessed. Automatic guessing may (re)write these nodes only.
    pub fn is_open_for_guess(&self, node: NodeId) -> bool {
        !self.speakers.contains_key(&node) || self.is_suggested(node)
    }

    /// Record an automatic guess. Ignored for nodes the user has assigned
    /// themselves. `None` clears a previous guess back to unknown.
    /// Returns whether the node changed.
    pub fn suggest(&mut self, node: NodeId, character: Option<CharacterId>) -> bool {
        if !self.is_open_for_guess(node) {
            return false;
        }
        match character {
            Some(id) if self.character(id).is_some() => {
                let changed = self.speakers.get(&node) != Some(&id) || !self.is_suggested(node);
                self.speakers.insert(node, id);
                if !self.is_suggested(node) {
                    self.suggested.push(node);
                }
                changed
            }
            _ => {
                let changed = self.speakers.shift_remove(&node).is_some();
                self.suggested.retain(|n| *n != node);
                changed
            }
        }
    }

    /// Turn guesses on `nodes` into confirmed assignments.
    pub fn confirm(&mut self, nodes: &[NodeId]) {
        self.suggested.retain(|n| !nodes.contains(n));
    }

    fn prune_suggested(&mut self) {
        let speakers = &self.speakers;
        self.suggested.retain(|n| speakers.contains_key(n));
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
        // A manual choice (including "unknown") is a confirmed decision.
        self.confirm(nodes);
        Ok(())
    }

    /// System prompt asking the model to label the speaker of each tagged
    /// block (tag `[i + 1]` is `blocks[i]`). Characters are labelled `S1..Sn`
    /// in book order; speakers the user already confirmed are given as
    /// fixed context. `None` when there are no characters to choose from.
    ///
    /// Every LLM path appends the block-tag rules ("output the same tags
    /// followed by the translated text"), so the task is phrased as a
    /// "translation" of each block into its speaker label.
    ///
    /// `layout[i]` (optional, may be shorter than `blocks`) describes where
    /// block `i` sits on the page; `previous` is `(speaker, line)` from the
    /// end of the previous page, given as conversation context only.
    pub fn speaker_guess_prompt(
        &self,
        blocks: &[NodeId],
        layout: &[String],
        previous: &[(String, String)],
    ) -> Option<String> {
        if self.characters.is_empty() {
            return None;
        }
        let mut out = String::new();
        out.push_str(
            "You are assisting a manga translator. Do NOT translate anything. Your only \
             task is to decide which character speaks each numbered text block on this \
             page. The blocks are given in reading order.\n\n\
             How to decide:\n\
             - Match how the line is spoken (self-reference, sentence endings, politeness, \
             tone) against the character profiles.\n\
             - Follow the conversation: a question is usually answered by someone else; \
             blocks close together on the page are usually one exchange.\n\
             - Use the block positions to see which lines belong together.\n\
             - Short interjections or reactions (\"eh...\", \"hmm\", \"no...\") rarely \
             reveal the speaker on their own: label them only when the surrounding \
             conversation makes it clear.\n\
             - Precision matters more than coverage. A wrong label is worse than ?. \
             Answer ? whenever you are not confident.\n\n\
             For this task the \"translation\" of a block is ONLY its speaker label: \
             after each tag write exactly one label (S1, S2, ... or ?) and nothing else.\n",
        );
        out.push_str("\nCharacters:\n");
        for (i, c) in self.characters.iter().enumerate() {
            let name = one_line(&c.name);
            let desc = one_line(&truncate_chars(c.description.trim(), MAX_DESCRIPTION_CHARS));
            if desc.is_empty() {
                out.push_str(&format!("- S{}: {}\n", i + 1, name));
            } else {
                out.push_str(&format!("- S{}: {} — {}\n", i + 1, name, desc));
            }
        }
        let known: Vec<String> = blocks
            .iter()
            .enumerate()
            .filter(|(_, node)| !self.is_suggested(**node))
            .filter_map(|(i, node)| {
                let id = self.speakers.get(node)?;
                let pos = self.characters.iter().position(|c| c.id == *id)?;
                Some(format!("[{}] S{}", i + 1, pos + 1))
            })
            .collect();
        if !known.is_empty() {
            out.push_str("\nAlready confirmed by the user (keep these labels):\n");
            out.push_str(&known.join("\n"));
            out.push('\n');
        }
        let positions: Vec<String> = layout
            .iter()
            .take(blocks.len())
            .enumerate()
            .filter(|(_, p)| !p.trim().is_empty())
            .map(|(i, p)| format!("[{}] {}", i + 1, one_line(p)))
            .collect();
        if !positions.is_empty() {
            out.push_str("\nBlock positions on the page:\n");
            out.push_str(&positions.join("\n"));
            out.push('\n');
        }
        if !previous.is_empty() {
            out.push_str("\nEnd of the previous page (context only; do not label these):\n");
            for (speaker, line) in previous {
                out.push_str(&format!(
                    "- {}: {}\n",
                    one_line(speaker),
                    one_line(&truncate_chars(line, 120))
                ));
            }
        }
        Some(out.trim_end().to_string())
    }

    /// Map a label from the guess response (`S2`, `s2.`, `S2 (Name)`, or a
    /// character's exact name) back to a character. `?`/anything else → None.
    pub fn parse_speaker_label(&self, label: &str) -> Option<CharacterId> {
        let label = label.trim().trim_matches(|c: char| c == '*' || c == '`');
        if label.is_empty() {
            return None;
        }
        let token: String = label
            .split(|c: char| c.is_whitespace() || c == '(' || c == ':' || c == '-')
            .next()
            .unwrap_or("")
            .trim_end_matches(['.', ',', ')'])
            .to_string();
        if let Some(num) = token.strip_prefix(['S', 's'])
            && let Ok(n) = num.parse::<usize>()
            && n >= 1
        {
            return self.characters.get(n - 1).map(|c| c.id);
        }
        self.characters
            .iter()
            .find(|c| c.name.trim().eq_ignore_ascii_case(label))
            .map(|c| c.id)
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
            ..Default::default()
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
    fn guesses_never_override_user_choices() {
        let mut book = book();
        let (a, b) = (book.characters[0].id, book.characters[1].id);
        book.assign(&[node(1)], Some(a)).unwrap();

        assert!(
            !book.suggest(node(1), Some(b)),
            "confirmed node must not change"
        );
        assert_eq!(book.speaker_of(node(1)).map(|c| c.id), Some(a));

        assert!(book.suggest(node(2), Some(b)));
        assert!(book.is_suggested(node(2)));
        // A later guess may revise an earlier guess, or clear it.
        assert!(book.suggest(node(2), Some(a)));
        assert!(book.suggest(node(2), None));
        assert!(book.speaker_of(node(2)).is_none());
        assert!(!book.is_suggested(node(2)));
    }

    #[test]
    fn manual_assignment_and_confirm_clear_suggested_mark() {
        let mut book = book();
        let a = book.characters[0].id;
        book.suggest(node(1), Some(a));
        book.suggest(node(2), Some(a));
        book.assign(&[node(1)], Some(a)).unwrap();
        assert!(!book.is_suggested(node(1)));
        book.confirm(&[node(2)]);
        assert!(book.suggested.is_empty());
        assert!(book.speaker_of(node(2)).is_some());
    }

    #[test]
    fn deleting_character_drops_its_suggestions() {
        let mut book = book();
        let a = book.characters[0].id;
        book.suggest(node(1), Some(a));
        let keep = vec![book.characters[1].clone()];
        book.set_characters(keep);
        assert!(book.suggested.is_empty());
    }

    #[test]
    fn guess_prompt_lists_characters_and_confirmed_hints() {
        let mut book = book();
        let a = book.characters[0].id;
        assert_eq!(
            CharacterBook::default().speaker_guess_prompt(&[node(1)], &[], &[]),
            None
        );

        book.assign(&[node(2)], Some(a)).unwrap();
        book.suggest(node(3), Some(a));
        let layout = vec![
            "top-right".to_string(),
            String::new(),
            "bottom-left".to_string(),
        ];
        let previous = vec![("Akazukin".to_string(), "line\nbefore".to_string())];
        let prompt = book
            .speaker_guess_prompt(&[node(1), node(2), node(3)], &layout, &previous)
            .unwrap();
        assert!(prompt.contains("[1] top-right"));
        assert!(prompt.contains("[3] bottom-left"));
        assert!(prompt.contains("- Akazukin: line before"));
        assert!(prompt.contains("do not label these"));
        assert!(prompt.contains("- S1: Akazukin"));
        assert!(prompt.contains("- S2: ผู้บรรยาย"));
        assert!(prompt.contains("[2] S1"));
        // Unconfirmed guesses are not presented as fixed context.
        assert!(!prompt.contains("[3] S1"));
    }

    #[test]
    fn parses_speaker_labels_leniently() {
        let book = book();
        let (a, b) = (book.characters[0].id, book.characters[1].id);
        assert_eq!(book.parse_speaker_label("S1"), Some(a));
        assert_eq!(book.parse_speaker_label(" s2. "), Some(b));
        assert_eq!(book.parse_speaker_label("S1 (Akazukin)"), Some(a));
        assert_eq!(book.parse_speaker_label("**S2**"), Some(b));
        assert_eq!(book.parse_speaker_label("akazukin"), Some(a));
        assert_eq!(book.parse_speaker_label("?"), None);
        assert_eq!(book.parse_speaker_label("S9"), None);
        assert_eq!(book.parse_speaker_label("S0"), None);
        assert_eq!(book.parse_speaker_label(""), None);
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
