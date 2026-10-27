//! Koharu text renderer.
//!
//! Owns the font book, symbol fallbacks, and Google Fonts service. Exposes
//! [`Renderer::render_page`], which rasterises each text block's translation
//! into an RGBA sprite and composites them onto the inpainted plane.
//!
//! Pure output: the pipeline engine ([`crate::pipeline::engines::renderer`])
//! takes a `RenderOutput` and translates sprites + final composite into ops.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result};
use image::{DynamicImage, GrayImage, RgbaImage, imageops};
use koharu_core::{
    FontFaceInfo, FontPrediction, FontSource, NodeId, TextDirection, TextShaderEffect,
    TextStrokeStyle, TextStyle, Transform,
};

use koharu_renderer::{
    TextAlign as RendererTextAlign, TextShaderEffect as RendererEffect,
    font::{FaceInfo, Font, FontBook},
    layout::{LayoutRun, TextLayout, WritingMode},
    renderer::{RasterOptions, RenderOptions, RenderStrokeOptions, TinySkiaRenderer},
    text::{
        latin::{BubbleIndex, LayoutBox},
        script::{font_families_for_text, writing_mode_for_block},
    },
    types::{RenderBlock, TextDirection as RendererTextDirection},
};

use crate::google_fonts::GoogleFontService;

// ---------------------------------------------------------------------------
// Inputs / outputs
// ---------------------------------------------------------------------------

/// Per-block input (immutable snapshot of a scene text node).
#[derive(Debug, Clone)]
pub struct RenderBlockInput {
    pub node_id: NodeId,
    pub transform: Transform,
    pub translation: String,
    pub style: Option<TextStyle>,
    pub font_prediction: Option<FontPrediction>,
    pub source_direction: Option<TextDirection>,
    pub rendered_direction: Option<TextDirection>,
    pub lock_layout_box: bool,
    /// Font of the block's speaker (character profile), used when the block
    /// has no font of its own. Wins over the document font.
    pub speaker_font: Option<String>,
    /// Colour of the block's speaker, used when the block has no style of
    /// its own. Wins over the project default colour.
    pub speaker_color: Option<[u8; 4]>,
    /// Speaker text size (already scaled to this page), used when the block
    /// has no size of its own. Wins over the project default size.
    pub speaker_font_size: Option<f32>,
    /// Speaker faux bold / italic, used when the block has no effect set.
    pub speaker_effect: Option<TextShaderEffect>,
    /// Speaker alignment, used when the block has no alignment set.
    pub speaker_align: Option<koharu_core::TextAlign>,
    /// Default outline for this block (speaker's over the project's, width
    /// already scaled to the page), used when the block has no outline set.
    pub default_stroke: Option<koharu_core::StrokeDefaults>,
}

/// Document-level render options (shared across all blocks).
#[derive(Debug, Clone, Default)]
pub struct PageRenderOptions {
    pub shader_effect: TextShaderEffect,
    pub shader_stroke: Option<TextStrokeStyle>,
    pub document_font: Option<String>,
    pub target_language: Option<String>,
    pub raster: RasterOptions,
    /// Target text size for blocks without their own size: used when the
    /// text fits, otherwise shrunk like auto-size. `None` = pure auto-size.
    pub default_font_size: Option<f32>,
    /// With a `default_font_size`: shrink long text to fit (`true`) or keep
    /// the size fixed (`false`).
    pub default_font_size_shrinks: bool,
    /// Project text colour for blocks without their own style.
    pub default_color: Option<[u8; 4]>,
    /// How Thai words are found for line breaking.
    pub thai_word_breaking: koharu_renderer::segment::ThaiWordBreaking,
    /// Words never split across lines (glossary renderings, character names).
    pub protected_words: Vec<String>,
}

/// Per-block sprite output. `transform` becomes `TextData.sprite_transform`
/// when the renderer expanded the layout beyond the original bubble.
pub struct RenderedBlock {
    pub node_id: NodeId,
    pub sprite: DynamicImage,
    pub rendered_direction: TextDirection,
    pub expanded_transform: Option<Transform>,
}

/// Result of rendering a whole page.
pub struct RenderOutput {
    pub final_render: DynamicImage,
    pub blocks: Vec<RenderedBlock>,
}

// ---------------------------------------------------------------------------
// Renderer
// ---------------------------------------------------------------------------

pub struct Renderer {
    fontbook: Arc<Mutex<FontBook>>,
    renderer: TinySkiaRenderer,
    symbol_fallbacks: Vec<Font>,
    pub google_fonts: Arc<GoogleFontService>,
}

impl Renderer {
    pub fn new() -> Result<Self> {
        let mut fontbook = FontBook::new();
        let symbol_fallbacks = load_symbol_fallbacks(&mut fontbook);
        let app_data_root = koharu_runtime::default_app_data_root();
        let google_fonts = Arc::new(
            GoogleFontService::new(&app_data_root)
                .context("failed to initialize Google Fonts service")?,
        );
        Ok(Self {
            fontbook: Arc::new(Mutex::new(fontbook)),
            renderer: TinySkiaRenderer::new()?,
            symbol_fallbacks,
            google_fonts,
        })
    }

    /// List system + cached Google Fonts for the API.
    pub fn available_fonts(&self) -> Result<Vec<FontFaceInfo>> {
        let fontbook = self
            .fontbook
            .lock()
            .map_err(|_| anyhow::anyhow!("failed to lock fontbook"))?;
        let mut fonts = fontbook
            .all_families()
            .into_iter()
            .filter(|face| !face.post_script_name.is_empty())
            .map(|face| {
                let family_name = face
                    .families
                    .first()
                    .map(|(family, _)| family.clone())
                    .unwrap_or_else(|| face.post_script_name.clone());
                FontFaceInfo {
                    family_name,
                    post_script_name: face.post_script_name,
                    source: FontSource::System,
                    category: None,
                    cached: true,
                }
            })
            .collect::<Vec<_>>();
        let catalog = self.google_fonts.catalog();
        for entry in &catalog.fonts {
            for variant in &entry.variants {
                // Unique PS name for Google Fonts to identify the specific weight/style
                let post_script_name = format!(
                    "{}:{}{}",
                    entry.family,
                    variant.weight,
                    if variant.style == "italic" { "i" } else { "" }
                );

                fonts.push(FontFaceInfo {
                    family_name: entry.family.clone(),
                    post_script_name,
                    source: FontSource::Google,
                    category: Some(entry.category.clone()),
                    cached: self.google_fonts.is_variant_cached(&entry.family, variant),
                });
            }
        }
        fonts.sort();
        Ok(fonts)
    }

