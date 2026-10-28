//! Turn imported files into page images: plain images pass through, ZIP/CBZ
//! and RAR/CBR archives yield their images in natural order, PDFs are
//! rendered at 300 DPI. Extractors ported from upstream Koharu (79cc4d6c).

use std::io::Write;
use std::path::Path;

use anyhow::{Context as _, Result, bail};

mod color;
mod pdf;
mod rar;
mod zip;

pub use color::{decode_page, decode_srgb};

/// One page image before decoding.
#[derive(Debug)]
pub struct EncodedPage {
    pub name: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Raster,
    Zip,
    Rar,
    Pdf,
}

/// Image extensions accepted inside archives and as plain files.
pub(crate) fn is_raster_ext(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "png" | "jpg" | "jpeg" | "webp"
    )
}

fn kind_of(name: &str) -> Option<Kind> {
    let ext = Path::new(name).extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "zip" | "cbz" => Some(Kind::Zip),
        "rar" | "cbr" => Some(Kind::Rar),
        "pdf" => Some(Kind::Pdf),
        e if is_raster_ext(e) => Some(Kind::Raster),
        _ => None,
    }
}

/// Whether a file name is something page import understands.
pub fn is_supported(name: &str) -> bool {
    kind_of(name).is_some()
}

/// Whether the file is an archive / PDF (expands to several pages).
pub fn is_container(name: &str) -> bool {
    matches!(kind_of(name), Some(Kind::Zip | Kind::Rar | Kind::Pdf))
}

pub(crate) fn natural_sort(images: &mut [EncodedPage]) {
    images.sort_by(|a, b| natord::compare(&a.name, &b.name));
}

/// Pages from a file on disk.
pub fn expand_path(path: &Path) -> Result<Vec<EncodedPage>> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "page".to_owned());
    match kind_of(&name) {
        Some(Kind::Zip) => zip::extract(path),
        Some(Kind::Rar) => rar::extract(path),
        Some(Kind::Pdf) => pdf::render(path),
        // Unknown extensions are tried as images (the decoder decides).
        Some(Kind::Raster) | None => Ok(vec![EncodedPage {
            bytes: std::fs::read(path)
                .with_context(|| format!("failed to read {}", path.display()))?,
            name,
        }]),
    }
}

/// Pages from an uploaded file (`name` is its file name). Archives / PDFs
/// are written to a temporary file first, since the readers work on paths.
pub fn expand_bytes(name: &str, bytes: Vec<u8>) -> Result<Vec<EncodedPage>> {
    if !is_container(name) {
        return Ok(vec![EncodedPage {
            name: name.to_owned(),
            bytes,
        }]);
    }
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin");
    let dir = tempfile::tempdir().context("create temporary folder for import")?;
    let stem = Path::new(name)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "import".to_owned());
    let safe: String = stem
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let path = dir.path().join(format!("{safe}.{ext}"));
    std::fs::File::create(&path)
        .and_then(|mut f| f.write_all(&bytes))
        .with_context(|| format!("write temporary copy of {name}"))?;
    let pages = expand_path(&path)?;
    if pages.is_empty() {
        bail!("{name} contains no pages");
    }
    Ok(pages)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_formats() {
        assert!(is_supported("a.PNG") && !is_container("a.png"));
        assert!(is_container("vol1.cbz") && is_container("x.CBR") && is_container("b.pdf"));
        assert!(!is_supported("notes.txt"));
    }

    #[test]
    fn plain_images_pass_through() {
        let pages = expand_bytes("p1.png", vec![1, 2, 3]).unwrap();
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].name, "p1.png");
        assert_eq!(pages[0].bytes, vec![1, 2, 3]);
    }
}
