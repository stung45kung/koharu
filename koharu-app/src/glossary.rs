//! Load/save `.khrproj/glossary.toml` (project glossary).

use std::io::Write;

use anyhow::{Context, Result};
use atomicwrites::{AtomicFile, OverwriteBehavior};
use camino::Utf8Path;
use koharu_core::Glossary;

pub const GLOSSARY_FILE: &str = "glossary.toml";

/// Missing file → empty glossary. An unparsable file is moved aside (never
/// silently overwritten) and an empty glossary is used.
pub fn load(dir: &Utf8Path) -> Glossary {
    let path = dir.join(GLOSSARY_FILE);
    let text = match std::fs::read_to_string(path.as_std_path()) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Glossary::default(),
        Err(e) => {
            tracing::warn!(path = %path, "failed to read glossary: {e}");
            return Glossary::default();
        }
    };
    match toml::from_str(&text) {
        Ok(g) => g,
        Err(e) => {
            let backup = dir.join(format!(
                "{GLOSSARY_FILE}.broken-{}",
                chrono::Utc::now().format("%Y%m%d%H%M%S")
            ));
            tracing::warn!(path = %path, backup = %backup, "invalid glossary: {e}");
            if let Err(e) = std::fs::rename(path.as_std_path(), backup.as_std_path()) {
                tracing::warn!("failed to move invalid glossary aside: {e}");
            }
            Glossary::default()
        }
    }
}

pub fn save(dir: &Utf8Path, glossary: &Glossary) -> Result<()> {
    let text = toml::to_string_pretty(glossary).context("encode glossary.toml")?;
    AtomicFile::new(
        dir.join(GLOSSARY_FILE).as_std_path(),
        OverwriteBehavior::AllowOverwrite,
    )
    .write(|f| f.write_all(text.as_bytes()))
    .context("write glossary.toml atomically")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use koharu_core::GlossaryEntry;

    #[test]
    fn round_trips_and_defaults_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        assert_eq!(load(&dir), Glossary::default());
        let g = Glossary {
            entries: vec![GlossaryEntry {
                source: "すい".into(),
                target: "ซุย".into(),
                note: "nickname".into(),
            }],
        };
        save(&dir, &g).unwrap();
        assert_eq!(load(&dir), g);
    }
}