    /// Render every block's translation, composite onto `inpainted`, return
    /// the full page + per-block sprites. Blocks with an empty translation
    /// are skipped (they appear as holes in the composite, falling through to
    /// the inpainted plane).
    #[allow(clippy::too_many_arguments)]
    #[tracing::instrument(level = "info", skip_all, fields(blocks = blocks.len()))]
    pub fn render_page(
        &self,
        inpainted: &DynamicImage,
        brush_layer: Option<&DynamicImage>,
        bubble_mask: Option<&DynamicImage>,
        image_width: u32,
        image_height: u32,
        blocks: &[RenderBlockInput],
        opts: &PageRenderOptions,
    ) -> Result<RenderOutput> {
        let min_font = min_font_size_for_image(image_width, image_height);
        // Build the bubble index once per page. The mask encodes each
        // detected bubble as a distinct grayscale ID; the index scans
        // once to record per-ID bboxes and then answers seed→bbox
        // lookups in O(seed_area).
        let bubble_index: Option<BubbleIndex> = bubble_mask.map(|m| BubbleIndex::new(m.to_luma8()));
        let mut layout_boxes = resolve_layout_boxes(blocks, bubble_index.as_ref());
        widen_narrow_boxes(blocks, &mut layout_boxes, image_width as f32);
        let bubble_mask = bubble_index.as_ref().map(BubbleIndex::mask);

        let mut rendered_blocks = Vec::with_capacity(blocks.len());
        for (block, layout_box) in blocks.iter().zip(layout_boxes.iter().copied()) {
            match self.render_one(
                block,
                layout_box,
                bubble_mask,
                &opts.shader_effect,
                &opts.shader_stroke,
                opts.document_font.as_deref(),
                opts.target_language.as_deref(),
                opts.raster,
                min_font,
                opts.default_font_size,
                opts.default_font_size_shrinks,
                opts.default_color,
                opts.thai_word_breaking,
                &opts.protected_words,
            ) {
                Ok(Some(out)) => rendered_blocks.push(out),
                Ok(None) => {}
                Err(e) => tracing::warn!(node = %block.node_id, "render failed: {e:#}"),
            }
        }

        // Compose the final page: inpainted → brush → per-block sprites.
        let mut canvas = inpainted.to_rgba8();
        if let Some(brush) = brush_layer {
            imageops::overlay(&mut canvas, &brush.to_rgba8(), 0, 0);
        }
        for out in &rendered_blocks {
            let (x, y) = placement_origin(find_input(blocks, out.node_id), &out.expanded_transform);
            imageops::overlay(&mut canvas, &out.sprite.to_rgba8(), x as i64, y as i64);
        }
        Ok(RenderOutput {
            final_render: DynamicImage::ImageRgba8(canvas),
            blocks: rendered_blocks,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn render_one(
        &self,
        block: &RenderBlockInput,
        resolved_box: ResolvedLayoutBox,
        bubble_mask: Option<&GrayImage>,
        effect: &TextShaderEffect,
        global_stroke: &Option<TextStrokeStyle>,
        document_font: Option<&str>,
        target_language: Option<&str>,
        raster: RasterOptions,
        min_font_size: f32,
        default_font_size: Option<f32>,
        default_font_size_shrinks: bool,
        default_color: Option<[u8; 4]>,
        thai_word_breaking: koharu_renderer::segment::ThaiWordBreaking,
        protected_words: &[String],
    ) -> Result<Option<RenderedBlock>> {
        let translation = block.translation.trim();
        if translation.is_empty() {
            return Ok(None);
        }

        let layout_source = layout_source_from_input(block, translation);

        let mut style = block.style.clone().unwrap_or_else(|| TextStyle {
            font_families: Vec::new(),
            font_size: None,
            color: [0, 0, 0, 255],
            effect: None,
            stroke: None,
            text_align: None,
        });
        if style.font_families.is_empty()
            && let Some(font) = block
                .speaker_font
                .as_deref()
                .filter(|f| !f.trim().is_empty())
                .or(document_font)
        {
            style.font_families.push(font.to_string());
        }
        apply_default_font_families(&mut style.font_families, translation);

        let font = self.select_font(&style)?;
        let block_effect = style.effect.or(block.speaker_effect).unwrap_or(*effect);
        let color = resolve_text_color(
            block.style.as_ref(),
            &style,
            block.font_prediction.as_ref(),
            block.speaker_color,
            default_color,
        );

        let writing_mode = writing_mode_for_block(&layout_source);
        // Translations default to centre alignment inside a bubble — each
        // line sits centred above/below the others, matching manga
        // typesetting convention. Explicit `style.text_align` wins if set.
        let align = style
            .text_align
            .or(block.speaker_align)
            .map(core_align_to_renderer)
            .unwrap_or(RendererTextAlign::Center);
        let layout_box = resolved_box.layout_box;

        let mut layout_builder = TextLayout::new(&font, None)
            .with_fallback_fonts(&self.symbol_fallbacks)
            .with_writing_mode(writing_mode)
            .with_alignment(align)
            .with_thai_word_breaking(thai_word_breaking)
            .with_protected_words(protected_words);
        if let Some(target_language) = target_language {
            layout_builder = layout_builder.with_hyphenation_language_tag(target_language);
        }
        // The speaker's size takes the project default's place.
        let default_font_size = block.speaker_font_size.or(default_font_size);
        let explicit_size = fixed_font_size(
            style.font_size,
            default_font_size,
            default_font_size_shrinks,
        );
        let (min_font_size, max_font) =
            font_size_bounds(explicit_size, default_font_size, layout_box, min_font_size);
        let mut render_candidate = |layout: &LayoutRun<'_>| -> Result<RenderedTextCandidate> {
            let resolved_stroke = resolve_stroke_style(
                block.font_prediction.as_ref(),
                style.stroke.as_ref(),
                global_stroke.as_ref(),
                layout.font_size,
                color,
                block.default_stroke.as_ref(),
            );

            let rendered = self.renderer.render(
                layout,
                writing_mode,
                &RenderOptions {
                    font_size: layout.font_size,
                    color,
                    effect: shader_core_to_renderer(block_effect),
                    stroke: resolved_stroke,
                    raster,
                    ..Default::default()
                },
            )?;
            let transform = centred_sprite_transform(
                layout_box,
                rendered.width(),
                rendered.height(),
                block.transform.rotation_deg,
            );
            Ok(RenderedTextCandidate {
                image: rendered,
                transform,
            })
        };

        if let Some((mask, bubble_id)) = bubble_mask.zip(resolved_box.bubble_id) {
            let candidate = fit_rendered_with_mask_collision(
                &layout_builder,
                translation,
                layout_box,
                explicit_size,
                min_font_size,
                max_font,
                mask,
                bubble_id,
                &mut render_candidate,
            )?;
            let candidate = rotate_candidate(candidate, block.transform.rotation_deg);
            return Ok(Some(RenderedBlock {
                node_id: block.node_id,
                sprite: DynamicImage::ImageRgba8(candidate.image),
                rendered_direction: rendered_direction_for_writing_mode(writing_mode),
                expanded_transform: Some(candidate.transform),
            }));
        }

        let layout = fit_font_size(
            &layout_builder,
            translation,
            layout_box.width,
            layout_box.height,
            explicit_size,
            min_font_size,
            max_font,
        )?;

        let candidate = render_candidate(&layout)?;
        let candidate = rotate_candidate(candidate, block.transform.rotation_deg);

        Ok(Some(RenderedBlock {
            node_id: block.node_id,
            sprite: DynamicImage::ImageRgba8(candidate.image),
            rendered_direction: rendered_direction_for_writing_mode(writing_mode),
            expanded_transform: Some(candidate.transform),
        }))
    }

    /// Resolve a set of font family candidates into a single PostScript name.
    pub fn resolve_post_script_name(
        &self,
        style: &TextStyle,
        text: Option<&str>,
    ) -> Result<String> {
        let fontbook = self
            .fontbook
            .lock()
            .map_err(|_| anyhow::anyhow!("failed to lock fontbook"))?;
        let faces = fontbook.all_families();

        let mut families = style.font_families.clone();
        if families.is_empty()
            && let Some(text) = text
        {
            tracing::debug!(
                "Families empty, applying script-based default font families for text: {}",
                text
            );
            apply_default_font_families(&mut families, text);
        }
        if families.is_empty() {
            families.push("ArialMT".to_string());
        }

        for candidate in &families {
            tracing::debug!("Attempting to resolve font candidate: {}", candidate);
            // 1. Exact PS name
            if let Some(face) = faces.iter().find(|f| f.post_script_name == *candidate) {
                tracing::debug!("Resolved via exact PS name: {}", face.post_script_name);
                return Ok(face.post_script_name.clone());
            }

            // 2. Google Font variant
            let (family, weight, style_str) = crate::google_fonts::parse_variant_query(candidate);
            if candidate.contains(':')
                && self
                    .google_fonts
                    .read_cached_variant(family, weight, style_str)
                    .map(|opt| opt.is_some())
                    .unwrap_or(false)
            {
                tracing::debug!("Resolved via Google Font variant: {}", candidate);
                return Ok(candidate.clone());
            }

            // 3. Fuzzy family name
            if let Some(psn) = face_post_script_name(&faces, candidate) {
                tracing::debug!("Resolved via fuzzy family name: {}", psn);
                return Ok(psn);
            }

            // 4. Base Google Font
            if self
                .google_fonts
                .read_cached_file(candidate)
                .map(|opt| opt.is_some())
                .unwrap_or(false)
            {
                tracing::debug!("Resolved via base Google Font: {}", candidate);
                return Ok(candidate.clone());
            }
        }

        tracing::warn!(?families, "font resolution failed, falling back to ArialMT");
        Ok("ArialMT".to_string())
    }

    fn select_font(&self, style: &TextStyle) -> Result<Font> {
        let mut fontbook = self
            .fontbook
            .lock()
            .map_err(|_| anyhow::anyhow!("failed to lock fontbook"))?;
        for candidate in &style.font_families {
            let faces = fontbook.all_families();

            // 1. Try exact PostScript name match first (most reliable for variants)
            if let Some(face) = faces.iter().find(|f| f.post_script_name == *candidate) {
                return fontbook.load_font(face.id);
            }

            // 2. Check if it's a Google Font variant (Family:WeightStyle)
            let (family, weight, style_str) = crate::google_fonts::parse_variant_query(candidate);
            if candidate.contains(':')
                && let Some(data) = self
                    .google_fonts
                    .read_cached_variant(family, weight, style_str)?
            {
                let mut font = fontbook.load_from_bytes(data)?;

                // Explicitly set the weight and style for variable font instancing
                font.weight = weight;
                font.style = style_str.to_string();

                return Ok(font);
            }

            // 3. Try fuzzy family name match
            if let Some(psn) = face_post_script_name(&faces, candidate) {
                return fontbook.query(&psn);
            }

            // 4. Try base Google Font file
            if let Some(data) = self.google_fonts.read_cached_file(candidate)? {
                return fontbook.load_from_bytes(data);
            }
        }
        Err(anyhow::anyhow!(
            "no font found for candidates: {:?}",
            style.font_families
        ))
    }
}

// ---------------------------------------------------------------------------
// Helpers: font sizing
// ---------------------------------------------------------------------------

const MASK_COLLISION_ALPHA_THRESHOLD: u8 = 8;
const FIT_EPSILON: f32 = 0.5;

struct RenderedTextCandidate {
    image: RgbaImage,
    transform: Transform,
}

struct MaskCollisionAttempt {
    candidate: RenderedTextCandidate,
    valid: bool,
}

/// Size to render at without searching: the block's own size, else the
/// project default when it is configured as fixed. `None` → fit search.
fn fixed_font_size(
    block_size: Option<f32>,
    default_size: Option<f32>,
    default_shrinks: bool,
) -> Option<f32> {
    block_size.or_else(|| default_size.filter(|s| !default_shrinks && s.is_finite() && *s > 0.0))
}

/// `(min, max)` sizes for the fit search. With a project default size and
/// no per-block size, the default becomes the ceiling (so every block that
/// fits uses exactly that size) and the floor is lowered if needed so long
/// text can still shrink below it.
fn font_size_bounds(
    block_size: Option<f32>,
    default_size: Option<f32>,
    layout_box: LayoutBox,
    min_size: f32,
) -> (f32, f32) {
    match (block_size, default_size) {
        (None, Some(target)) if target.is_finite() && target > 0.0 => {
            (min_size.min(target), target)
        }
        _ => (min_size, max_font_size_for_box(layout_box, min_size)),
    }
}

fn min_font_size_for_image(image_width: u32, image_height: u32) -> f32 {
    let max_dim = image_width.max(image_height) as f32;
    (max_dim / 90.0).clamp(12.0, 28.0)
}

/// Maximum font size for the given layout box, derived from its dimensions.
/// Caps extreme cases (huge empty bubble + short text → giant glyphs).
fn max_font_size_for_box(layout_box: LayoutBox, min_size: f32) -> f32 {
    const GLOBAL_CAP_PX: f32 = 72.0;
    let by_height = layout_box.height * 0.45;
    let by_width = layout_box.width * 0.9;
    by_height.min(by_width).clamp(min_size + 1.0, GLOBAL_CAP_PX)
}

/// Binary-search the largest integer font size in `[min_size, max_size]`
/// whose shaped layout still fits inside the constraint box. An
/// `explicit_size` override (user-set per-block font size) bypasses the
/// search.
fn fit_font_size<'a>(
    layout_builder: &TextLayout<'a>,
    text: &str,
    constraint_width: f32,
    constraint_height: f32,
    explicit_size: Option<f32>,
    min_size: f32,
    max_size: f32,
) -> Result<LayoutRun<'a>> {
    let run_at = |size: f32| -> Result<LayoutRun<'a>> {
        layout_builder
            .clone()
            .with_font_size(size.max(1.0))
            .with_max_width(constraint_width)
            .with_max_height(constraint_height)
            .run(text)
    };
    if let Some(s) = explicit_size {
        return run_at(s);
    }

    let fits =
        |run: &LayoutRun<'a>| run.width <= constraint_width && run.height <= constraint_height;

    let min_size = min_size.max(1.0).round() as i32;
    let max_size = (max_size.round() as i32).max(min_size);

    let at_max = run_at(max_size as f32)?;
    if fits(&at_max) {
        return Ok(at_max);
    }
    // Binary-search [min, max) for the largest fitting size.
    let mut lo = min_size;
    let mut hi = max_size - 1;
    let mut best = run_at(min_size as f32)?;
    if !fits(&best) {
        return Ok(best);
    }
    while lo <= hi {
        let mid = lo + (hi - lo) / 2;
        let candidate = run_at(mid as f32)?;
        if fits(&candidate) {
            best = candidate;
            lo = mid + 1;
        } else {
            hi = mid - 1;
        }
    }
    Ok(best)
}

