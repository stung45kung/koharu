//! Load/save `.khrproj/typesetting.toml` (project default text size).
//! Kept outside `scene.bin` for the same reason as `characters.toml`.

use std::io::Write;

use anyhow::{Context, Result};
use atomicwrites::{AtomicFile, OverwriteBehavior};
use camino::Utf8Path;
use koharu_core::TypesettingDefaults;

pub const TYPESETTING_FILE: &str = "typesetting.toml";

/// Missing or unreadable file → no defaults (rendering behaves as before).
pub fn load(dir: &Utf8Path) -> TypesettingDefaults {
    let path = dir.join(TYPESETTING_FILE);
    match std::fs::read_to_string(path.as_std_path()) {
        Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
            tracing::warn!(path = %path, "invalid typesetting file, ignoring: {e}");
            TypesettingDefaults::default()
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => TypesettingDefaults::default(),
        Err(e) => {
            tracing::warn!(path = %path, "failed to read typesetting file: {e}");
            TypesettingDefaults::default()
        }
    }
}

pub fn save(dir: &Utf8Path, defaults: &TypesettingDefaults) -> Result<()> {
    let text = toml::to_string_pretty(defaults).context("encode typesetting.toml")?;
    AtomicFile::new(
        dir.join(TYPESETTING_FILE).as_std_path(),
        OverwriteBehavior::AllowOverwrite,
    )
    .write(|f| f.write_all(text.as_bytes()))
    .context("write typesetting.toml atomically")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;

    #[test]
    fn round_trips_and_defaults_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        assert_eq!(load(&dir), TypesettingDefaults::default());

        let d = TypesettingDefaults {
            font_size_px: Some(28.5),
            reference_page_height: Some(1518),
            color: Some([255, 255, 255, 255]),
            shrink_to_fit: true,
            stroke: Some(koharu_core::StrokeDefaults {
                enabled: true,
                color: Some([0, 0, 0, 255]),
                width_px: Some(3.0),
            }),
            thai_lstm_line_breaking: true,
        };
        save(&dir, &d).unwrap();
        assert_eq!(load(&dir), d);

        save(&dir, &TypesettingDefaults::default()).unwrap();
        assert_eq!(load(&dir), TypesettingDefaults::default());
    }
}
