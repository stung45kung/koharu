//! A loaded project. One `ProjectSession` = one `.khrproj/` directory.
//!
//! Holds:
//!   - an exclusive `.lock` via `fs4` (refuses second opener)
//!   - the in-memory `Scene` behind a `parking_lot::RwLock` (never held across `.await`)
//!   - the `History` behind a `Mutex` (linear, all writes serialized)
//!   - the `BlobStore` (content-addressed images)
//!
//! On-disk layout:
//!   `.khrproj/project.toml`    — TOML-encoded `ProjectMeta`
//!   `.khrproj/scene.bin`       — postcard-encoded `Snapshot { epoch, scene }`
//!   `.khrproj/history.log`     — append-only `LogFrame { epoch, op }`
//!   `.khrproj/characters.toml` — character profiles + speaker map (optional)
//!   `.khrproj/typesetting.toml` — project default text size (optional)
//!   `.khrproj/glossary.toml`    — fixed term translations (optional)
//!   `.khrproj/blobs/ab/cdef…`  — content-addressed blobs
//!   `.khrproj/.lock`           — fs4 exclusive lock (session lifetime)

use std::fs::File;
use std::io::Write;
use std::sync::Arc;

use anyhow::{Context, Result};
use atomicwrites::{AtomicFile, OverwriteBehavior};
use camino::{Utf8Path, Utf8PathBuf};
use chrono::Utc;
use fs4::FileExt;
use koharu_core::{CharacterBook, Glossary, Scene, TypesettingDefaults, op::Op};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};

use crate::blobs::BlobStore;
use crate::characters;
use crate::glossary;
use crate::history::{self, History};
use crate::typesetting;

const SCENE_FILE: &str = "scene.bin";
const LOG_FILE: &str = "history.log";
const LOCK_FILE: &str = ".lock";
const BLOBS_DIR: &str = "blobs";
const CACHE_DIR: &str = "cache";
const PROJECT_TOML: &str = "project.toml";

/// Snapshot written to `scene.bin`.
#[derive(Serialize, Deserialize)]
struct Snapshot {
    epoch: u64,
    scene: Scene,
}

/// A loaded project.
pub struct ProjectSession {
    pub dir: Utf8PathBuf,
    pub scene: RwLock<Scene>,
    pub history: Mutex<History>,
    pub blobs: Arc<BlobStore>,
    /// Character profiles + text-node speakers, mirrored to `characters.toml`.
    characters: RwLock<CharacterBook>,
    /// Project default text size, mirrored to `typesetting.toml`.
    typesetting: RwLock<TypesettingDefaults>,
    /// Project glossary, mirrored to `glossary.toml`.
    glossary: RwLock<Glossary>,
    /// Interleaving of scene ops and character-book edits, so one Ctrl+Z
    /// undoes whichever happened last.
    timeline: Mutex<Timeline>,
    /// Held for the lifetime of the session.
    _lock: File,
}

/// What a single undo / redo step reverted or re-applied.
#[derive(Debug, Clone)]
pub enum UndoStep {
    /// A scene op (new epoch, the op that was applied).
    Scene(u64, Op),
    /// The character book (speakers / profiles) went back to an earlier
    /// version — the scene epoch is unchanged.
    Characters,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StepKind {
    Scene,
    Characters,
}

/// Max character-book versions kept for undo.
const CHARACTER_UNDO_LIMIT: usize = 200;
/// Max interleaved steps remembered (the scene history has its own cap).
const TIMELINE_LIMIT: usize = 2000;

#[derive(Default)]
struct Timeline {
    undo: std::collections::VecDeque<StepKind>,
    redo: Vec<StepKind>,
    /// Character book *before* each undoable character edit.
    books_undo: std::collections::VecDeque<CharacterBook>,
    /// Character book *before* each undone edit was reverted.
    books_redo: Vec<CharacterBook>,
}

impl Timeline {
    fn record(&mut self, kind: StepKind) {
        self.undo.push_back(kind);
        while self.undo.len() > TIMELINE_LIMIT {
            self.undo.pop_front();
        }
        // A new action invalidates everything that could be redone.
        self.redo.clear();
        self.books_redo.clear();
    }