#[allow(clippy::too_many_arguments)]
fn fit_rendered_with_mask_collision<'a, F>(
    layout_builder: &TextLayout<'a>,
    text: &str,
    layout_box: LayoutBox,
    explicit_size: Option<f32>,
    min_size: f32,
    max_size: f32,
    mask: &GrayImage,
    bubble_id: u8,
    render_candidate: &mut F,
) -> Result<RenderedTextCandidate>
where
    F: FnMut(&LayoutRun<'a>) -> Result<RenderedTextCandidate>,
{
    if let Some(size) = explicit_size {
        let attempt = render_mask_collision_attempt(
            layout_builder,
            text,
            layout_box,
            size.max(1.0),
            mask,
            bubble_id,
            render_candidate,
        )?;
        return Ok(attempt.candidate);
    }

    let min_size = min_size.max(1.0).round() as i32;
    let max_size = (max_size.max(1.0).round() as i32).max(min_size);

    if let Some(candidate) = try_mask_collision_size(
        layout_builder,
        text,
        layout_box,
        max_size as f32,
        mask,
        bubble_id,
        render_candidate,
    )? {
        return Ok(candidate);
    }

    let min_attempt = render_mask_collision_attempt(
        layout_builder,
        text,
        layout_box,
        min_size as f32,
        mask,
        bubble_id,
        render_candidate,
    )?;
    if !min_attempt.valid {
        return Ok(min_attempt.candidate);
    }
    let mut best = min_attempt.candidate;

    let mut lo = min_size + 1;
    let mut hi = max_size - 1;
    while lo <= hi {
        let mid = lo + (hi - lo) / 2;
        if let Some(candidate) = try_mask_collision_size(
            layout_builder,
            text,
            layout_box,
            mid as f32,
            mask,
            bubble_id,
            render_candidate,
        )? {
            best = candidate;
            lo = mid + 1;
        } else {
            hi = mid - 1;
        }
    }

    Ok(best)
}

#[allow(clippy::too_many_arguments)]
fn try_mask_collision_size<'a, F>(
    layout_builder: &TextLayout<'a>,
    text: &str,
    layout_box: LayoutBox,
    font_size: f32,
    mask: &GrayImage,
    bubble_id: u8,
    render_candidate: &mut F,
) -> Result<Option<RenderedTextCandidate>>
where
    F: FnMut(&LayoutRun<'a>) -> Result<RenderedTextCandidate>,
{
    let layout = run_collision_layout_at(layout_builder, text, layout_box, font_size)?;
    let fits_layout_box = layout_fits_collision_attempt(&layout, layout_box);
    if !fits_layout_box {
        return Ok(None);
    }

    let candidate = render_candidate(&layout)?;
    if sprite_collides_with_bubble_mask(&candidate.image, &candidate.transform, mask, bubble_id) {
        return Ok(None);
    }
    Ok(Some(candidate))
}

#[allow(clippy::too_many_arguments)]
fn render_mask_collision_attempt<'a, F>(
    layout_builder: &TextLayout<'a>,
    text: &str,
    layout_box: LayoutBox,
    font_size: f32,
    mask: &GrayImage,
    bubble_id: u8,
    render_candidate: &mut F,
) -> Result<MaskCollisionAttempt>
where
    F: FnMut(&LayoutRun<'a>) -> Result<RenderedTextCandidate>,
{
    let layout = run_collision_layout_at(layout_builder, text, layout_box, font_size)?;
    let fits_layout_box = layout_fits_collision_attempt(&layout, layout_box);
    let candidate = render_candidate(&layout)?;
    let valid = fits_layout_box
        && !sprite_collides_with_bubble_mask(
            &candidate.image,
            &candidate.transform,
            mask,
            bubble_id,
        );
    Ok(MaskCollisionAttempt { candidate, valid })
}

fn run_collision_layout_at<'a>(
    layout_builder: &TextLayout<'a>,
    text: &str,
    layout_box: LayoutBox,
    font_size: f32,
) -> Result<LayoutRun<'a>> {
    layout_builder
        .clone()
        .with_font_size(font_size.max(1.0))
        .with_max_width(layout_box.width.max(1.0))
        .with_max_height(layout_box.height.max(1.0))
        .run(text)
}

