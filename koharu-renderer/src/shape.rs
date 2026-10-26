use anyhow::Result;
use harfrust::{Direction, Feature, Script, ShapeOptions, ShaperData, Tag, UnicodeBuffer};
use icu_properties::{
    CodePointMapData,
    props::{GeneralCategory, Script as IcuScript},
};
use skrifa::raw::TableProvider;

use crate::font::Font;

/// A glyph with positioning information.
/// clone of harfrust::PositionedGlyph with glyph_id and cluster
#[derive(Debug, Clone)]
pub struct PositionedGlyph<'a> {
    /// The glyph ID as per the font's glyph set.
    pub glyph_id: u32,
    /// The cluster index in the original text that this glyph corresponds to.
    pub cluster: u32,
    /// Font used to shape this glyph.
    pub font: &'a Font,
    /// How much the line advances after drawing this glyph when setting text in
    /// horizontal direction.
    pub x_advance: f32,
    /// How much the line advances after drawing this glyph when setting text in
    /// vertical direction.
    pub y_advance: f32,
    /// How much the glyph moves on the X-axis before drawing it, this should
    /// not affect how much the line advances.
    pub x_offset: f32,
    /// How much the glyph moves on the Y-axis before drawing it, this should
    /// not affect how much the line advances.
    pub y_offset: f32,
}

/// A shaped run of text, containing positioned glyphs and overall advance.
#[derive(Debug, Clone)]
pub struct ShapedRun<'a> {
    pub glyphs: Vec<PositionedGlyph<'a>>,
    pub x_advance: f32,
    pub y_advance: f32,
}

/// Options for shaping text.
#[derive(Debug, Clone)]
pub struct ShapingOptions<'a> {
    pub direction: Direction,
    pub script: Option<Script>,
    pub font_size: f32,
    pub features: &'a [Feature],
}

/// Text shaper using HarfRust.
///
/// TODO: add shaper plan cache
#[derive(Debug, Clone, Default)]
pub struct TextShaper;

impl TextShaper {
    pub fn new() -> Self {
        Self
    }

    #[tracing::instrument(level = "debug", skip_all)]
    pub fn shape<'a>(
        &self,
        text: &str,
        font: &'a Font,
        options: &ShapingOptions,
    ) -> Result<ShapedRun<'a>> {
        let font_ref = font.harfrust()?;

        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(text);
        buffer.guess_segment_properties();
        buffer.set_direction(options.direction);
        if let Some(script) = options.script {
            buffer.set_script(script);
        }

        let shaper_data = ShaperData::new(&font_ref);
        let shaper = shaper_data.shaper(&font_ref).build();
        let output = shaper.shape(
            buffer,
            ShapeOptions::new()
                .features(options.features)
                .point_size(Some(options.font_size)),
        );

        let glyph_positions = output.glyph_positions();
        let glyph_infos = output.glyph_infos();

        // Scale factor to convert font units to pixels
        let upem = font.skrifa()?.head()?.units_per_em() as f32;
        let scale = options.font_size / upem;

        let mut positioned_glyphs = Vec::with_capacity(glyph_infos.len());
        for (info, pos) in glyph_infos.iter().zip(glyph_positions.iter()) {
            positioned_glyphs.push(PositionedGlyph {
                glyph_id: info.glyph_id,
                cluster: info.cluster,
                font,
                x_offset: (pos.x_offset as f32) * scale,
                y_offset: (pos.y_offset as f32) * scale,
                x_advance: (pos.x_advance as f32) * scale,
                y_advance: (pos.y_advance as f32) * scale,
            });
        }

        Ok(ShapedRun {
            glyphs: positioned_glyphs,
            x_advance: glyph_positions
                .iter()
                .map(|p| (p.x_advance as f32) * scale)
                .sum(),
            y_advance: glyph_positions
                .iter()
                .map(|p| (p.y_advance as f32) * scale)
                .sum(),
        })
    }
}

/// Length (in chars) of each cluster: a base char plus following marks.
fn cluster_lengths(chars: &[char]) -> Vec<usize> {
    let categories = CodePointMapData::<GeneralCategory>::new();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let mut end = i + 1;
        while end < chars.len() && is_mark(categories.get(chars[end])) {
            end += 1;
        }
        out.push(end - i);
        i = end;
    }
    out
}

fn is_mark(category: GeneralCategory) -> bool {
    matches!(
        category,
        GeneralCategory::NonspacingMark
            | GeneralCategory::SpacingMark
            | GeneralCategory::EnclosingMark
    )
}

