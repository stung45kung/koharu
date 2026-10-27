//! Series library: named sets of character profiles + glossary (+ project
//! typesetting defaults) kept in the app data folder, so a new chapter of the
//! same series can start with everything the previous chapter had.
//!
//! Each preset is one `library/<file>.toml`; the display name lives inside
//! the file, the file name is only a sanitised copy of it.

use std::collections::HashSet;
use std::io::Write;

use anyhow::{Context, Result, bail};
use atomicwrites::{AtomicFile, OverwriteBehavior};
use camino::{Utf8Path, Utf8PathBuf};
use koharu_core::{Character, CharacterBook, CharacterId, Glossary, TypesettingDefaults};
use serde::{Deserialize, Serialize};

pub const LIBRARY_DIR: &str = "library";
const MAX_NAME_CHARS: usize = 80;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub name: String,
    #[serde(default)]
    pub updated_at_ms: u64,
    #[serde(default)]
    pub characters: Vec<Character>,
    #[serde(default)]
    pub glossary: Glossary,
    #[serde(default)]
    pub typesetting: Option<TypesettingDefaults>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PresetSummary {
    pub name: String,
    pub updated_at_ms: u64,
    pub characters: usize,
    pub glossary_entries: usize,
    pub has_typesetting: bool,
}

impl Preset {
    pub fn summary(&self) -> PresetSummary {
        PresetSummary {
            name: self.name.clone(),
            updated_at_ms: self.updated_at_ms,
            characters: self.characters.len(),
            glossary_entries: self.glossary.entries.len(),
            has_typesetting: self.typesetting.is_some(),
        }
    }
}

/// `<app data>/library`.
pub fn default_dir() -> Utf8PathBuf {
    koharu_runtime::default_app_data_root().join(LIBRARY_DIR)
}

/// Trimmed name, or an error if it's empty / too long.
pub fn clean_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        bail!("preset name must not be empty");
    }
    if name.chars().count() > MAX_NAME_CHARS {
        bail!("preset name longer than {MAX_NAME_CHARS} characters");
    }
    Ok(name.to_string())
}

/// File stem for a preset name: letters/digits (any script) kept, the rest
/// turned into `_`. Prefixed so Windows device names (CON, NUL…) can't occur.
fn file_stem(name: &str) -> String {
    let body: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c.to_lowercase().next().unwrap_or(c)
            } else {
                '_'
            }
        })
        .collect();
    format!("preset-{body}")
}

fn path_for(dir: &Utf8Path, name: &str) -> Utf8PathBuf {
    dir.join(format!("{}.toml", file_stem(name)))
}

fn read(path: &Utf8Path) -> Option<Preset> {
    let text = std::fs::read_to_string(path.as_std_path()).ok()?;
    match toml::from_str::<Preset>(&text) {
        Ok(p) => Some(p),
        Err(e) => {
            tracing::warn!(path = %path, "invalid library preset: {e}");
            None
        }
    }
}

/// All presets in `dir`, sorted by name. A missing folder is an empty list.
pub fn list(dir: &Utf8Path) -> Vec<Preset> {
    let Ok(entries) = std::fs::read_dir(dir.as_std_path()) else {
        return Vec::new();
    };
    let mut out: Vec<Preset> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| Utf8PathBuf::from_path_buf(e.path()).ok())
        .filter(|p| p.extension() == Some("toml"))
        .filter_map(|p| read(&p))
        .collect();
    out.sort_by_key(|p| p.name.to_lowercase());
    out
}

/// The preset called `name` (case-insensitive).
pub fn get(dir: &Utf8Path, name: &str) -> Option<Preset> {
    let wanted = name.trim().to_lowercase();
    if let Some(p) = read(&path_for(dir, name)).filter(|p| p.name.to_lowercase() == wanted) {
        return Some(p);
    }
    list(dir)
        .into_iter()
        .find(|p| p.name.to_lowercase() == wanted)
}

/// Write `preset` (replacing one of the same name).
pub fn save(dir: &Utf8Path, preset: &Preset) -> Result<()> {
    std::fs::create_dir_all(dir.as_std_path()).with_context(|| format!("create {dir}"))?;
    let text = toml::to_string_pretty(preset).context("encode preset")?;
    AtomicFile::new(
        path_for(dir, &preset.name).as_std_path(),
        OverwriteBehavior::AllowOverwrite,
    )
    .write(|f| f.write_all(text.as_bytes()))
    .context("write preset atomically")?;
    Ok(())
}

/// Delete the preset called `name`. Returns whether one existed.
pub fn delete(dir: &Utf8Path, name: &str) -> Result<bool> {
    let wanted = name.trim().to_lowercase();
    let Ok(entries) = std::fs::read_dir(dir.as_std_path()) else {
        return Ok(false);
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let Ok(path) = Utf8PathBuf::from_path_buf(entry.path()) else {
            continue;
        };
        if path.extension() == Some("toml")
            && read(&path).is_some_and(|p| p.name.to_lowercase() == wanted)
        {
            std::fs::remove_file(path.as_std_path()).with_context(|| format!("delete {path}"))?;
            return Ok(true);
        }
    }
    Ok(false)
}

