//! Central library of characters and glossary terms, shared by all projects.
//!
//! Items are kept one by one (grouped by a free-form *series* label) in
//! `<app data>/library/library.toml`, so a new chapter can take exactly the
//! characters and terms it needs. Older whole-project presets
//! (`library/preset-*.toml`) are imported once, each preset becoming a series.

use std::collections::HashSet;
use std::io::Write;

use anyhow::{Context, Result, bail};
use atomicwrites::{AtomicFile, OverwriteBehavior};
use camino::{Utf8Path, Utf8PathBuf};
use koharu_core::{Character, CharacterId, Glossary, GlossaryEntry, TypesettingDefaults};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const LIBRARY_DIR: &str = "library";
pub const LIBRARY_FILE: &str = "library.toml";
const MAX_SERIES_CHARS: usize = 80;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LibraryCharacter {
    /// Library item id (not the character id of any project).
    pub key: Uuid,
    #[serde(default)]
    pub series: String,
    #[serde(default)]
    pub updated_at_ms: u64,
    pub character: Character,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LibraryTerm {
    pub key: Uuid,
    #[serde(default)]
    pub series: String,
    #[serde(default)]
    pub updated_at_ms: u64,
    pub entry: GlossaryEntry,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    #[serde(default)]
    pub characters: Vec<LibraryCharacter>,
    #[serde(default)]
    pub terms: Vec<LibraryTerm>,
}

/// `<app data>/library`.
pub fn default_dir() -> Utf8PathBuf {
    koharu_runtime::default_app_data_root().join(LIBRARY_DIR)
}

/// Trimmed series label (may be empty), or an error if it's too long.
pub fn clean_series(series: &str) -> Result<String> {
    let series = series.trim();
    if series.chars().count() > MAX_SERIES_CHARS {
        bail!("series name longer than {MAX_SERIES_CHARS} characters");
    }
    Ok(series.to_string())
}

fn now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

fn key(s: &str) -> String {
    s.trim().to_lowercase()
}

/// The library in `dir`. A missing file is built once from old presets.
pub fn load(dir: &Utf8Path) -> Library {
    let path = dir.join(LIBRARY_FILE);
    match std::fs::read_to_string(path.as_std_path()) {
        Ok(text) => match toml::from_str::<Library>(&text) {
            Ok(lib) => lib,
            Err(e) => {
                let backup = dir.join(format!(
                    "{LIBRARY_FILE}.broken-{}",
                    chrono::Utc::now().format("%Y%m%d%H%M%S")
                ));
                tracing::warn!(path = %path, backup = %backup, "invalid library: {e}");
                let _ = std::fs::rename(path.as_std_path(), backup.as_std_path());
                Library::default()
            }
        },
        Err(_) => {
            let lib = from_old_presets(dir);
            if (!lib.characters.is_empty() || !lib.terms.is_empty())
                && let Err(e) = save(dir, &lib)
            {
                tracing::warn!("failed to save migrated library: {e:#}");
            }
            lib
        }
    }
}

pub fn save(dir: &Utf8Path, lib: &Library) -> Result<()> {
    std::fs::create_dir_all(dir.as_std_path()).with_context(|| format!("create {dir}"))?;
    let text = toml::to_string_pretty(lib).context("encode library")?;
    AtomicFile::new(
        dir.join(LIBRARY_FILE).as_std_path(),
        OverwriteBehavior::AllowOverwrite,
    )
    .write(|f| f.write_all(text.as_bytes()))
    .context("write library atomically")?;
    Ok(())
}

/// Old whole-project preset (`preset-*.toml`).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OldPreset {
    name: String,
    #[serde(default)]
    updated_at_ms: u64,
    #[serde(default)]
    characters: Vec<Character>,
    #[serde(default)]
    glossary: Glossary,
    #[allow(dead_code)]
    #[serde(default)]
    typesetting: Option<TypesettingDefaults>,
}

fn from_old_presets(dir: &Utf8Path) -> Library {
    let mut lib = Library::default();
    let Ok(entries) = std::fs::read_dir(dir.as_std_path()) else {
        return lib;
    };
    let mut presets: Vec<OldPreset> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| Utf8PathBuf::from_path_buf(e.path()).ok())
        .filter(|p| {
            p.extension() == Some("toml") && p.file_name().is_some_and(|n| n.starts_with("preset-"))
        })
        .filter_map(|p| std::fs::read_to_string(p.as_std_path()).ok())
        .filter_map(|t| toml::from_str::<OldPreset>(&t).ok())
        .collect();
    presets.sort_by_key(|p| p.name.to_lowercase());
    for p in presets {
        upsert_characters(&mut lib, &p.name, &p.characters, p.updated_at_ms);
        upsert_terms(&mut lib, &p.name, &p.glossary.entries, p.updated_at_ms);
    }
    lib
}