/// Font index for every char of `text`, chosen per cluster (a base char plus
/// the combining marks after it) rather than per char.
///
/// Choosing per char let a Thai vowel or tone mark (e.g. U+0E4B in เดี๋ยว)
/// land in a different fallback font than its consonant; it was then shaped
/// in a separate run, could not attach to its base, and was drawn floating
/// away from it. Now the whole cluster uses the first font (primary, then
/// the last used, then the rest in order) that covers every char in it,
/// falling back to the base char's font when no font covers them all.
fn assign_cluster_fonts(text: &str, fonts: &[&Font]) -> Vec<usize> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::with_capacity(chars.len());
    let mut last_idx = 0;
    let mut i = 0;
    for len in cluster_lengths(&chars) {
        let cluster = &chars[i..i + len];
        let covers = |idx: usize| cluster.iter().all(|c| fonts[idx].has_glyph(*c));
        let idx = if covers(0) {
            0
        } else if last_idx != 0 && covers(last_idx) {
            last_idx
        } else if let Some(idx) = (0..fonts.len()).find(|&idx| covers(idx)) {
            idx
        } else {
            let base = cluster[0];
            if fonts[0].has_glyph(base) {
                0
            } else {
                fonts.iter().position(|f| f.has_glyph(base)).unwrap_or(0)
            }
        };
        last_idx = idx;
        out.extend(std::iter::repeat_n(idx, len));
        i += len;
    }
    out
}

