//! Shared Parley font/layout resources and size-independent glyph raster caches.
mod cache;
mod font;
mod font_fallbacks;
mod font_features;
mod paragraph;
mod paragraph_inline;
mod paragraph_rows;
mod source;
use crate::*;
use anyhow::Context as _;
pub use cache::*;
pub use font::*;
pub use font_fallbacks::*;
pub use font_features::*;
pub use paragraph::*;
pub use paragraph_inline::{InlineAlignment, InlineTextStyle};
use parking_lot::{RwLock, RwLockUpgradableReadGuard};
use rustc_hash::FxHashMap;
use std::{borrow::Cow, sync::Arc};
#[derive(Hash, PartialEq, Eq, Clone, Copy, Debug)]
#[repr(C)]
pub struct FontId(pub usize);
pub const SUBPIXEL_VARIANTS_X: u8 = 4;
pub const SUBPIXEL_VARIANTS_Y: u8 = 1;
pub struct TextSystem {
    pub(crate) backend: Arc<ParleyTextSystem>,
    font_metrics: RwLock<FxHashMap<FontId, FontMetrics>>,
    raster_bounds: parking_lot::Mutex<
        crate::resource_cache::BudgetCache<RenderGlyphParams, Bounds<DevicePixels>>,
    >,
}
impl TextSystem {
    pub fn new(backend: Arc<ParleyTextSystem>) -> Self {
        Self {
            backend,
            font_metrics: Default::default(),
            raster_bounds: parking_lot::Mutex::new(crate::resource_cache::BudgetCache::new(
                crate::resource_cache::CacheBudget {
                    max_bytes: 4 * 1024 * 1024,
                    max_entries: 32768,
                },
            )),
        }
    }
    pub fn all_font_names(&self) -> Vec<String> {
        self.backend.all_font_names()
    }
    pub fn add_fonts(&self, fonts: Vec<Cow<'static, [u8]>>) -> Result<()> {
        self.backend.add_fonts(fonts)
    }
    /// Share lazily loaded application fonts with text, widgets and embedded
    /// views. Successful resources are registered once for this font backend.
    /// Select resources by family name; generic UI font defaults are preserved.
    /// The loader must not call back into this service.
    pub fn add_fonts_once(
        &self,
        key: &str,
        load: impl FnOnce() -> Result<Vec<Cow<'static, [u8]>>>,
    ) -> Result<()> {
        self.backend.add_fonts_once(key, load)
    }
    /// Register one immutable face using declared family, weight and style
    /// instead of possibly incomplete font-file metadata. Shares the named
    /// resource cache and font revision with all other text in the application.
    pub fn add_font_face_once(
        &self,
        key: &str,
        face: &Font,
        load: impl FnOnce() -> Result<Cow<'static, [u8]>>,
    ) -> Result<()> {
        self.backend.add_font_face_once(key, face, load)
    }
    pub fn resolve_font(&self, font: &Font) -> FontId {
        self.backend
            .font_id(font)
            .expect("failed to resolve a font")
    }
    pub fn get_font_for_id(&self, id: FontId) -> Option<Font> {
        self.backend.get_font_for_id(id)
    }
    pub fn prewarm_fonts(&self, fonts: &[Font]) {
        for font in fonts {
            self.resolve_font(font);
        }
    }
    /// Revision of registered fonts for independently retained editor layouts.
    pub fn font_revision(&self) -> u64 {
        self.backend.revision()
    }
    pub fn stats(&self) -> TextSystemStats {
        self.backend.stats()
    }
    /// Get the bounding box for the given font and font size.
    /// A font's bounding box is the smallest rectangle that could enclose all glyphs
    /// in the font. superimposed over one another.
    pub fn bounding_box(&self, font_id: FontId, font_size: Pixels) -> Bounds<Pixels> {
        self.read_metrics(font_id, |metrics| metrics.bounding_box(font_size))
    }

    /// Get the typographic bounds for the given character, in the given font and size.
    pub fn typographic_bounds(
        &self,
        font_id: FontId,
        font_size: Pixels,
        character: char,
    ) -> Result<Bounds<Pixels>> {
        let glyph_id = self
            .backend
            .glyph_for_char(font_id, character)
            .with_context(|| format!("glyph not found for character '{character}'"))?;
        let bounds = self.backend.typographic_bounds(font_id, glyph_id)?;
        Ok(self.read_metrics(font_id, |metrics| {
            (bounds / metrics.units_per_em as f32 * font_size.0).map(px)
        }))
    }

    /// Get the advance width for the given character, in the given font and size.
    pub fn advance(&self, font_id: FontId, font_size: Pixels, ch: char) -> Result<Size<Pixels>> {
        let glyph_id = self
            .backend
            .glyph_for_char(font_id, ch)
            .with_context(|| format!("glyph not found for character '{ch}'"))?;
        let result = self.backend.advance(font_id, glyph_id)? / self.units_per_em(font_id) as f32;

        Ok(result * font_size)
    }