/// Add or update characters of `series` (matched by name). Returns
/// `(added, updated)`.
pub fn upsert_characters(
    lib: &mut Library,
    series: &str,
    characters: &[Character],
    at_ms: u64,
) -> (usize, usize) {
    let (mut added, mut updated) = (0, 0);
    for c in characters {
        if c.name.trim().is_empty() {
            continue;
        }
        match lib
            .characters
            .iter_mut()
            .find(|i| key(&i.series) == key(series) && key(&i.character.name) == key(&c.name))
        {
            Some(item) => {
                if item.character != *c {
                    item.character = c.clone();
                    item.updated_at_ms = at_ms;
                    updated += 1;
                }
            }
            None => {
                lib.characters.push(LibraryCharacter {
                    key: Uuid::now_v7(),
                    series: series.to_string(),
                    updated_at_ms: at_ms,
                    character: c.clone(),
                });
                added += 1;
            }
        }
    }
    (added, updated)
}

/// Add or update terms of `series` (matched by source). Returns
/// `(added, updated)`.
pub fn upsert_terms(
    lib: &mut Library,
    series: &str,
    entries: &[GlossaryEntry],
    at_ms: u64,
) -> (usize, usize) {
    let (mut added, mut updated) = (0, 0);
    for e in entries {
        if e.source.trim().is_empty() {
            continue;
        }
        match lib
            .terms
            .iter_mut()
            .find(|i| key(&i.series) == key(series) && key(&i.entry.source) == key(&e.source))
        {
            Some(item) => {
                if item.entry != *e {
                    item.entry = e.clone();
                    item.updated_at_ms = at_ms;
                    updated += 1;
                }
            }
            None => {
                lib.terms.push(LibraryTerm {
                    key: Uuid::now_v7(),
                    series: series.to_string(),
                    updated_at_ms: at_ms,
                    entry: e.clone(),
                });
                added += 1;
            }
        }
    }
    (added, updated)
}

/// Save chosen project characters / terms into the library under `series`.
pub fn save_from_project(
    lib: &mut Library,
    series: &str,
    characters: &[Character],
    entries: &[GlossaryEntry],
) -> (usize, usize) {
    let at = now_ms();
    let (ca, cu) = upsert_characters(lib, series, characters, at);
    let (ta, tu) = upsert_terms(lib, series, entries, at);
    (ca + ta, cu + tu)
}

/// Merge another library (an exported file) into `lib`: items are matched
/// by series + name / source term and updated, new ones added. Returns
/// `(added, updated)`.
pub fn merge_library(lib: &mut Library, incoming: &Library) -> (usize, usize) {
    let (mut added, mut updated) = (0, 0);
    for c in &incoming.characters {
        let (a, u) = upsert_characters(
            lib,
            c.series.trim(),
            std::slice::from_ref(&c.character),
            c.updated_at_ms,
        );
        added += a;
        updated += u;
    }
    for t in &incoming.terms {
        let (a, u) = upsert_terms(
            lib,
            t.series.trim(),
            std::slice::from_ref(&t.entry),
            t.updated_at_ms,
        );
        added += a;
        updated += u;
    }
    (added, updated)
}