#[tracing::instrument(level = "debug", skip_all)]
pub(crate) fn shape_script_runs<'a>(
    shaper: &TextShaper,
    text: &str,
    fonts: &[&'a Font],
    options: &ShapingOptions,
) -> Result<Vec<ShapedRun<'a>>> {
    if text.is_empty() || fonts.is_empty() {
        return Ok(Vec::new());
    }

    let script_map = CodePointMapData::<IcuScript>::new();
    let mut runs = Vec::new();

    let mut char_iter = text.char_indices().peekable();
    while let Some((start, ch)) = char_iter.next() {
        let mut script = script_map.get(ch);
        let mut end = start + ch.len_utf8();

        while let Some(&(next_start, next_ch)) = char_iter.peek() {
            let next_script = script_map.get(next_ch);
            if next_script == script
                || next_script == IcuScript::Common
                || next_script == IcuScript::Inherited
            {
                char_iter.next();
                end = next_start + next_ch.len_utf8();
            } else if script == IcuScript::Common || script == IcuScript::Inherited {
                script = next_script;
                char_iter.next();
                end = next_start + next_ch.len_utf8();
            } else {
                break;
            }
        }

        let script_run_text = &text[start..end];

        let char_fonts = assign_cluster_fonts(script_run_text, fonts);

        // Group characters into runs using the pre-calculated font indices.
        let mut font_item_iter = script_run_text.char_indices().enumerate().peekable();
        while let Some((i, (start_in_script, ch))) = font_item_iter.next() {
            let font_idx = char_fonts[i];
            let mut end_in_script = start_in_script + ch.len_utf8();

            while let Some(&(next_i, (next_start, next_ch))) = font_item_iter.peek() {
                if char_fonts[next_i] == font_idx {
                    font_item_iter.next();
                    end_in_script = next_start + next_ch.len_utf8();
                } else {
                    break;
                }
            }

            let font_run_text = &script_run_text[start_in_script..end_in_script];
            let absolute_start = start + start_in_script;
            let chosen_font = fonts[font_idx];

            let mut run_opts = options.clone();
            // Apply the detected script to this specific run.
            run_opts.script = match script {
                IcuScript::Arabic => Script::from_iso15924_tag(Tag::new(b"Arab")),
                IcuScript::Hebrew => Script::from_iso15924_tag(Tag::new(b"Hebr")),
                IcuScript::Syriac => Script::from_iso15924_tag(Tag::new(b"Syrc")),
                IcuScript::Thaana => Script::from_iso15924_tag(Tag::new(b"Thaa")),
                IcuScript::Nko => Script::from_iso15924_tag(Tag::new(b"Nkoo")),
                IcuScript::Adlam => Script::from_iso15924_tag(Tag::new(b"Adlm")),
                IcuScript::Thai => Script::from_iso15924_tag(Tag::new(b"Thai")),
                IcuScript::Han | IcuScript::Hiragana | IcuScript::Katakana => {
                    Script::from_iso15924_tag(Tag::new(b"Hani"))
                }
                _ => None,
            };

            let mut shaped = shaper.shape(font_run_text, chosen_font, &run_opts)?;
            for glyph in &mut shaped.glyphs {
                glyph.cluster += absolute_start as u32;
            }

            runs.push(shaped);
        }
    }

    Ok(runs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::FontBook;

    const PRIMARY_FAMILIES: &[&str] = &[
        "Arial",
        "Segoe UI",
        "Noto Sans",
        "DejaVu Sans",
        "Liberation Sans",
        "Yu Gothic",
        "MS Gothic",
        "Times New Roman",
    ];
    const FALLBACK_FAMILIES: &[&str] = &[
        "Segoe UI Symbol",
        "Segoe UI Emoji",
        "Noto Sans Symbols",
        "Noto Sans Symbols2",
        "Noto Color Emoji",
        "Apple Color Emoji",
        "Apple Symbols",
        "Symbola",
        "Arial Unicode MS",
    ];
    const SYMBOLS: &[char] = &[
        '\u{1F4A9}',
        '\u{1F642}',
        '\u{2665}',
        '\u{2605}',
        '\u{2713}',
        '\u{2603}',
        '\u{262F}',
        '\u{2691}',
        '\u{26A0}',
    ];

    fn query_font(book: &mut FontBook, name: &str) -> Option<Font> {
        let post_script_name = book
            .all_families()
            .into_iter()
            .find(|face| {
                face.post_script_name == name
                    || face
                        .families
                        .iter()
                        .any(|(family, _)| family.as_str() == name)
            })
            .map(|face| face.post_script_name)
            .filter(|post_script_name| !post_script_name.is_empty())?;
        book.query(&post_script_name).ok()
    }

    #[test]
    fn thai_marks_stay_in_their_consonant_cluster() {
        // เดี๋ยว = เ | ด+ี+๋ | ย | ว ; ก็นั่น = ก+็ | น+ั+่ | น
        let chars: Vec<char> = "เดี๋ยว".chars().collect();
        assert_eq!(cluster_lengths(&chars), vec![1, 3, 1, 1]);
        let chars: Vec<char> = "ก็นั่น".chars().collect();
        assert_eq!(cluster_lengths(&chars), vec![2, 3, 1]);
        // Leading marks without a base still form their own cluster.
        let chars: Vec<char> = "\u{0E48}a".chars().collect();
        assert_eq!(cluster_lengths(&chars), vec![1, 1]);
    }

    #[test]
    fn shape_segment_uses_fallback_font() -> Result<()> {
        let mut book = FontBook::new();
        let primary = PRIMARY_FAMILIES
            .iter()
            .find_map(|name| query_font(&mut book, name))
            .expect("no primary font available for fallback test");

        let mut chosen = None;
        for ch in SYMBOLS {
            if primary.has_glyph(*ch) {
                continue;
            }
            if let Some(fallback) = FALLBACK_FAMILIES.iter().find_map(|name| {
                let font = query_font(&mut book, name)?;
                font.has_glyph(*ch).then_some(font)
            }) {
                chosen = Some((fallback, *ch));
                break;
            }
        }

        let (fallback, symbol) = chosen.expect("no fallback font with missing primary glyph found");

        let text = format!("A{}!", symbol);
        let shaper = TextShaper::new();
        let opts = ShapingOptions {
            direction: harfrust::Direction::LeftToRight,
            script: None,
            font_size: 16.0,
            features: &[],
        };
        let runs = shape_script_runs(&shaper, &text, &[&primary, &fallback], &opts)?;

        // The mixed run should have been split into 3 segments: [A], [symbol], [!]
        assert!(
            runs.len() >= 2,
            "expected at least 2 runs for mixed text, got {}",
            runs.len()
        );
        let glyph_count: usize = runs.iter().map(|r| r.glyphs.len()).sum();
        assert!(glyph_count >= 3);

        Ok(())
    }

    #[test]
    fn shape_arabic_with_fallback_preserves_context() -> Result<()> {
        let mut book = FontBook::new();
        let primary = PRIMARY_FAMILIES
            .iter()
            .find_map(|name| query_font(&mut book, name))
            .expect("no primary font available");
        let fallback = FALLBACK_FAMILIES
            .iter()
            .find_map(|name| query_font(&mut book, name))
            .expect("no fallback font available");

        let shaper = TextShaper::new();
        let opts = ShapingOptions {
            direction: harfrust::Direction::RightToLeft,
            script: Some(harfrust::Script::from_iso15924_tag(harfrust::Tag::new(b"Arab")).unwrap()),
            font_size: 16.0,
            features: &[],
        };

        // "مرحبا" (Marhaba)
        let text = "مرحبا";
        let shaped = shape_script_runs(&shaper, text, &[&primary, &fallback], &opts)?;

        assert!(!shaped.is_empty());
        // Verify it used a consistent font for the whole run to ensure joining.
        let font_at_start = shaped[0].glyphs[0].font;
        assert!(
            shaped[0]
                .glyphs
                .iter()
                .all(|g| std::ptr::eq(g.font, font_at_start))
        );

        Ok(())
    }

    #[test]
    fn shape_rtl_multi_run_preserves_visual_order() -> Result<()> {
        let mut book = FontBook::new();
        let font = PRIMARY_FAMILIES
            .iter()
            .find_map(|name| query_font(&mut book, name))
            .expect("no font available");

        let shaper = TextShaper::new();
        let opts = ShapingOptions {
            direction: harfrust::Direction::RightToLeft,
            script: Some(harfrust::Script::from_iso15924_tag(harfrust::Tag::new(b"Arab")).unwrap()),
            font_size: 16.0,
            features: &[],
        };

        // Mixed script: Arabic + Hebrew (will be detected as separate script runs).
        let text = "مرحبا שלום";
        let shaped = shape_script_runs(&shaper, text, &[&font], &opts)?;

        // Now returns separate script runs in logical order.
        assert!(shaped.len() >= 2); // Arabic+space, Hebrew (or maybe space separate)
        assert!(shaped[0].glyphs[0].cluster < shaped[shaped.len() - 1].glyphs[0].cluster);

        Ok(())
    }
}
