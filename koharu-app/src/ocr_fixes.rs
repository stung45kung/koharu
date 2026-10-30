//! Load/save `.khrproj/ocr_fixes.toml` (OCR text replaced by the LLM OCR check).

use std::io::Write;

use anyhow::{Context, Result};
use atomicwrites::{AtomicFile, OverwriteBehavior};
use camino::Utf8Path;
use koharu_core::OcrFixes;

pub const OCR_FIXES_FILE: &str = "ocr_fixes.toml";

/// Missing file → no fixes. An unparsable file is moved aside (never
/// silently overwritten) and no fixes are used.
pub fn load(dir: &Utf8Path) -> OcrFixes {
    let path = dir.join(OCR_FIXES_FILE);
    let text = match std::fs::read_to_string(path.as_std_path()) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return OcrFixes::default(),
        Err(e) => {
            tracing::warn!(path = %path, "failed to read OCR fixes: {e}");
            return OcrFixes::default();
        }
    };
    match toml::from_str(&text) {
        Ok(g) => g,
        Err(e) => {
            let backup = dir.join(format!(
                "{OCR_FIXES_FILE}.broken-{}",
                chrono::Utc::now().format("%Y%m%d%H%M%S")
            ));
            tracing::warn!(path = %path, backup = %backup, "invalid OCR fixes: {e}");
            if let Err(e) = std::fs::rename(path.as_std_path(), backup.as_std_path()) {
                tracing::warn!("failed to move invalid OCR fixes aside: {e}");
            }
            OcrFixes::default()
        }
    }
}

pub fn save(dir: &Utf8Path, fixes: &OcrFixes) -> Result<()> {
    let text = toml::to_string_pretty(fixes).context("encode ocr_fixes.toml")?;
    AtomicFile::new(
        dir.join(OCR_FIXES_FILE).as_std_path(),
        OverwriteBehavior::AllowOverwrite,
    )
    .write(|f| f.write_all(text.as_bytes()))
    .context("write ocr_fixes.toml atomically")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use koharu_core::NodeId;

    #[test]
    fn round_trips_and_defaults_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        assert_eq!(load(&dir), OcrFixes::default());
        let mut fixes = OcrFixes::default();
        fixes.record(NodeId::new(), "あリがとう".into());
        save(&dir, &fixes).unwrap();
        assert_eq!(load(&dir), fixes);
    }
}