/// Snapshot of a project's characters / glossary / typesetting as a preset.
pub fn from_project(
    name: String,
    book: &CharacterBook,
    glossary: &Glossary,
    typesetting: &TypesettingDefaults,
) -> Preset {
    Preset {
        name,
        updated_at_ms: chrono::Utc::now().timestamp_millis().max(0) as u64,
        characters: book.characters.clone(),
        glossary: glossary.clone(),
        typesetting: (*typesetting != TypesettingDefaults::default()).then(|| typesetting.clone()),
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MergeOutcome {
    pub characters_added: usize,
    pub characters_updated: usize,
    pub glossary_added: usize,
    pub glossary_updated: usize,
    pub typesetting_applied: bool,
}

/// Merge the preset's characters into `characters`: a character with the
/// same name (case-insensitive) gets the preset's profile and style but keeps
/// its id, so its speaker assignments stay; others are appended.
pub fn merge_characters(
    characters: &mut Vec<Character>,
    preset: &[Character],
    outcome: &mut MergeOutcome,
) {
    for incoming in preset {
        let key = incoming.name.trim().to_lowercase();
        if let Some(existing) = characters
            .iter_mut()
            .find(|c| c.name.trim().to_lowercase() == key)
        {
            let id = existing.id;
            if *existing
                != (Character {
                    id,
                    ..incoming.clone()
                })
            {
                *existing = Character {
                    id,
                    ..incoming.clone()
                };
                outcome.characters_updated += 1;
            }
            continue;
        }
        let taken: HashSet<CharacterId> = characters.iter().map(|c| c.id).collect();
        let mut added = incoming.clone();
        if taken.contains(&added.id) {
            added.id = CharacterId::new();
        }
        characters.push(added);
        outcome.characters_added += 1;
    }
}

/// Merge glossary entries by source term (case-insensitive): the preset's
/// translation / note win, new terms are appended.
pub fn merge_glossary(glossary: &mut Glossary, preset: &Glossary, outcome: &mut MergeOutcome) {
    for incoming in &preset.entries {
        let key = incoming.source.trim().to_lowercase();
        if key.is_empty() {
            continue;
        }
        if let Some(existing) = glossary
            .entries
            .iter_mut()
            .find(|e| e.source.trim().to_lowercase() == key)
        {
            if existing != incoming {
                *existing = incoming.clone();
                outcome.glossary_updated += 1;
            }
            continue;
        }
        glossary.entries.push(incoming.clone());
        outcome.glossary_added += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use koharu_core::GlossaryEntry;
    use uuid::Uuid;

    fn tmp() -> (tempfile::TempDir, Utf8PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let p = Utf8PathBuf::from_path_buf(t.path().to_path_buf()).unwrap();
        (t, p.join(LIBRARY_DIR))
    }

    fn ch(n: u128, name: &str, desc: &str) -> Character {
        Character {
            id: CharacterId(Uuid::from_u128(n)),
            name: name.into(),
            description: desc.into(),
            ..Default::default()
        }
    }

    fn entry(source: &str, target: &str) -> GlossaryEntry {
        GlossaryEntry {
            source: source.into(),
            target: target.into(),
            note: String::new(),
        }
    }

    #[test]
    fn save_list_get_delete_round_trip() {
        let (_t, dir) = tmp();
        assert!(list(&dir).is_empty());
        let preset = Preset {
            name: "มาจิคัล★ルルイエ".into(),
            updated_at_ms: 1,
            characters: vec![ch(1, "Ruruie", "sharp")],
            glossary: Glossary {
                entries: vec![entry("魔法", "เวทมนตร์")],
            },
            typesetting: None,
        };
        save(&dir, &preset).unwrap();
        assert_eq!(list(&dir), vec![preset.clone()]);
        assert_eq!(get(&dir, "  มาจิคัล★ルルイエ ").unwrap(), preset);
        // Saving again under the same name replaces it.
        let renamed = Preset {
            updated_at_ms: 2,
            ..preset.clone()
        };
        save(&dir, &renamed).unwrap();
        assert_eq!(list(&dir).len(), 1);
        assert!(delete(&dir, "มาจิคัล★ルルイエ").unwrap());
        assert!(!delete(&dir, "มาจิคัล★ルルイエ").unwrap());
        assert!(list(&dir).is_empty());
    }

    #[test]
    fn file_names_are_safe() {
        assert_eq!(file_stem("CON"), "preset-con");
        assert_eq!(file_stem("../a b"), "preset-___a_b");
        assert!(clean_name("   ").is_err());
    }

    #[test]
    fn merging_characters_keeps_ids_of_same_name() {
        let mut current = vec![ch(1, "Ruruie", "old"), ch(2, "Man", "")];
        let preset = vec![ch(9, "ruruie", "new"), ch(2, "Cat", "meow")];
        let mut out = MergeOutcome::default();
        merge_characters(&mut current, &preset, &mut out);
        assert_eq!(current[0].id, CharacterId(Uuid::from_u128(1)));
        assert_eq!(current[0].description, "new");
        assert_eq!(current.len(), 3);
        // Id 2 was taken by "Man", so "Cat" got a fresh one.
        assert_ne!(current[2].id, CharacterId(Uuid::from_u128(2)));
        assert_eq!((out.characters_added, out.characters_updated), (1, 1));
    }

    #[test]
    fn merging_glossary_overrides_by_source() {
        let mut g = Glossary {
            entries: vec![entry("魔法", "มายา"), entry("剣", "ดาบ")],
        };
        let preset = Glossary {
            entries: vec![
                entry("魔法", "เวทมนตร์"),
                entry("盾", "โล่"),
                entry("剣", "ดาบ"),
            ],
        };
        let mut out = MergeOutcome::default();
        merge_glossary(&mut g, &preset, &mut out);
        assert_eq!(g.entries[0].target, "เวทมนตร์");
        assert_eq!(g.entries.len(), 3);
        assert_eq!((out.glossary_added, out.glossary_updated), (1, 1));
    }
}
