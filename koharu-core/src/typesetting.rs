//! Per-project typesetting defaults that are not part of the scene format.
//!
//! Stored in `.khrproj/typesetting.toml` (like `characters.toml`) so older
//! Koharu builds can still open the project. The project's default *font*
//! lives in `ProjectMeta.style.default_font` (already in the scene).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TypesettingDefaults {
    /// Preferred text size in px, as measured on a page that is
    /// `reference_page_height` px tall. Blocks without their own size use
    /// this, shrinking only when the text would not fit its bubble.
    #[serde(default)]
    pub font_size_px: Option<f32>,
    /// Height of the page the size was tuned on. Pages of a different
    /// resolution get a proportionally scaled size.
    #[serde(default)]
    pub reference_page_height: Option<u32>,
}

impl TypesettingDefaults {
    /// Default text size for a page `page_height` px tall, if one is set.
    pub fn font_size_for_page(&self, page_height: u32) -> Option<f32> {
        let size = self.font_size_px.filter(|s| s.is_finite() && *s > 0.0)?;
        match self.reference_page_height {
            Some(reference) if reference > 0 && page_height > 0 => {
                Some((size * page_height as f32 / reference as f32).max(1.0))
            }
            _ => Some(size),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_size_gives_none() {
        assert_eq!(
            TypesettingDefaults::default().font_size_for_page(1500),
            None
        );
        let bad = TypesettingDefaults {
            font_size_px: Some(f32::NAN),
            reference_page_height: None,
        };
        assert_eq!(bad.font_size_for_page(1500), None);
    }

    #[test]
    fn size_scales_with_page_height() {
        let d = TypesettingDefaults {
            font_size_px: Some(30.0),
            reference_page_height: Some(1500),
        };
        assert_eq!(d.font_size_for_page(1500), Some(30.0));
        assert_eq!(d.font_size_for_page(3000), Some(60.0));
        assert_eq!(d.font_size_for_page(750), Some(15.0));
    }

    #[test]
    fn size_without_reference_is_used_as_is() {
        let d = TypesettingDefaults {
            font_size_px: Some(24.0),
            reference_page_height: None,
        };
        assert_eq!(d.font_size_for_page(6000), Some(24.0));
    }
}