    fn record_book(&mut self, before: CharacterBook) {
        self.books_undo.push_back(before);
        if self.books_undo.len() > CHARACTER_UNDO_LIMIT {
            self.books_undo.pop_front();
            // Keep the step list consistent: forget the oldest book step.
            if let Some(i) = self.undo.iter().position(|k| *k == StepKind::Characters) {
                self.undo.remove(i);
            }
        }
        self.record(StepKind::Characters);
    }
}

impl ProjectSession {
    /// Open an existing `.khrproj/` directory.
    pub fn open(dir: impl AsRef<Utf8Path>) -> Result<Arc<Self>> {
        let dir = dir.as_ref().to_path_buf();
        if !dir.is_dir() {
            anyhow::bail!("not a project directory: {dir}");
        }
        Self::open_inner(dir, false)
    }

    /// Create a fresh `.khrproj/` at `dir`, failing if it already exists.
    pub fn create(dir: impl AsRef<Utf8Path>, name: impl Into<String>) -> Result<Arc<Self>> {
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(dir.as_std_path())
            .with_context(|| format!("create project dir {dir}"))?;
        // Project should be empty.
        let is_empty = std::fs::read_dir(dir.as_std_path())?.next().is_none();
        if !is_empty {
            anyhow::bail!("project directory not empty: {dir}");
        }
        // Seed the TOML with the name so open_inner can load it.
        let meta = ProjectTomlFile {
            name: name.into(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        std::fs::write(
            dir.join(PROJECT_TOML).as_std_path(),
            toml::to_string_pretty(&meta)?,
        )?;
        Self::open_inner(dir, true)
    }

    fn open_inner(dir: Utf8PathBuf, creating: bool) -> Result<Arc<Self>> {
        std::fs::create_dir_all(dir.join(BLOBS_DIR).as_std_path())?;
        std::fs::create_dir_all(dir.join(CACHE_DIR).as_std_path())?;

        // Exclusive lock — one opener at a time.
        let lock_path = dir.join(LOCK_FILE);
        let lock = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path.as_std_path())
            .with_context(|| format!("open lock file {}", lock_path))?;
        FileExt::try_lock(&lock).context("project is already open elsewhere")?;

        let blobs = Arc::new(BlobStore::open(dir.join(BLOBS_DIR).as_std_path())?);

        // Load or synthesize the scene + epoch.
        let (mut scene, mut epoch) = load_snapshot(&dir, creating)?;
        // Replay any log frames past the snapshot epoch.
        let log_path = dir.join(LOG_FILE);
        epoch = history::replay(log_path.as_std_path(), epoch, &mut scene)
            .with_context(|| format!("replay log {}", log_path))?;

        let history_obj = History::open(log_path.as_std_path(), epoch)?;
        let characters = RwLock::new(characters::load(&dir));
        let typesetting = RwLock::new(typesetting::load(&dir));
        let glossary = RwLock::new(glossary::load(&dir));

        Ok(Arc::new(Self {
            dir,
            scene: RwLock::new(scene),
            history: Mutex::new(history_obj),
            blobs,
            characters,
            typesetting,
            glossary,
            timeline: Mutex::new(Timeline::default()),
            _lock: lock,
        }))
    }

    // --- scene mutation ----------------------------------------------------

    /// Apply an Op. Returns the epoch after apply.
    // Lock order everywhere: timeline → characters → history → scene.
    pub fn apply(&self, op: Op) -> Result<u64> {
        let mut timeline = self.timeline.lock();
        let mut history = self.history.lock();
        let mut scene = self.scene.write();
        let epoch = history.apply(&mut scene, op)?;
        timeline.record(StepKind::Scene);
        Ok(epoch)
    }

    /// Undo the most recent scene op or character-book edit.
    pub fn undo(&self) -> Result<Option<UndoStep>> {
        let mut timeline = self.timeline.lock();
        match timeline.undo.pop_back() {
            Some(StepKind::Characters) => {
                let Some(before) = timeline.books_undo.pop_back() else {
                    return Ok(None);
                };
                let mut guard = self.characters.write();
                characters::save(&self.dir, &before)?;
                let current = std::mem::replace(&mut *guard, before);
                timeline.books_redo.push(current);
                timeline.redo.push(StepKind::Characters);
                Ok(Some(UndoStep::Characters))
            }
            // `None`: steps from before the timeline cap — let the scene
            // history decide.
            kind => {
                let mut history = self.history.lock();
                let mut scene = self.scene.write();
                let out = history.undo(&mut scene)?;
                if out.is_some() {
                    timeline.redo.push(StepKind::Scene);
                } else if let Some(kind) = kind {
                    // Nothing left to undo; keep the step list as it was.
                    timeline.undo.push_back(kind);
                }
                Ok(out.map(|(e, op)| UndoStep::Scene(e, op)))
            }
        }
    }

    /// Redo the most recently undone step.
    pub fn redo(&self) -> Result<Option<UndoStep>> {
        let mut timeline = self.timeline.lock();
        match timeline.redo.pop() {
            None => Ok(None),
            Some(StepKind::Characters) => {
                let Some(after) = timeline.books_redo.pop() else {
                    return Ok(None);
                };
                let mut guard = self.characters.write();
                characters::save(&self.dir, &after)?;
                let current = std::mem::replace(&mut *guard, after);
                timeline.books_undo.push_back(current);
                timeline.undo.push_back(StepKind::Characters);
                Ok(Some(UndoStep::Characters))
            }
            Some(StepKind::Scene) => {
                let mut history = self.history.lock();
                let mut scene = self.scene.write();
                let out = history.redo(&mut scene)?;
                if out.is_some() {
                    timeline.undo.push_back(StepKind::Scene);
                }
                Ok(out.map(|(e, op)| UndoStep::Scene(e, op)))
            }
        }
    }

    pub fn epoch(&self) -> u64 {
        self.history.lock().epoch()
    }

    /// Cheap clone of the scene for read-only consumers (pipeline engines).
    pub fn scene_snapshot(&self) -> Scene {
        self.scene.read().clone()
    }

    // --- characters ---------------------------------------------------------

    /// Current character book (cheap clone; small).
    pub fn characters(&self) -> CharacterBook {
        self.characters.read().clone()
    }

    /// Mutate the character book and persist it. The in-memory copy only
    /// changes if both `f` and the disk write succeed.
    pub fn update_characters<R>(
        &self,
        f: impl FnOnce(&mut CharacterBook) -> Result<R>,
    ) -> Result<(R, CharacterBook)> {
        let mut timeline = self.timeline.lock();
        let mut guard = self.characters.write();
        let mut next = guard.clone();
        let out = f(&mut next)?;
        if next != *guard {
            characters::save(&self.dir, &next)?;
            let before = std::mem::replace(&mut *guard, next.clone());
            timeline.record_book(before);
        }
        Ok((out, next))
    }

    // --- typesetting --------------------------------------------------------

    pub fn typesetting(&self) -> TypesettingDefaults {
        self.typesetting.read().clone()
    }

    /// Replace the typesetting defaults and persist them.
    pub fn set_typesetting(&self, defaults: TypesettingDefaults) -> Result<()> {
        let mut guard = self.typesetting.write();
        typesetting::save(&self.dir, &defaults)?;
        *guard = defaults;
        Ok(())
    }

    // --- glossary -----------------------------------------------------------

    pub fn glossary(&self) -> Glossary {
        self.glossary.read().clone()
    }

    /// Replace the glossary and persist it.
    pub fn set_glossary(&self, glossary: Glossary) -> Result<()> {
        let mut guard = self.glossary.write();
        glossary::save(&self.dir, &glossary)?;
        *guard = glossary;
        Ok(())
    }

    // --- compaction --------------------------------------------------------

    /// Write a new snapshot (scene.bin) and truncate the log. Safe to call
    /// at any time; crash mid-compaction leaves the old snapshot + full log.
    pub fn compact(&self) -> Result<()> {
        let snap = {
            let scene = self.scene.read();
            let epoch = self.history.lock().epoch();
            Snapshot {
                epoch,
                scene: scene.clone(),
            }
        };
        let bytes = postcard::to_allocvec(&snap).context("encode snapshot")?;
        AtomicFile::new(
            self.dir.join(SCENE_FILE).as_std_path(),
            OverwriteBehavior::AllowOverwrite,
        )
        .write(|f| f.write_all(&bytes))
        .context("write scene.bin atomically")?;
        // Log truncation only after snapshot is durably on disk.
        self.history.lock().truncate_log()?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Snapshot loading / TOML metadata
// ---------------------------------------------------------------------------

fn load_snapshot(dir: &Utf8Path, creating: bool) -> Result<(Scene, u64)> {
    let scene_path = dir.join(SCENE_FILE);
    if scene_path.exists() {
        let bytes = std::fs::read(scene_path.as_std_path())
            .with_context(|| format!("read {}", scene_path))?;
        let snap: Snapshot =
            postcard::from_bytes(&bytes).with_context(|| format!("decode {}", scene_path))?;
        return Ok((snap.scene, snap.epoch));
    }

    // No snapshot — build one from `project.toml` (or defaults for creation).
    let toml_path = dir.join(PROJECT_TOML);
    let meta = if toml_path.exists() {
        let text = std::fs::read_to_string(toml_path.as_std_path())?;
        toml::from_str::<ProjectTomlFile>(&text).with_context(|| format!("parse {}", toml_path))?
    } else if creating {
        ProjectTomlFile {
            name: String::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    } else {
        anyhow::bail!("missing project.toml at {}", toml_path);
    };

    let mut scene = Scene::default();
    scene.project.name = meta.name;
    scene.project.created_at = meta.created_at;
    scene.project.updated_at = meta.updated_at;
    Ok((scene, 0))
}

#[derive(Serialize, Deserialize)]
struct ProjectTomlFile {
    name: String,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use koharu_core::{
        Node, NodeId, NodeKind, Op, Page, PageId, TextData, TextShaderEffect, TextStyle, Transform,
    };
    use tempfile::tempdir;

    fn tmp_dir() -> (tempfile::TempDir, Utf8PathBuf) {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        (dir, path.join("proj.khrproj"))
    }

    #[test]
    fn undo_and_redo_follow_scene_and_character_edits_in_order() {
        use koharu_core::{Character, CharacterId};
        let (_tmp, path) = tmp_dir();
        let session = ProjectSession::create(&path, "undo").unwrap();
        let narrator = CharacterId::new();

        // 1. scene op, 2. character edit, 3. scene op
        session
            .apply(Op::AddPage {
                page: Page::new("p1", 10, 10),
                at: 0,
            })
            .unwrap();
        session
            .update_characters(|book| {
                book.set_characters(vec![Character {
                    id: narrator,
                    name: "N".into(),
                    ..Default::default()
                }]);
                Ok(())
            })
            .unwrap();
        session
            .apply(Op::AddPage {
                page: Page::new("p2", 10, 10),
                at: 1,
            })
            .unwrap();
        let pages = |s: &ProjectSession| s.scene.read().pages.len();

        assert!(matches!(session.undo().unwrap(), Some(UndoStep::Scene(..))));
        assert_eq!(pages(&session), 1);
        assert_eq!(session.characters().characters.len(), 1);

        assert!(matches!(
            session.undo().unwrap(),
            Some(UndoStep::Characters)
        ));
        assert!(session.characters().characters.is_empty());
        assert_eq!(pages(&session), 1);
        // Undo reached the file too.
        assert!(characters::load(&session.dir).characters.is_empty());

        assert!(matches!(session.undo().unwrap(), Some(UndoStep::Scene(..))));
        assert_eq!(pages(&session), 0);
        assert!(session.undo().unwrap().is_none());

        assert!(matches!(session.redo().unwrap(), Some(UndoStep::Scene(..))));
        assert!(matches!(
            session.redo().unwrap(),
            Some(UndoStep::Characters)
        ));
        assert_eq!(session.characters().characters.len(), 1);
        assert!(matches!(session.redo().unwrap(), Some(UndoStep::Scene(..))));
        assert_eq!(pages(&session), 2);
        assert!(session.redo().unwrap().is_none());

        // A new edit clears redo.
        session.undo().unwrap();
        session
            .update_characters(|book| {
                book.set_characters(Vec::new());
                Ok(())
            })
            .unwrap();
        assert!(session.redo().unwrap().is_none());
    }

    #[test]
    fn unchanged_character_book_is_not_an_undo_step() {
        let (_tmp, path) = tmp_dir();
        let session = ProjectSession::create(&path, "noop").unwrap();
        session.update_characters(|_| Ok(())).unwrap();
        assert!(session.undo().unwrap().is_none());
    }

    #[test]
    fn create_apply_close_reopen_preserves_scene() {
        let (_tmp, path) = tmp_dir();
        let page_id: PageId;
        {
            let session = ProjectSession::create(&path, "test").unwrap();
            let page = Page::new("p1", 800, 600);
            page_id = page.id;
            session
                .apply(Op::AddPage { page, at: 0 })
                .expect("apply AddPage");
            session.compact().unwrap();
            // Session drops, lock released.
        }
        let session = ProjectSession::open(&path).unwrap();
        assert_eq!(session.scene.read().pages.len(), 1);
        assert!(session.scene.read().pages.contains_key(&page_id));
    }

    #[test]
    fn reopen_preserves_text_style_effects_in_scene_bin() {
        let (_tmp, path) = tmp_dir();
        let page_id: PageId;
        let node_id: NodeId;
        {
            let session = ProjectSession::create(&path, "styled").unwrap();
            let page = Page::new("p1", 800, 600);
            page_id = page.id;
            session
                .apply(Op::AddPage { page, at: 0 })
                .expect("apply AddPage");

            node_id = NodeId::new();
            let mut scene = session.scene.write();
            let page = scene.pages.get_mut(&page_id).expect("page");
            page.nodes.insert(
                node_id,
                Node {
                    id: node_id,
                    transform: Transform {
                        x: 0.0,
                        y: 0.0,
                        width: 100.0,
                        height: 40.0,
                        rotation_deg: 0.0,
                    },
                    visible: true,
                    kind: NodeKind::Text(TextData {
                        style: Some(TextStyle {
                            font_families: vec!["Arial".to_string()],
                            font_size: Some(20.0),
                            color: [0, 0, 0, 255],
                            effect: Some(TextShaderEffect {
                                italic: true,
                                bold: true,
                            }),
                            stroke: None,
                            text_align: None,
                        }),
                        ..Default::default()
                    }),
                },
            );
            drop(scene);
            session.compact().unwrap();
        }

        let session = ProjectSession::open(&path).unwrap();
        let scene = session.scene.read();
        let page = scene.pages.get(&page_id).expect("page");
        let node = page.nodes.get(&node_id).expect("node");
        let NodeKind::Text(text) = &node.kind else {
            panic!("expected text node");
        };
        let effect = text
            .style
            .as_ref()
            .and_then(|style| style.effect)
            .expect("effect");
        assert!(effect.italic);
        assert!(effect.bold);
    }

    #[test]
    fn characters_persist_across_reopen() {
        use koharu_core::{Character, CharacterId};

        let (_tmp, path) = tmp_dir();
        let node = NodeId::new();
        let id = CharacterId::new();
        {
            let session = ProjectSession::create(&path, "chars").unwrap();
            assert!(session.characters().is_empty());
            session
                .update_characters(|book| {
                    book.set_characters(vec![Character {
                        id,
                        name: "ผู้บรรยาย".into(),
                        description: String::new(),
                        ..Default::default()
                    }]);
                    book.assign(&[node], Some(id))?;
                    Ok(())
                })
                .unwrap();
        }
        let session = ProjectSession::open(&path).unwrap();
        let book = session.characters();
        assert_eq!(book.speaker_of(node).map(|c| c.id), Some(id));
    }

    #[test]
    fn failed_character_update_leaves_book_unchanged() {
        let (_tmp, path) = tmp_dir();
        let session = ProjectSession::create(&path, "chars").unwrap();
        let bogus = koharu_core::CharacterId::new();
        let err = session.update_characters(|book| {
            book.assign(&[NodeId::new()], Some(bogus))?;
            Ok(())
        });
        assert!(err.is_err());
        assert!(session.characters().is_empty());
        assert!(!path.join(characters::CHARACTERS_FILE).exists());
    }

    #[test]
    fn exclusive_lock_prevents_second_open() {
        let (_tmp, path) = tmp_dir();
        let a = ProjectSession::create(&path, "test").unwrap();
        let err = ProjectSession::open(&path)
            .err()
            .expect("second open must fail");
        assert!(err.to_string().contains("already open"));
        drop(a);
    }
}
