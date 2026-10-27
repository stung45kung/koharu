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
    /// Text colour (RGBA) for blocks without their own style. Takes
    /// precedence over the colour detected from the source lettering.
    #[serde(default)]
    pub color: Option<[u8; 4]>,
    /// `false` (default): blocks without their own size use exactly
    /// `font_size_px` (scaled to the page), even if the text overflows its
    /// box. `true`: the size is a ceiling and long text shrinks to fit.
    #[serde(default)]
    pub shrink_to_fit: bool,
    /// Outline for blocks without an outline of their own.
    #[serde(default)]
    pub stroke: Option<StrokeDefaults>,
    /// Find Thai words for line breaking with ICU's LSTM model instead of
    /// its word dictionary (the default).
    #[serde(default)]
    pub thai_lstm_line_breaking: bool,
}

/// A default text outline. Unset fields fall back to automatic values
/// (colour contrasting with the text, width from the detected lettering or
/// the text size).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StrokeDefaults {
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub color: Option<[u8; 4]>,
    /// Width in px on a page `reference_page_height` px tall (scaled to
    /// other page heights like the default size).
    #[serde(default)]
    pub width_px: Option<f32>,
}

fn yes() -> bool {
    true
}

impl Default for StrokeDefaults {
    fn default() -> Self {
        Self {
            enabled: true,
            color: None,
            width_px: None,
        }
    }
}

impl TypesettingDefaults {
    /// Default text size for a page `page_height` px tall, if one is set.
    pub fn font_size_for_page(&self, page_height: u32) -> Option<f32> {
        self.scale_size_for_page(self.font_size_px?, page_height)
    }

    /// Scale a size tuned on the reference page height to a page
    /// `page_height` px tall (used for character sizes too).
    pub fn scale_size_for_page(&self, size: f32, page_height: u32) -> Option<f32> {
        let size = Some(size).filter(|s| s.is_finite() && *s > 0.0)?;
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
            ..Default::default()
        };
        assert_eq!(bad.font_size_for_page(1500), None);
    }

    #[test]
    fn character_sizes_scale_like_the_default() {
        let d = TypesettingDefaults {
            font_size_px: None,
            reference_page_height: Some(1000),
            ..Default::default()
        };
        assert_eq!(d.scale_size_for_page(20.0, 2000), Some(40.0));
        assert_eq!(d.scale_size_for_page(0.0, 2000), None);
        assert_eq!(
            TypesettingDefaults::default().scale_size_for_page(20.0, 2000),
            Some(20.0)
        );
    }

    #[test]
    fn size_scales_with_page_height() {
        let d = TypesettingDefaults {
            font_size_px: Some(30.0),
            reference_page_height: Some(1500),
            ..Default::default()
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
            ..Default::default()
        };
        assert_eq!(d.font_size_for_page(6000), Some(24.0));
    }
}