/// Remove items by key. Returns how many were removed.
pub fn remove(lib: &mut Library, character_keys: &[Uuid], term_keys: &[Uuid]) -> usize {
    let before = lib.characters.len() + lib.terms.len();
    let ck: HashSet<_> = character_keys.iter().collect();
    let tk: HashSet<_> = term_keys.iter().collect();
    lib.characters.retain(|c| !ck.contains(&c.key));
    lib.terms.retain(|t| !tk.contains(&t.key));
    before - lib.characters.len() - lib.terms.len()
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MergeOutcome {
    pub characters_added: usize,
    pub characters_updated: usize,
    pub glossary_added: usize,
    pub glossary_updated: usize,
}

/// Merge characters into `characters`: a character with the same name
/// (case-insensitive) gets the incoming profile and style but keeps its id,
/// so its speaker assignments stay; others are appended.
pub fn merge_characters(
    characters: &mut Vec<Character>,
    incoming: &[Character],
    outcome: &mut MergeOutcome,
) {
    for c in incoming {
        if let Some(existing) = characters.iter_mut().find(|e| key(&e.name) == key(&c.name)) {
            let id = existing.id;
            let next = Character { id, ..c.clone() };
            if *existing != next {
                *existing = next;
                outcome.characters_updated += 1;
            }
            continue;
        }
        let taken: HashSet<CharacterId> = characters.iter().map(|c| c.id).collect();
        let mut added = c.clone();
        if taken.contains(&added.id) {
            added.id = CharacterId::new();
        }
        characters.push(added);
        outcome.characters_added += 1;
    }
}

/// Merge glossary entries by source term (case-insensitive): incoming
/// translation / note win, new terms are appended.
pub fn merge_glossary(
    glossary: &mut Glossary,
    incoming: &[GlossaryEntry],
    outcome: &mut MergeOutcome,
) {
    for e in incoming {
        let k = key(&e.source);
        if k.is_empty() {
            continue;
        }
        if let Some(existing) = glossary.entries.iter_mut().find(|x| key(&x.source) == k) {
            if existing != e {
                *existing = e.clone();
                outcome.glossary_updated += 1;
            }
            continue;
        }
        glossary.entries.push(e.clone());
        outcome.glossary_added += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn saves_items_one_by_one_and_updates_by_name() {
        let (_t, dir) = tmp();
        let mut lib = load(&dir);
        assert_eq!(lib, Library::default());
        save_from_project(
            &mut lib,
            "Ruruie",
            &[ch(1, "Ruruie", "a")],
            &[entry("魔法", "เวท")],
        );
        let (added, updated) = save_from_project(
            &mut lib,
            "Ruruie",
            &[ch(1, "ruruie", "b")],
            &[entry("魔法", "เวท")],
        );
        assert_eq!((added, updated), (0, 1));
        // Same name in another series is a separate item.
        save_from_project(&mut lib, "Other", &[ch(1, "Ruruie", "c")], &[]);
        assert_eq!(lib.characters.len(), 2);
        save(&dir, &lib).unwrap();
        assert_eq!(load(&dir), lib);
        let key = lib.characters[1].key;
        assert_eq!(remove(&mut lib, &[key], &[]), 1);
        assert_eq!(lib.characters.len(), 1);
    }

    #[test]
    fn importing_an_exported_library_merges_by_series_and_name() {
        let mut mine = Library::default();
        save_from_project(&mut mine, "S", &[ch(1, "A", "old")], &[entry("剣", "ดาบ")]);
        let mut other = Library::default();
        save_from_project(
            &mut other,
            "S",
            &[ch(2, "a", "new"), ch(3, "B", "")],
            &[entry("剣", "ดาบ")],
        );
        let (added, updated) = merge_library(&mut mine, &other);
        assert_eq!((added, updated), (1, 1));
        assert_eq!(mine.characters.len(), 2);
        assert_eq!(mine.characters[0].character.description, "new");
    }

    #[test]
    fn old_presets_become_series() {
        let (_t, dir) = tmp();
        std::fs::create_dir_all(dir.as_std_path()).unwrap();
        std::fs::write(
            dir.join("preset-x.toml").as_std_path(),
            "name = \"Series X\"\n[[characters]]\nid = \"00000000-0000-0000-0000-000000000001\"\nname = \"A\"\n[glossary]\n[[glossary.entries]]\nsource = \"剣\"\ntarget = \"ดาบ\"\n",
        )
        .unwrap();
        let lib = load(&dir);
        assert_eq!(lib.characters.len(), 1);
        assert_eq!(lib.characters[0].series, "Series X");
        assert_eq!(lib.terms[0].entry.target, "ดาบ");
        assert!(dir.join(LIBRARY_FILE).exists());
    }

    #[test]
    fn merging_characters_keeps_ids_of_same_name() {
        let mut current = vec![ch(1, "Ruruie", "old"), ch(2, "Man", "")];
        let incoming = vec![ch(9, "ruruie", "new"), ch(2, "Cat", "meow")];
        let mut out = MergeOutcome::default();
        merge_characters(&mut current, &incoming, &mut out);
        assert_eq!(current[0].id, CharacterId(Uuid::from_u128(1)));
        assert_eq!(current[0].description, "new");
        assert_eq!(current.len(), 3);
        assert_ne!(current[2].id, CharacterId(Uuid::from_u128(2)));
        assert_eq!((out.characters_added, out.characters_updated), (1, 1));
    }

    #[test]
    fn merging_glossary_overrides_by_source() {
        let mut g = Glossary {
            entries: vec![entry("魔法", "มายา"), entry("剣", "ดาบ")],
        };
        let mut out = MergeOutcome::default();
        merge_glossary(
            &mut g,
            &[
                entry("魔法", "เวทมนตร์"),
                entry("盾", "โล่"),
                entry("剣", "ดาบ"),
            ],
            &mut out,
        );
        assert_eq!(g.entries[0].target, "เวทมนตร์");
        assert_eq!(g.entries.len(), 3);
        assert_eq!((out.glossary_added, out.glossary_updated), (1, 1));
    }
}