fn layout_fits_collision_attempt(layout: &LayoutRun<'_>, layout_box: LayoutBox) -> bool {
    layout.width <= layout_box.width + FIT_EPSILON
        && layout.height <= layout_box.height + FIT_EPSILON
}

fn sprite_collides_with_bubble_mask(
    sprite: &RgbaImage,
    transform: &Transform,
    mask: &GrayImage,
    bubble_id: u8,
) -> bool {
    let origin_x = transform.x.round() as i32;
    let origin_y = transform.y.round() as i32;
    let mask_w = mask.width() as i32;
    let mask_h = mask.height() as i32;

    for (x, y, pixel) in sprite.enumerate_pixels() {
        if pixel.0[3] <= MASK_COLLISION_ALPHA_THRESHOLD {
            continue;
        }
        let mask_x = origin_x + x as i32;
        let mask_y = origin_y + y as i32;
        if mask_x < 0 || mask_y < 0 || mask_x >= mask_w || mask_y >= mask_h {
            return true;
        }
        if mask.get_pixel(mask_x as u32, mask_y as u32).0[0] != bubble_id {
            return true;
        }
    }
    false
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ResolvedLayoutBox {
    seed_box: LayoutBox,
    layout_box: LayoutBox,
    bubble_id: Option<u8>,
}

fn resolve_layout_boxes(
    blocks: &[RenderBlockInput],
    bubble_index: Option<&BubbleIndex>,
) -> Vec<ResolvedLayoutBox> {
    let Some(bubble_index) = bubble_index else {
        return blocks
            .iter()
            .map(|block| {
                let seed_box = seed_layout_box(block);
                ResolvedLayoutBox {
                    seed_box,
                    layout_box: seed_box,
                    bubble_id: None,
                }
            })
            .collect();
    };

    let mut counts: HashMap<u8, usize> = HashMap::new();
    let mut matches = Vec::with_capacity(blocks.len());

    for block in blocks {
        let seed_box = seed_layout_box(block);
        let translation = block.translation.trim();
        let bubble_match = if block.lock_layout_box || translation.is_empty() {
            None
        } else {
            let layout_source = layout_source_from_input(block, translation);
            let writing_mode = writing_mode_for_block(&layout_source);
            bubble_index.lookup_match(seed_box, writing_mode)
        };
        if let Some(matched) = bubble_match {
            *counts.entry(matched.id).or_insert(0) += 1;
        }
        matches.push((seed_box, bubble_match));
    }

    matches
        .into_iter()
        .map(|(seed_box, bubble_match)| match bubble_match {
            // Connected bubbles can contain multiple independently detected
            // text blocks. Expanding all of them to the same safe area makes
            // their layouts collide, so shared bubbles keep each block's
            // original detector box.
            Some(matched) if counts.get(&matched.id).copied().unwrap_or(0) == 1 => {
                ResolvedLayoutBox {
                    seed_box,
                    layout_box: matched.layout_box,
                    bubble_id: Some(matched.id),
                }
            }
            Some(matched) => ResolvedLayoutBox {
                seed_box,
                layout_box: seed_box,
                bubble_id: Some(matched.id),
            },
            None => ResolvedLayoutBox {
                seed_box,
                layout_box: seed_box,
                bubble_id: None,
            },
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Helpers: font families, fallbacks
// ---------------------------------------------------------------------------

fn apply_default_font_families(font_families: &mut Vec<String>, text: &str) {
    if font_families.is_empty() {
        *font_families = font_families_for_text(text);
    }
}

fn load_symbol_fallbacks(fontbook: &mut FontBook) -> Vec<Font> {
    let candidates = [
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
    let faces = fontbook.all_families();
    candidates
        .iter()
        .filter_map(|candidate| face_post_script_name(&faces, candidate))
        .filter_map(|post_script_name| fontbook.query(&post_script_name).ok())
        .collect()
}

fn face_post_script_name(faces: &[FaceInfo], candidate: &str) -> Option<String> {
    let candidate_lower = candidate.trim().to_lowercase();
    faces
        .iter()
        .find(|face| {
            face.post_script_name.to_lowercase() == candidate_lower
                || face
                    .families
                    .iter()
                    .any(|(family, _)| family.to_lowercase() == candidate_lower)
        })
        .map(|face| face.post_script_name.clone())
        .filter(|post_script_name| !post_script_name.is_empty())
}

fn layout_source_from_input(block: &RenderBlockInput, translation: &str) -> RenderBlock {
    RenderBlock {
        x: block.transform.x,
        y: block.transform.y,
        width: block.transform.width.max(1.0),
        height: block.transform.height.max(1.0),
        text: translation.to_string(),
        source_direction: block.source_direction.map(core_direction_to_renderer),
    }
}

/// Target width of a widened box relative to its height.
const WIDEN_RATIO: f32 = 0.8;
/// A box is "narrow" when its height exceeds its width by this factor.
const NARROW_ASPECT: f32 = 1.2;
/// Space kept between a widened box and a neighbouring block.
const WIDEN_GAP: f32 = 4.0;

/// Horizontal translations (e.g. Thai) of vertical source text get a tall,
/// narrow detector box, which wraps them into one-word lines. Widen such
/// boxes around their centre (to `WIDEN_RATIO` × height), stopping short of
/// neighbouring blocks on the same rows and the page edges. Skips locked
/// boxes and boxes already expanded into their bubble.
fn widen_narrow_boxes(
    blocks: &[RenderBlockInput],
    boxes: &mut [ResolvedLayoutBox],
    page_width: f32,
) {
    let seeds: Vec<LayoutBox> = blocks.iter().map(seed_layout_box).collect();
    for (i, block) in blocks.iter().enumerate() {
        let Some(resolved) = boxes.get(i) else { break };
        let current = resolved.layout_box;
        let translation = block.translation.trim();
        if block.lock_layout_box || current != resolved.seed_box || translation.is_empty() {
            continue;
        }
        let mode = writing_mode_for_block(&layout_source_from_input(block, translation));
        if mode != WritingMode::Horizontal || current.height < current.width * NARROW_ASPECT {
            continue;
        }
        let target = current.height * WIDEN_RATIO;
        if target <= current.width {
            continue;
        }
        let centre = current.x + current.width / 2.0;
        let mut left = (centre - target / 2.0).max(0.0);
        let mut right = (centre + target / 2.0).min(page_width.max(current.x + current.width));
        for (j, other) in seeds.iter().enumerate() {
            if j == i {
                continue;
            }
            let same_rows =
                other.y < current.y + current.height && other.y + other.height > current.y;
            if !same_rows {
                continue;
            }
            if other.x >= current.x + current.width {
                right = right.min(other.x - WIDEN_GAP);
            } else if other.x + other.width <= current.x {
                left = left.max(other.x + other.width + WIDEN_GAP);
            }
        }
        // Never end up narrower than the original box.
        let left = left.min(current.x);
        let right = right.max(current.x + current.width);
        boxes[i].layout_box = LayoutBox {
            x: left,
            y: current.y,
            width: right - left,
            height: current.height,
        };
    }
}

fn seed_layout_box(block: &RenderBlockInput) -> LayoutBox {
    LayoutBox {
        x: block.transform.x,
        y: block.transform.y,
        width: block.transform.width.max(1.0),
        height: block.transform.height.max(1.0),
    }
}

// ---------------------------------------------------------------------------
// Helpers: stroke resolution
// ---------------------------------------------------------------------------

fn default_stroke_width(font_size: f32) -> f32 {
    (font_size * 0.10).clamp(1.2, 8.0)
}

fn contrasting_stroke_color(text_color: [u8; 4]) -> [u8; 4] {
    let luminance =
        0.299 * text_color[0] as f32 + 0.587 * text_color[1] as f32 + 0.114 * text_color[2] as f32;
    if luminance > 128.0 {
        [0, 0, 0, 255]
    } else {
        [255, 255, 255, 255]
    }
}

fn resolve_stroke_style(
    font_prediction: Option<&FontPrediction>,
    block_stroke: Option<&TextStrokeStyle>,
    global_stroke: Option<&TextStrokeStyle>,
    font_size: f32,
    text_color: [u8; 4],
    default_stroke: Option<&koharu_core::StrokeDefaults>,
) -> Option<RenderStrokeOptions> {
    if let Some(stroke) = block_stroke {
        if !stroke.enabled {
            return None;
        }
        return Some(RenderStrokeOptions {
            color: stroke.color,
            width_px: stroke
                .width_px
                .unwrap_or_else(|| default_stroke_width(font_size)),
        });
    }
    if let Some(stroke) = global_stroke {
        if !stroke.enabled {
            return None;
        }
        return Some(RenderStrokeOptions {
            color: stroke.color,
            width_px: stroke
                .width_px
                .unwrap_or_else(|| default_stroke_width(font_size)),
        });
    }
    if default_stroke.is_some_and(|d| !d.enabled) {
        return None;
    }
    let auto_stroke_color = contrasting_stroke_color(text_color);
    let color = default_stroke
        .and_then(|d| d.color)
        .unwrap_or(auto_stroke_color);
    // A width set for the project / speaker keeps every block's outline
    // the same; otherwise use the detected lettering, then the text size.
    let width_px = default_stroke
        .and_then(|d| d.width_px)
        .filter(|w| w.is_finite() && *w > 0.0)
        .or_else(|| {
            font_prediction
                .map(|p| p.stroke_width_px)
                .filter(|w| *w > 0.0)
        })
        .unwrap_or_else(|| default_stroke_width(font_size));
    Some(RenderStrokeOptions { color, width_px })
}

/// Block style colour → speaker (character) colour → project default colour →
/// detected source colour → black.
///
/// Editing any style field of a block (size, alignment, font…) makes the UI
/// store a whole `TextStyle`, and `TextStyle::color` can't be left empty, so
/// the block gets the colour it was showing at that moment stamped in. Such
/// a stamped colour — the project default, the detected colour or plain
/// black — is not a real choice and must not hide the speaker's colour.
pub fn resolve_text_color(
    explicit_style: Option<&TextStyle>,
    derived_style: &TextStyle,
    font_prediction: Option<&FontPrediction>,
    speaker_color: Option<[u8; 4]>,
    project_default: Option<[u8; 4]>,
) -> [u8; 4] {
    let predicted = font_prediction.map(|pred| {
        [
            pred.text_color[0],
            pred.text_color[1],
            pred.text_color[2],
            255,
        ]
    });
    if explicit_style.is_some() {
        let color = derived_style.color;
        let stamped =
            Some(color) == project_default || Some(color) == predicted || color == [0, 0, 0, 255];
        return match speaker_color {
            Some(speaker) if stamped => speaker,
            _ => color,
        };
    }
    speaker_color
        .or(project_default)
        .or(predicted)
        .unwrap_or([0, 0, 0, 255])
}

// ---------------------------------------------------------------------------
// Helpers: type conversions
// ---------------------------------------------------------------------------

fn shader_core_to_renderer(e: TextShaderEffect) -> RendererEffect {
    RendererEffect {
        italic: e.italic,
        bold: e.bold,
    }
}

fn core_align_to_renderer(a: koharu_core::TextAlign) -> RendererTextAlign {
    match a {
        koharu_core::TextAlign::Left => RendererTextAlign::Left,
        koharu_core::TextAlign::Center => RendererTextAlign::Center,
        koharu_core::TextAlign::Right => RendererTextAlign::Right,
    }
}

fn core_direction_to_renderer(d: TextDirection) -> RendererTextDirection {
    match d {
        TextDirection::Horizontal => RendererTextDirection::Horizontal,
        TextDirection::Vertical => RendererTextDirection::Vertical,
    }
}

fn rendered_direction_for_writing_mode(writing_mode: WritingMode) -> TextDirection {
    match writing_mode {
        WritingMode::Horizontal => TextDirection::Horizontal,
        WritingMode::VerticalRl => TextDirection::Vertical,
    }
}

// ---------------------------------------------------------------------------
// Helpers: placement
// ---------------------------------------------------------------------------

/// Rotate a rendered block clockwise by `rotation_deg` about its centre.
/// Text is laid out upright in the block's box first, so line breaks and
/// size don't change with the angle; the finished sprite is then rotated
/// into a canvas large enough for its corners, keeping the same centre.
fn rotate_candidate(candidate: RenderedTextCandidate, rotation_deg: f32) -> RenderedTextCandidate {
    let angle = normalize_degrees(rotation_deg);
    if angle.abs() < 0.01 || !angle.is_finite() {
        return candidate;
    }
    let rotated = rotate_rgba(&candidate.image, angle);
    let t = candidate.transform;
    let cx = t.x + t.width * 0.5;
    let cy = t.y + t.height * 0.5;
    let (w, h) = (rotated.width() as f32, rotated.height() as f32);
    RenderedTextCandidate {
        image: rotated,
        transform: Transform {
            x: (cx - w * 0.5).round(),
            y: (cy - h * 0.5).round(),
            width: w,
            height: h,
            rotation_deg: angle,
        },
    }
}

/// Map any angle to (-180, 180].
fn normalize_degrees(deg: f32) -> f32 {
    let mut d = deg % 360.0;
    if d > 180.0 {
        d -= 360.0;
    } else if d <= -180.0 {
        d += 360.0;
    }
    d
}

/// Clockwise rotation (screen coordinates, y down) into an expanded canvas.
/// Bilinear sampling on premultiplied alpha so glyph edges don't pick up
/// dark fringes from transparent pixels.
fn rotate_rgba(src: &RgbaImage, deg: f32) -> RgbaImage {
    let (sin, cos) = deg.to_radians().sin_cos();
    let (w, h) = (src.width() as f32, src.height() as f32);
    // Small tolerance so float noise (cos 90° ≈ 4e-8) doesn't add a pixel.
    let out_w = (w * cos.abs() + h * sin.abs() - 1e-3).ceil().max(1.0) as u32;
    let out_h = (w * sin.abs() + h * cos.abs() - 1e-3).ceil().max(1.0) as u32;
    let (src_cx, src_cy) = (w * 0.5, h * 0.5);
    let (out_cx, out_cy) = (out_w as f32 * 0.5, out_h as f32 * 0.5);
    let mut out = RgbaImage::new(out_w, out_h);

    // Premultiplied source pixel (zero outside the image).
    let fetch = |x: i64, y: i64| -> [f32; 4] {
        if x < 0 || y < 0 || x >= src.width() as i64 || y >= src.height() as i64 {
            return [0.0; 4];
        }
        let p = src.get_pixel(x as u32, y as u32).0;
        let a = p[3] as f32 / 255.0;
        [
            p[0] as f32 * a,
            p[1] as f32 * a,
            p[2] as f32 * a,
            p[3] as f32,
        ]
    };

    for oy in 0..out_h {
        for ox in 0..out_w {
            // Inverse-rotate the destination pixel centre into the source.
            let dx = ox as f32 + 0.5 - out_cx;
            let dy = oy as f32 + 0.5 - out_cy;
            let sx = dx * cos + dy * sin + src_cx - 0.5;
            let sy = -dx * sin + dy * cos + src_cy - 0.5;
            if sx < -1.0 || sy < -1.0 || sx > w || sy > h {
                continue;
            }
            let (x0, y0) = (sx.floor(), sy.floor());
            let (fx, fy) = (sx - x0, sy - y0);
            let (x0, y0) = (x0 as i64, y0 as i64);
            let p00 = fetch(x0, y0);
            let p10 = fetch(x0 + 1, y0);
            let p01 = fetch(x0, y0 + 1);
            let p11 = fetch(x0 + 1, y0 + 1);
            let mut px = [0.0f32; 4];
            for c in 0..4 {
                let top = p00[c] * (1.0 - fx) + p10[c] * fx;
                let bottom = p01[c] * (1.0 - fx) + p11[c] * fx;
                px[c] = top * (1.0 - fy) + bottom * fy;
            }
            let alpha = px[3];
            if alpha <= 0.0 {
                continue;
            }
            let unpremultiply = |v: f32| (v / (alpha / 255.0)).round().clamp(0.0, 255.0) as u8;
            out.put_pixel(
                ox,
                oy,
                image::Rgba([
                    unpremultiply(px[0]),
                    unpremultiply(px[1]),
                    unpremultiply(px[2]),
                    alpha.round().clamp(0.0, 255.0) as u8,
                ]),
            );
        }
    }
    out
}

fn centred_sprite_transform(
    anchor_box: LayoutBox,
    sprite_width: u32,
    sprite_height: u32,
    rotation_deg: f32,
) -> Transform {
    let sprite_w = sprite_width as f32;
    let sprite_h = sprite_height as f32;
    let cx = anchor_box.x + anchor_box.width * 0.5;
    let cy = anchor_box.y + anchor_box.height * 0.5;
    Transform {
        x: (cx - sprite_w * 0.5).round(),
        y: (cy - sprite_h * 0.5).round(),
        width: sprite_w,
        height: sprite_h,
        rotation_deg,
    }
}

fn find_input(blocks: &[RenderBlockInput], id: NodeId) -> &RenderBlockInput {
    blocks
        .iter()
        .find(|b| b.node_id == id)
        .expect("rendered_block must have matching input")
}

fn placement_origin(input: &RenderBlockInput, expanded: &Option<Transform>) -> (f32, f32) {
    if let Some(t) = expanded {
        (t.x.round(), t.y.round())
    } else {
        (input.transform.x, input.transform.y)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GrayImage, Luma, Rgba, RgbaImage};
    use koharu_core::NodeId;

    #[test]
    fn fixed_project_size_is_used_unless_shrinking_or_block_has_own_size() {
        assert_eq!(fixed_font_size(None, Some(28.0), false), Some(28.0));
        assert_eq!(fixed_font_size(None, Some(28.0), true), None);
        assert_eq!(fixed_font_size(Some(40.0), Some(28.0), false), Some(40.0));
        assert_eq!(fixed_font_size(None, None, false), None);
        assert_eq!(fixed_font_size(None, Some(f32::NAN), false), None);
    }

    #[test]
    fn normalizes_angles_into_half_open_range() {
        assert_eq!(normalize_degrees(0.0), 0.0);
        assert_eq!(normalize_degrees(370.0), 10.0);
        assert_eq!(normalize_degrees(-190.0), 170.0);
        assert_eq!(normalize_degrees(180.0), 180.0);
        assert_eq!(normalize_degrees(-180.0), 180.0);
    }

    #[test]
    fn rotating_90_degrees_swaps_size_and_turns_clockwise() {
        // 4x2 image: left half red, right half blue.
        let mut img = RgbaImage::new(4, 2);
        for y in 0..2 {
            for x in 0..4 {
                let c = if x < 2 {
                    [255, 0, 0, 255]
                } else {
                    [0, 0, 255, 255]
                };
                img.put_pixel(x, y, Rgba(c));
            }
        }
        let out = rotate_rgba(&img, 90.0);
        assert_eq!((out.width(), out.height()), (2, 4));
        // Clockwise: the left (red) end ends up on top.
        assert_eq!(out.get_pixel(1, 0).0, [255, 0, 0, 255]);
        assert_eq!(out.get_pixel(0, 3).0, [0, 0, 255, 255]);
    }

    #[test]
    fn rotated_candidate_keeps_its_centre() {
        let candidate = RenderedTextCandidate {
            image: RgbaImage::from_pixel(100, 20, Rgba([0, 0, 0, 255])),
            transform: Transform {
                x: 50.0,
                y: 40.0,
                width: 100.0,
                height: 20.0,
                rotation_deg: 0.0,
            },
        };
        let out = rotate_candidate(candidate, 30.0);
        let t = out.transform;
        assert!((t.x + t.width / 2.0 - 100.0).abs() <= 1.0);
        assert!((t.y + t.height / 2.0 - 50.0).abs() <= 1.0);
        assert!(t.width < 100.0 && t.height > 20.0);
        assert_eq!(t.rotation_deg, 30.0);
        // No rotation is a no-op.
        let same = rotate_candidate(
            RenderedTextCandidate {
                image: RgbaImage::new(3, 3),
                transform: Transform::default(),
            },
            360.0,
        );
        assert_eq!((same.image.width(), same.image.height()), (3, 3));
    }

    #[test]
    fn project_default_size_caps_the_fit_search() {
        let layout_box = LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 400.0,
        };
        // No defaults: unchanged auto-size bounds.
        assert_eq!(
            font_size_bounds(None, None, layout_box, 14.0),
            (14.0, max_font_size_for_box(layout_box, 14.0))
        );
        // Project default: it is the ceiling.
        assert_eq!(
            font_size_bounds(None, Some(30.0), layout_box, 14.0),
            (14.0, 30.0)
        );
        // A default below the page minimum lowers the floor too.
        assert_eq!(
            font_size_bounds(None, Some(10.0), layout_box, 14.0),
            (10.0, 10.0)
        );
        // A per-block size ignores the project default.
        assert_eq!(
            font_size_bounds(Some(22.0), Some(30.0), layout_box, 14.0),
            (14.0, max_font_size_for_box(layout_box, 14.0))
        );
    }

    #[test]
    fn default_font_families_should_fill_empty_list() {
        let mut font_families = Vec::new();
        apply_default_font_families(&mut font_families, "hello");
        assert!(!font_families.is_empty());
    }

    #[test]
    fn default_stroke_color_uses_black_for_light_text() {
        let stroke = resolve_stroke_style(None, None, None, 16.0, [255, 255, 255, 255], None)
            .expect("default stroke should be present");
        assert_eq!(stroke.color, [0, 0, 0, 255]);
        assert_eq!(stroke.width_px, 1.6);
    }

    #[test]
    fn predicted_stroke_width_keeps_auto_black_or_white_color() {
        let prediction = FontPrediction {
            stroke_color: [12, 34, 56],
            stroke_width_px: 3.0,
            ..Default::default()
        };
        let stroke = resolve_stroke_style(
            Some(&prediction),
            None,
            None,
            18.0,
            [255, 255, 255, 255],
            None,
        )
        .expect("predicted stroke should be present");
        assert_eq!(stroke.color, [0, 0, 0, 255]);
        assert_eq!(stroke.width_px, 3.0);
    }

    #[test]
    fn default_stroke_overrides_detected_width_and_auto_colour() {
        let prediction = FontPrediction {
            stroke_width_px: 7.0,
            ..Default::default()
        };
        let d = koharu_core::StrokeDefaults {
            enabled: true,
            color: Some([10, 20, 30, 255]),
            width_px: Some(2.5),
        };
        let s = resolve_stroke_style(
            Some(&prediction),
            None,
            None,
            40.0,
            [0, 0, 0, 255],
            Some(&d),
        )
        .unwrap();
        assert_eq!((s.color, s.width_px), ([10, 20, 30, 255], 2.5));
        // Only a width: colour stays automatic.
        let w = koharu_core::StrokeDefaults {
            color: None,
            ..d.clone()
        };
        let s = resolve_stroke_style(None, None, None, 40.0, [0, 0, 0, 255], Some(&w)).unwrap();
        assert_eq!((s.color, s.width_px), ([255, 255, 255, 255], 2.5));
        // Turned off.
        let off = koharu_core::StrokeDefaults {
            enabled: false,
            ..d
        };
        assert!(resolve_stroke_style(None, None, None, 40.0, [0, 0, 0, 255], Some(&off)).is_none());
    }

    #[test]
    fn explicit_block_stroke_color_is_preserved_even_if_it_matches_text() {
        let stroke = resolve_stroke_style(
            None,
            Some(&TextStrokeStyle {
                enabled: true,
                color: [255, 255, 255, 255],
                width_px: Some(2.0),
            }),
            None,
            18.0,
            [255, 255, 255, 255],
            None,
        )
        .expect("explicit stroke should be present");
        assert_eq!(stroke.color, [255, 255, 255, 255]);
        assert_eq!(stroke.width_px, 2.0);
    }

    #[test]
    fn predicted_text_color_wins_without_explicit_style() {
        let derived = TextStyle {
            font_families: Vec::new(),
            font_size: None,
            color: [0, 0, 0, 255],
            effect: None,
            stroke: None,
            text_align: None,
        };
        let prediction = FontPrediction {
            text_color: [12, 34, 56],
            ..Default::default()
        };
        assert_eq!(
            resolve_text_color(None, &derived, Some(&prediction), None, None),
            [12, 34, 56, 255]
        );
    }

    #[test]
    fn project_default_color_wins_over_prediction_but_not_block_style() {
        let derived = TextStyle {
            font_families: Vec::new(),
            font_size: None,
            color: [0, 0, 0, 255],
            effect: None,
            stroke: None,
            text_align: None,
        };
        let prediction = FontPrediction {
            text_color: [12, 34, 56],
            ..Default::default()
        };
        let white = Some([255, 255, 255, 255]);
        assert_eq!(
            resolve_text_color(None, &derived, Some(&prediction), None, white),
            [255, 255, 255, 255]
        );
        let explicit = TextStyle {
            color: [200, 100, 50, 255],
            ..derived.clone()
        };
        assert_eq!(
            resolve_text_color(Some(&explicit), &explicit, Some(&prediction), None, white),
            [200, 100, 50, 255]
        );
    }

    #[test]
    fn speaker_color_beats_a_stamped_block_colour_but_not_a_chosen_one() {
        let prediction = FontPrediction {
            text_color: [12, 34, 56],
            ..Default::default()
        };
        let project = Some([255, 255, 255, 255]);
        let pink = Some([247, 12, 132, 255]);
        let style = |color| TextStyle {
            font_families: Vec::new(),
            font_size: Some(30.0),
            color,
            effect: None,
            stroke: None,
            text_align: None,
        };
        // No block style: speaker beats project default.
        let plain = style([0, 0, 0, 255]);
        assert_eq!(
            resolve_text_color(None, &plain, Some(&prediction), pink, project),
            pink.unwrap()
        );
        // Block style whose colour is just the stamped project / detected /
        // black colour: speaker still wins.
        for stamped in [[255, 255, 255, 255], [12, 34, 56, 255], [0, 0, 0, 255]] {
            let s = style(stamped);
            assert_eq!(
                resolve_text_color(Some(&s), &s, Some(&prediction), pink, project),
                pink.unwrap()
            );
        }
        // A colour the user picked for this block wins over the speaker.
        let chosen = style([40, 200, 90, 255]);
        assert_eq!(
            resolve_text_color(Some(&chosen), &chosen, Some(&prediction), pink, project),
            [40, 200, 90, 255]
        );
        // Without a speaker colour a stamped colour is kept as before.
        let s = style([255, 255, 255, 255]);
        assert_eq!(
            resolve_text_color(Some(&s), &s, Some(&prediction), None, project),
            [255, 255, 255, 255]
        );
    }

    #[test]
    fn explicit_text_color_wins_over_prediction() {
        let explicit = TextStyle {
            font_families: Vec::new(),
            font_size: None,
            color: [200, 100, 50, 255],
            effect: None,
            stroke: None,
            text_align: None,
        };
        let prediction = FontPrediction {
            text_color: [12, 34, 56],
            ..Default::default()
        };
        assert_eq!(
            resolve_text_color(Some(&explicit), &explicit, Some(&prediction), None, None),
            [200, 100, 50, 255]
        );
    }

    #[test]
    fn mask_collision_fit_renders_min_size_when_no_safe_size_exists() -> Result<()> {
        let font = any_system_font();
        let layout_builder = TextLayout::new(&font, None);
        let layout_box = LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 24.0,
            height: 12.0,
        };
        let mask = GrayImage::from_pixel(64, 64, Luma([0u8]));
        let mut rendered_sizes = Vec::new();
        let mut render_candidate = |layout: &LayoutRun<'_>| -> Result<RenderedTextCandidate> {
            rendered_sizes.push(layout.font_size);
            let width = layout.width.ceil().max(1.0) as u32;
            let height = layout.height.ceil().max(1.0) as u32;
            Ok(RenderedTextCandidate {
                image: RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 255])),
                transform: Transform {
                    x: 0.0,
                    y: 0.0,
                    width: width as f32,
                    height: height as f32,
                    rotation_deg: 0.0,
                },
            })
        };

        let candidate = fit_rendered_with_mask_collision(
            &layout_builder,
            "overflowing text",
            layout_box,
            None,
            12.0,
            18.0,
            &mask,
            1,
            &mut render_candidate,
        )?;

        assert_eq!(rendered_sizes.last().copied(), Some(12.0));
        assert!(candidate.image.width() >= 1);
        assert!(candidate.image.height() >= 1);
        Ok(())
    }

    #[test]
    fn shared_bubble_keeps_seed_boxes_to_avoid_overlap() {
        let mut mask = GrayImage::from_pixel(200, 200, Luma([0u8]));
        paint_rect(&mut mask, 10, 10, 190, 190, 1);
        let index = BubbleIndex::new(mask);
        let blocks = vec![
            block(30.0, 30.0, 40.0, 80.0, "hello"),
            block(120.0, 30.0, 40.0, 80.0, "world"),
        ];

        let layout_boxes = resolve_layout_boxes(&blocks, Some(&index));

        assert_eq!(layout_boxes[0].layout_box, seed_layout_box(&blocks[0]));
        assert_eq!(layout_boxes[0].bubble_id, Some(1));
        assert_eq!(layout_boxes[1].layout_box, seed_layout_box(&blocks[1]));
        assert_eq!(layout_boxes[1].bubble_id, Some(1));
    }

    #[test]
    fn single_block_can_still_expand_into_its_bubble() {
        let mut mask = GrayImage::from_pixel(200, 200, Luma([0u8]));
        paint_rect(&mut mask, 20, 20, 180, 180, 1);
        let index = BubbleIndex::new(mask);
        let blocks = vec![block(70.0, 70.0, 20.0, 30.0, "hello")];

        let layout_boxes = resolve_layout_boxes(&blocks, Some(&index));

        assert!(layout_boxes[0].layout_box.width > blocks[0].transform.width);
        assert!(layout_boxes[0].layout_box.height > blocks[0].transform.height);
        assert_eq!(layout_boxes[0].bubble_id, Some(1));
    }

    fn resolved(blocks: &[RenderBlockInput]) -> Vec<ResolvedLayoutBox> {
        resolve_layout_boxes(blocks, None)
    }

    #[test]
    fn narrow_box_widens_around_its_centre() {
        let blocks = vec![block(100.0, 50.0, 40.0, 200.0, "ไปทะเลกันเถอะ")];
        let mut boxes = resolved(&blocks);
        widen_narrow_boxes(&blocks, &mut boxes, 1000.0);
        let b = boxes[0].layout_box;
        assert_eq!(b.width, 160.0); // 200 * 0.8
        assert_eq!(b.x + b.width / 2.0, 120.0); // centre kept
        assert_eq!((b.y, b.height), (50.0, 200.0));
    }

    #[test]
    fn widening_stops_at_neighbours_and_page_edge() {
        let blocks = vec![
            block(10.0, 50.0, 40.0, 200.0, "ซ้าย"),
            block(90.0, 60.0, 40.0, 200.0, "ขวา"),
        ];
        let mut boxes = resolved(&blocks);
        widen_narrow_boxes(&blocks, &mut boxes, 1000.0);
        let first = boxes[0].layout_box;
        assert_eq!(first.x, 0.0); // clamped to the page edge
        assert!(first.x + first.width <= 90.0 - WIDEN_GAP + 0.001);
        let second = boxes[1].layout_box;
        assert!(second.x >= 50.0 + WIDEN_GAP - 0.001);
    }

    #[test]
    fn wide_locked_or_vertical_boxes_are_left_alone() {
        let mut locked = block(100.0, 50.0, 40.0, 200.0, "ล็อก");
        locked.lock_layout_box = true;
        let blocks = vec![
            block(300.0, 50.0, 200.0, 80.0, "กว้างอยู่แล้ว"),
            locked,
            block(600.0, 50.0, 40.0, 200.0, "縦書きのまま"),
        ];
        let mut boxes = resolved(&blocks);
        widen_narrow_boxes(&blocks, &mut boxes, 1000.0);
        for (b, r) in blocks.iter().zip(&boxes) {
            assert_eq!(r.layout_box, seed_layout_box(b));
        }
    }

    #[test]
    fn locked_block_keeps_manual_layout_box_inside_bubble() {
        let mut mask = GrayImage::from_pixel(200, 200, Luma([0u8]));
        paint_rect(&mut mask, 20, 20, 180, 180, 1);
        let index = BubbleIndex::new(mask);
        let mut locked = block(70.0, 70.0, 20.0, 30.0, "hello");
        locked.lock_layout_box = true;
        let blocks = vec![locked];

        let layout_boxes = resolve_layout_boxes(&blocks, Some(&index));

        assert_eq!(layout_boxes[0].layout_box, seed_layout_box(&blocks[0]));
        assert_eq!(layout_boxes[0].bubble_id, None);
    }

    #[test]
    fn mask_collision_detects_alpha_outside_matched_bubble() {
        let mut mask = GrayImage::from_pixel(10, 10, Luma([0u8]));
        paint_rect(&mut mask, 2, 2, 8, 8, 1);
        let sprite = RgbaImage::from_pixel(4, 4, Rgba([0, 0, 0, 255]));

        let inside = Transform {
            x: 3.0,
            y: 3.0,
            width: 4.0,
            height: 4.0,
            rotation_deg: 0.0,
        };
        assert!(!sprite_collides_with_bubble_mask(
            &sprite, &inside, &mask, 1
        ));

        let outside = Transform {
            x: 0.0,
            y: 0.0,
            width: 4.0,
            height: 4.0,
            rotation_deg: 0.0,
        };
        assert!(sprite_collides_with_bubble_mask(
            &sprite, &outside, &mask, 1
        ));
    }

    #[test]
    fn mask_collision_ignores_transparent_sprite_pixels() {
        let mask = GrayImage::from_pixel(4, 4, Luma([0u8]));
        let sprite = RgbaImage::from_pixel(4, 4, Rgba([0, 0, 0, 0]));
        let transform = Transform {
            x: 0.0,
            y: 0.0,
            width: 4.0,
            height: 4.0,
            rotation_deg: 0.0,
        };

        assert!(!sprite_collides_with_bubble_mask(
            &sprite, &transform, &mask, 1
        ));
    }

    fn block(x: f32, y: f32, width: f32, height: f32, translation: &str) -> RenderBlockInput {
        RenderBlockInput {
            node_id: NodeId::new(),
            transform: Transform {
                x,
                y,
                width,
                height,
                rotation_deg: 0.0,
            },
            translation: translation.to_string(),
            style: None,
            font_prediction: None,
            source_direction: None,
            rendered_direction: None,
            lock_layout_box: false,
            speaker_font: None,
            speaker_color: None,
            speaker_font_size: None,
            speaker_effect: None,
            speaker_align: None,
            default_stroke: None,
        }
    }

    fn paint_rect(img: &mut GrayImage, x0: u32, y0: u32, x1: u32, y1: u32, value: u8) {
        for y in y0..y1 {
            for x in x0..x1 {
                img.put_pixel(x, y, Luma([value]));
            }
        }
    }

    fn any_system_font() -> Font {
        let mut book = FontBook::new();
        let preferred = [
            "Yu Gothic",
            "MS Gothic",
            "Noto Sans CJK JP",
            "Noto Sans",
            "Arial",
            "DejaVu Sans",
            "Liberation Sans",
        ];

        for name in preferred {
            if let Some(post_script_name) = book
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
                .filter(|post_script_name| !post_script_name.is_empty())
                && let Ok(font) = book.query(&post_script_name)
            {
                return font;
            }
        }

        if let Some(face) = book
            .all_families()
            .into_iter()
            .find(|face| !face.post_script_name.is_empty())
        {
            return book
                .query(&face.post_script_name)
                .expect("failed to load first system font");
        }

        panic!("no system font available for tests");
    }

    #[test]
    fn centred_sprite_transform_anchors_to_provided_box_center() {
        let anchor = LayoutBox {
            x: 100.0,
            y: 100.0,
            width: 200.0,
            height: 100.0,
        };
        let sprite_w = 100;
        let sprite_h = 50;

        let transform = centred_sprite_transform(anchor, sprite_w, sprite_h, 0.0);

        // Center of anchor is (200, 150).
        // Sprite (100x50) centered on (200, 150) starts at (150, 125).
        assert_eq!(transform.x, 150.0);
        assert_eq!(transform.y, 125.0);
    }
}
