//! Load/save `.khrproj/characters.toml` (character profiles + speaker map).
//!
//! Kept outside `scene.bin` on purpose: the postcard snapshot and the
//! history log are positional binary formats, and adding a field there would
//! make every existing project unreadable. A side file is ignored by older
//! Koharu builds and is included in `.khr` exports automatically.

use std::io::Write;

use anyhow::{Context, Result};
use atomicwrites::{AtomicFile, OverwriteBehavior};
use camino::Utf8Path;
use koharu_core::CharacterBook;

pub const CHARACTERS_FILE: &str = "characters.toml";

/// Read the book from `dir`. A missing file is an empty book. A file that
/// fails to parse is moved aside (never silently overwritten) and an empty
/// book is returned so the project still opens.
pub fn load(dir: &Utf8Path) -> CharacterBook {
    let path = dir.join(CHARACTERS_FILE);
    let text = match std::fs::read_to_string(path.as_std_path()) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return CharacterBook::default(),
        Err(e) => {
            tracing::warn!(path = %path, "failed to read characters file: {e}");
            return CharacterBook::default();
        }
    };
    match toml::from_str::<CharacterBook>(&text) {
        Ok(book) => book,
        Err(e) => {
            let backup = dir.join(format!(
                "{CHARACTERS_FILE}.broken-{}",
                chrono::Utc::now().format("%Y%m%d%H%M%S")
            ));
            tracing::warn!(path = %path, backup = %backup, "invalid characters file: {e}");
            if let Err(e) = std::fs::rename(path.as_std_path(), backup.as_std_path()) {
                tracing::warn!("failed to move invalid characters file aside: {e}");
            }
            CharacterBook::default()
        }
    }
}

/// Atomically write the book to `dir`.
pub fn save(dir: &Utf8Path, book: &CharacterBook) -> Result<()> {
    let text = toml::to_string_pretty(book).context("encode characters.toml")?;
    AtomicFile::new(
        dir.join(CHARACTERS_FILE).as_std_path(),
        OverwriteBehavior::AllowOverwrite,
    )
    .write(|f| f.write_all(text.as_bytes()))
    .context("write characters.toml atomically")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use koharu_core::{Character, CharacterId, NodeId};
    use uuid::Uuid;

    fn tmp() -> (tempfile::TempDir, Utf8PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        (dir, path)
    }

    #[test]
    fn missing_file_is_empty_book() {
        let (_t, dir) = tmp();
        assert!(load(&dir).is_empty());
    }

    #[test]
    fn round_trips_through_toml() {
        let (_t, dir) = tmp();
        let mut book = CharacterBook::default();
        let id = CharacterId(Uuid::from_u128(7));
        book.set_characters(vec![Character {
            id,
            name: "อากาซูกิน".into(),
            description: "ใช้ ข้า/เจ้า\nลงท้าย ...เถิด".into(),
            ..Default::default()
        }]);
        let node = NodeId(Uuid::from_u128(42));
        book.assign(&[node], Some(id)).unwrap();

        save(&dir, &book).unwrap();
        let loaded = load(&dir);
        assert_eq!(loaded, book);
        assert_eq!(
            loaded.speaker_of(node).map(|c| c.name.as_str()),
            Some("อากาซูกิน")
        );
    }

    #[test]
    fn invalid_file_is_moved_aside_not_lost() {
        let (_t, dir) = tmp();
        std::fs::write(dir.join(CHARACTERS_FILE).as_std_path(), "not = [valid").unwrap();
        assert!(load(&dir).is_empty());
        assert!(!dir.join(CHARACTERS_FILE).exists());
        let backups: Vec<_> = std::fs::read_dir(dir.as_std_path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".broken-"))
            .collect();
        assert_eq!(backups.len(), 1);
    }
}