    /// Returns the width of an `em`.
    ///
    /// Uses the width of the `m` character in the given font and size.
    pub fn em_width(&self, font_id: FontId, font_size: Pixels) -> Result<Pixels> {
        Ok(self.typographic_bounds(font_id, font_size, 'm')?.size.width)
    }

    /// Returns the advance width of an `em`.
    ///
    /// Uses the advance width of the `m` character in the given font and size.
    pub fn em_advance(&self, font_id: FontId, font_size: Pixels) -> Result<Pixels> {
        Ok(self.advance(font_id, font_size, 'm')?.width)
    }

    /// Returns the width of an `ch`.
    ///
    /// Uses the width of the `0` character in the given font and size.
    pub fn ch_width(&self, font_id: FontId, font_size: Pixels) -> Result<Pixels> {
        Ok(self.typographic_bounds(font_id, font_size, '0')?.size.width)
    }

    /// Returns the advance width of an `ch`.
    ///
    /// Uses the advance width of the `0` character in the given font and size.
    pub fn ch_advance(&self, font_id: FontId, font_size: Pixels) -> Result<Pixels> {
        Ok(self.advance(font_id, font_size, '0')?.width)
    }

    /// Get the number of font size units per 'em square',
    /// Per MDN: "an abstract square whose height is the intended distance between
    /// lines of type in the same type size"
    pub fn units_per_em(&self, font_id: FontId) -> u32 {
        self.read_metrics(font_id, |metrics| metrics.units_per_em)
    }

    /// Get the height of a capital letter in the given font and size.
    pub fn cap_height(&self, font_id: FontId, font_size: Pixels) -> Pixels {
        self.read_metrics(font_id, |metrics| metrics.cap_height(font_size))
    }

    /// Get the height of the x character in the given font and size.
    pub fn x_height(&self, font_id: FontId, font_size: Pixels) -> Pixels {
        self.read_metrics(font_id, |metrics| metrics.x_height(font_size))
    }

    /// Get the recommended distance from the baseline for the given font
    pub fn ascent(&self, font_id: FontId, font_size: Pixels) -> Pixels {
        self.read_metrics(font_id, |metrics| metrics.ascent(font_size))
    }

    /// Get the recommended distance below the baseline for the given font,
    /// in single spaced text.
    pub fn descent(&self, font_id: FontId, font_size: Pixels) -> Pixels {
        self.read_metrics(font_id, |metrics| metrics.descent(font_size))
    }

    /// Get the recommended baseline offset for the given font and line height.
    pub fn baseline_offset(
        &self,
        font_id: FontId,
        font_size: Pixels,
        line_height: Pixels,
    ) -> Pixels {
        let ascent = self.ascent(font_id, font_size);
        let descent = self.descent(font_id, font_size);
        let padding_top = (line_height - ascent - descent) / 2.;
        padding_top + ascent
    }

    fn read_metrics<T>(&self, font_id: FontId, read: impl FnOnce(&FontMetrics) -> T) -> T {
        let lock = self.font_metrics.upgradable_read();

        if let Some(metrics) = lock.get(&font_id) {
            read(metrics)
        } else {
            let mut lock = RwLockUpgradableReadGuard::upgrade(lock);
            let metrics = lock
                .entry(font_id)
                .or_insert_with(|| self.backend.font_metrics(font_id));
            read(metrics)
        }
    }

    /// Get the rasterized size and location of a specific, rendered glyph.
    /// Bounds are cheap metadata and can be recomputed without invalidating atlas tiles.
    pub fn set_raster_bounds_limit(&self, entries: usize) {
        self.raster_bounds
            .lock()
            .set_budget(crate::resource_cache::CacheBudget {
                max_bytes: 4 * 1024 * 1024,
                max_entries: entries,
            });
    }
    pub fn raster_bounds(&self, params: &RenderGlyphParams) -> Result<Bounds<DevicePixels>> {
        let mut cache = self.raster_bounds.lock();
        if let Some(bounds) = cache.get(params) {
            return Ok(*bounds);
        }
        let bounds = self.backend.glyph_raster_bounds(params)?;
        cache.insert(
            params.clone(),
            bounds,
            std::mem::size_of_val(params) + std::mem::size_of_val(&bounds),
        );
        Ok(bounds)
    }

    pub fn rasterize_glyph(
        &self,
        params: &RenderGlyphParams,
    ) -> Result<(Size<DevicePixels>, Vec<u8>)> {
        let raster_bounds = self.raster_bounds(params)?;
        self.backend.rasterize_glyph(params, raster_bounds)
    }

    /// Returns the dilation level to use for a glyph painted in the given color.
    pub fn glyph_dilation_for_color(&self, color: Hsla) -> u8 {
        self.backend.glyph_dilation_for_color(color)
    }

    /// Returns the text rendering mode recommended by the platform for the given font and size.
    /// The return value will never be [`TextRenderingMode::PlatformDefault`].
    pub fn recommended_rendering_mode(
        &self,
        font_id: FontId,
        font_size: Pixels,
    ) -> TextRenderingMode {
        self.backend.recommended_rendering_mode(font_id, font_size)
    }
}
