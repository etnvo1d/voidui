//! Fontique discovery, Parley paragraph construction and Swash rasterization.
//! Font/layout workspaces are shared; native layouts own shared font handles.
mod raster;
use crate::*;
use parking_lot::Mutex;
use parley::{FontContext, FontFamily, FontFamilyName, LayoutContext, TextStyle};
use raster::Raster;
use rustc_hash::{FxHashMap, FxHashSet};
use std::{
    borrow::Cow,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TextSystemStats {
    pub paragraphs_shaped: u64,
    pub fonts_registered: u64,
}
struct State {
    font_resources: FxHashSet<String>,
    fonts: FontContext,
    layout: LayoutContext<TextBrush>,
    resolved: FxHashMap<Font, FontId>,
    line_metrics: FxHashMap<(Font, Pixels), parley::RunMetrics>,
}
pub struct ParleyTextSystem {
    state: Mutex<State>,
    pub(crate) raster: Raster,
    system_family: String,
    revision: AtomicU64,
    shaped: AtomicU64,
}
impl ParleyTextSystem {
    pub fn new(system_family: &str) -> Self {
        Self::with_system_fonts(system_family, true)
    }
    /// Skip native font discovery; register application-owned fonts with add_fonts.
    pub fn new_without_system_fonts(system_family: &str) -> Self {
        Self::with_system_fonts(system_family, false)
    }
    fn with_system_fonts(system_family: &str, system_fonts: bool) -> Self {
        Self {
            state: Mutex::new(State {
                font_resources: Default::default(),
                fonts: FontContext {
                    collection: parley::fontique::Collection::new(
                        parley::fontique::CollectionOptions {
                            system_fonts,
                            shared: false,
                        },
                    ),
                    source_cache: Default::default(),
                },
                layout: LayoutContext::new(),
                resolved: Default::default(),
                line_metrics: Default::default(),
            }),
            raster: Default::default(),
            system_family: system_family.into(),
            revision: AtomicU64::new(0),
            shaped: AtomicU64::new(0),
        }
    }
    pub fn add_fonts(&self, fonts: Vec<Cow<'static, [u8]>>) -> Result<()> {
        if fonts.is_empty() {
            return Ok(());
        }
        let mut state = self.state.lock();
        self.register_fonts(&mut state, fonts, true, None)
    }

    /// Register an immutable font resource once per shared font service. The
    /// loader runs only on a miss. Failed loads are not marked as registered.
    /// Named resources do not change generic-family defaults. The loader must
    /// not re-enter this service. Use a namespaced key for each immutable resource.
    pub fn add_fonts_once(
        &self,
        key: &str,
        load: impl FnOnce() -> Result<Vec<Cow<'static, [u8]>>>,
    ) -> Result<()> {
        self.register_resource(key, None, load)
    }

    /// Register a bundled face with authoritative family, weight and style,
    /// like a CSS @font-face rule. Use this when font-file metadata disagrees
    /// with its published face definition. No font bytes are modified.
    pub fn add_font_face_once(
        &self,
        key: &str,
        face: &Font,
        load: impl FnOnce() -> Result<Cow<'static, [u8]>>,
    ) -> Result<()> {
        self.register_resource(key, Some(face), || Ok(vec![load()?]))
    }

    fn register_resource(
        &self,
        key: &str,
        face: Option<&Font>,
        load: impl FnOnce() -> Result<Vec<Cow<'static, [u8]>>>,
    ) -> Result<()> {
        let mut state = self.state.lock();
        if state.font_resources.contains(key) {
            return Ok(());
        }
        let fonts = load()?;
        anyhow::ensure!(!fonts.is_empty(), "font resource is empty");
        self.register_fonts(&mut state, fonts, false, face)?;
        state.font_resources.insert(key.to_owned());
        Ok(())
    }

    fn register_fonts(
        &self,
        state: &mut State,
        fonts: Vec<Cow<'static, [u8]>>,
        generic_fallback: bool,
        face: Option<&Font>,
    ) -> Result<()> {
        // Fontique's non-shared collection uses copy-on-write. Commit only after
        // every blob succeeds, so a failed batch cannot leave caches stale.
        let mut collection = state.fonts.collection.clone();
        for data in fonts {
            let blob = match data {
                Cow::Borrowed(data) => parley::fontique::Blob::new(Arc::new(data)),
                Cow::Owned(data) => data.into(),
            };
            let metadata = face.map(|face| parley::fontique::FontInfoOverride {
                family_name: Some(face.family.as_str()),
                weight: Some(parley::FontWeight::new(face.weight.0)),
                style: Some(match face.style {
                    FontStyle::Normal => parley::FontStyle::Normal,
                    FontStyle::Italic => parley::FontStyle::Italic,
                    FontStyle::Oblique => parley::FontStyle::Oblique(None),
                }),
                ..Default::default()
            });
            let added = collection.register_fonts(blob, metadata);
            anyhow::ensure!(!added.is_empty(), "font data contains no supported face");
            // Explicit add_fonts also supplies a fallback in fontless hosts.
            // Named resources (math, icons, etc.) must not replace the default
            // UI face just because a newly opened document happens to use them.
            if generic_fallback {
                collection.append_generic_families(
                    parley::GenericFamily::SansSerif,
                    added.iter().map(|(id, _)| *id),
                );
            }
        }
        state.fonts.collection = collection;
        state.resolved.clear();
        state.line_metrics.clear();
        self.revision.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    pub fn all_font_names(&self) -> Vec<String> {
        let mut s = self.state.lock();
        let mut names: Vec<_> = s
            .fonts
            .collection
            .family_names()
            .map(str::to_owned)
            .collect();
        names.sort();
        names.dedup();
        names
    }
    pub(crate) fn revision(&self) -> u64 {
        self.revision.load(Ordering::Relaxed)
    }
    pub fn stats(&self) -> TextSystemStats {
        TextSystemStats {
            paragraphs_shaped: self.shaped.load(Ordering::Relaxed),
            fonts_registered: self.raster.len() as u64,
        }
    }
    fn families<'a>(&'a self, font: &'a Font) -> Vec<FontFamilyName<'a>> {
        let name = |name: &'a str| {
            if name == ".SystemUIFont" {
                FontFamilyName::named(&self.system_family)
            } else {
                match name {
                    "serif" => FontFamilyName::Generic(parley::GenericFamily::Serif),
                    "sans-serif" => FontFamilyName::Generic(parley::GenericFamily::SansSerif),
                    "monospace" => FontFamilyName::Generic(parley::GenericFamily::Monospace),
                    "system-ui" => FontFamilyName::Generic(parley::GenericFamily::SystemUi),
                    _ => FontFamilyName::named(name),
                }
            }
        };
        let mut result = vec![name(&font.family)];
        if let Some(fallbacks) = &font.fallbacks {
            result.extend(fallbacks.fallback_list().iter().map(|s| name(s)));
        }
        result.push(FontFamilyName::named(&self.system_family));
        result.push(FontFamilyName::Generic(parley::GenericFamily::SansSerif));
        result
    }
    pub(crate) fn build(
        &self,
        text: &str,
        runs: &[TextRun],
        font_size: f32,
        line_height: f32,
    ) -> Result<parley::Layout<TextBrush>> {
        self.build_with_line_height(text, runs, font_size, line_height, None)
    }
    /// A common native height prevents Parley 0.11.1 from assigning the next
    /// style's height to the previous run. Paragraph owns mixed-height geometry.
    pub(crate) fn build_with_line_height(
        &self,
        text: &str,
        runs: &[TextRun],
        font_size: f32,
        line_height: f32,
        native_height: Option<f32>,
    ) -> Result<parley::Layout<TextBrush>> {
        self.build_with_boxes(text, runs, font_size, line_height, native_height, &[])
    }
    pub(crate) fn build_with_boxes(
        &self,
        text: &str,
        runs: &[TextRun],
        font_size: f32,
        line_height: f32,
        native_height: Option<f32>,
        boxes: &[parley::InlineBox],
    ) -> Result<parley::Layout<TextBrush>> {
        let mut state = self.state.lock();
        let State { fonts, layout, .. } = &mut *state;
        let mut builder = layout.style_run_builder(fonts, text, 1.0, false);
        builder.reserve(runs.len(), runs.len());
        for item in boxes {
            builder.push_inline_box(item.clone());
        }
        let mut start = 0;
        for run in runs {
            let end = start + run.len;
            anyhow::ensure!(
                end <= text.len() && text.is_char_boundary(end),
                "invalid UTF-8 text run"
            );
            let families = self.families(&run.font);
            let features: Vec<_> = run
                .font
                .features
                .tag_value_list()
                .iter()
                .map(|(tag, value)| {
                    let bytes: [u8; 4] = tag.as_bytes().try_into().map_err(|_| {
                        anyhow::anyhow!("OpenType feature tags must contain four ASCII bytes")
                    })?;
                    anyhow::ensure!(
                        bytes.iter().all(|b| (0x20..=0x7e).contains(b)),
                        "invalid OpenType feature tag"
                    );
                    Ok(parley::FontFeature::new(
                        parley::setting::Tag::new(&bytes),
                        (*value)
                            .try_into()
                            .map_err(|_| anyhow::anyhow!("OpenType feature value exceeds 65535"))?,
                    ))
                })
                .collect::<Result<_>>()?;
            // CRLF normalization can remove an entire run containing only CR.
            // Validate its features above, but do not let an empty style range
            // affect the surviving LF or the following text.
            if start == end {
                continue;
            }
            let style = TextStyle {
                font_family: FontFamily::List(Cow::Borrowed(&families)),
                font_size: run.font_size.unwrap_or(font_size),
                font_weight: parley::FontWeight::new(run.font.weight.0),
                font_style: match run.font.style {
                    FontStyle::Normal => parley::FontStyle::Normal,
                    FontStyle::Italic => parley::FontStyle::Italic,
                    FontStyle::Oblique => parley::FontStyle::Oblique(None),
                },
                line_height: parley::LineHeight::Absolute(
                    native_height.unwrap_or_else(|| run.line_height.unwrap_or(line_height)),
                ),
                font_features: features.as_slice().into(),
                brush: TextBrush::from(run),
                ..Default::default()
            };
            let index = builder.push_style(style);
            builder.push_style_run(index, start..end);
            start = end;
        }
        anyhow::ensure!(
            start == text.len(),
            "text runs must cover the entire paragraph"
        );
        let layout = builder.build(text);
        self.shaped.fetch_add(1, Ordering::Relaxed);
        Ok(layout)
    }
    /// Resolve metrics through the same font selection, variations and shaping
    /// path as visible text. A cached space supplies the primary font even when
    /// an inline formatting context contains only widgets. No space is inserted
    /// into the document, so source indices and caret stops are unchanged.
    pub(crate) fn line_metrics(&self, font: &Font, size: f32) -> Result<parley::RunMetrics> {
        let key = (font.clone(), px(size));
        if let Some(metrics) = self.state.lock().line_metrics.get(&key).copied() {
            return Ok(metrics);
        }
        let revision = self.revision();
        let mut layout = self.build(
            " ",
            &[TextRun {
                len: 1,
                font: font.clone(),
                ..Default::default()
            }],
            size,
            size,
        )?;
        layout.break_all_lines(None);
        let metrics = layout
            .lines()
            .next()
            .and_then(|line| line.runs().next())
            .map(|run| *run.metrics())
            .ok_or_else(|| anyhow::anyhow!("no font available for {}", font.family))?;
        let mut state = self.state.lock();
        if self.revision() == revision {
            state.line_metrics.insert(key, metrics);
        }
        Ok(metrics)
    }
    pub fn font_id(&self, font: &Font) -> Result<FontId> {
        if let Some(id) = self.state.lock().resolved.get(font).copied() {
            return Ok(id);
        }
        let mut layout = self.build(
            "M",
            &[TextRun {
                len: 1,
                font: font.clone(),
                ..Default::default()
            }],
            16.0,
            20.0,
        )?;
        layout.break_all_lines(None);
        let run = layout
            .lines()
            .next()
            .and_then(|l| l.runs().next())
            .ok_or_else(|| anyhow::anyhow!("no font available for {}", font.family))?;
        let (id, _) = self.raster.register(&run);
        self.state.lock().resolved.insert(font.clone(), id);
        Ok(id)
    }
    pub fn get_font_for_id(&self, id: FontId) -> Option<Font> {
        self.state
            .lock()
            .resolved
            .iter()
            .find(|(_, v)| **v == id)
            .map(|(font, _)| font.clone())
    }
    pub fn font_metrics(&self, id: FontId) -> FontMetrics {
        self.raster.font_metrics(id)
    }
    pub fn typographic_bounds(&self, id: FontId, glyph: GlyphId) -> Result<Bounds<f32>> {
        Ok(Bounds::new(
            point(0.0, 0.0),
            self.raster.advance(id, glyph)?,
        ))
    }
    pub fn advance(&self, id: FontId, glyph: GlyphId) -> Result<Size<f32>> {
        self.raster.advance(id, glyph)
    }
    pub fn glyph_for_char(&self, id: FontId, ch: char) -> Option<GlyphId> {
        self.raster.glyph_for_char(id, ch)
    }
    pub fn glyph_raster_bounds(&self, p: &RenderGlyphParams) -> Result<Bounds<DevicePixels>> {
        self.raster.glyph_raster_bounds(p)
    }
    pub fn rasterize_glyph(
        &self,
        p: &RenderGlyphParams,
        b: Bounds<DevicePixels>,
    ) -> Result<(Size<DevicePixels>, Vec<u8>)> {
        self.raster.rasterize_glyph(p, b)
    }
    pub fn glyph_dilation_for_color(&self, _: Hsla) -> u8 {
        0
    }
    pub fn recommended_rendering_mode(&self, _: FontId, _: Pixels) -> TextRenderingMode {
        TextRenderingMode::Subpixel
    }
}
