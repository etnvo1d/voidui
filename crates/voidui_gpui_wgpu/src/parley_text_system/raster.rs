//! Shared font-face identities and Swash rasterization for Parley glyph runs.
//! Bounds and image generation share one pending bitmap; the atlas upload consumes it.
use crate::*;
use parking_lot::Mutex;
use read_fonts::{
    TableProvider as _,
    tables::{head::MacStyle, os2::SelectionFlags},
    types::Tag,
};
use swash::{
    CacheKey, FontRef,
    scale::{
        Render, ScaleContext, Source, StrikeWith,
        image::{Content, Image},
    },
    zeno::{Format, Vector},
};
#[derive(Default)]
pub struct Raster(Mutex<State>);
#[derive(Default)]
struct State {
    fonts: Vec<Face>,
    scale: ScaleContext,
    // Bounds are normally followed immediately by an atlas upload. Keep only
    // that image, rather than retaining bitmaps for arbitrary bounds-only queries.
    pending: Option<(RenderGlyphParams, Image)>,
}
struct Face {
    font: parley::FontData,
    coords: Vec<i16>,
    key: CacheKey,
    emoji: bool,
    can_embolden: bool,
    synthesis: RasterSynthesis,
}

/// Cache the transforms actually applied, so requests that select the same
/// real bold face share glyphs even when Fontique suggests extra emboldening.
#[derive(Clone, Copy, PartialEq)]
struct RasterSynthesis {
    embolden: bool,
    skew: Option<f32>,
}

impl RasterSynthesis {
    fn for_run(run: &parley::Run<'_, TextBrush>, can_embolden: bool) -> Self {
        let suggested = run.synthesis();
        Self {
            // A missing medium weight is not a request for synthetic bold.
            embolden: can_embolden
                && run.font_attrs().weight.value() > FontWeight::MEDIUM.0
                && suggested.embolden(),
            skew: suggested.skew(),
        }
    }
}

fn can_embolden(font: &parley::FontData) -> bool {
    let Ok(font) = read_fonts::FontRef::from_index(font.data.as_ref(), font.index) else {
        return false;
    };
    // Use portable font metadata, not family names or platform-specific APIs.
    // Semibold is already a bold face even when its legacy bold flags are unset.
    let bold = font.os2().is_ok_and(|table| {
        f32::from(table.us_weight_class()) >= FontWeight::SEMIBOLD.0
            || table.fs_selection().contains(SelectionFlags::BOLD)
    }) || font
        .head()
        .is_ok_and(|table| table.mac_style().contains(MacStyle::BOLD));
    // Variable weight is supplied through normalized coordinates. Never add a
    // faux outline on top of an instance, including at the axis limits.
    let variable_weight = font.fvar().is_ok_and(|table| {
        table
            .axes()
            .is_ok_and(|axes| axes.iter().any(|axis| axis.axis_tag() == Tag::new(b"wght")))
    });
    !bold && !variable_weight
}
impl Face {
    fn as_swash(&self) -> FontRef<'_> {
        let mut f = FontRef::from_index(self.font.data.as_ref(), self.font.index as usize).unwrap();
        f.key = self.key;
        f
    }
}
impl Raster {
    pub(crate) fn len(&self) -> usize {
        self.0.lock().fonts.len()
    }
    pub fn register(&self, run: &parley::Run<'_, TextBrush>) -> (FontId, bool) {
        let font = run.font();
        let coords = run.normalized_coords();
        let mut s = self.0.lock();
        if let Some((i, f)) = s.fonts.iter().enumerate().find(|(_, f)| {
            f.font.data.id() == font.data.id()
                && f.font.index == font.index
                && f.coords == coords
                && f.synthesis == RasterSynthesis::for_run(run, f.can_embolden)
        }) {
            return (FontId(i), f.emoji);
        }
        let f = FontRef::from_index(font.data.as_ref(), font.index as usize).unwrap();
        let emoji = [*b"sbix", *b"CBDT", *b"COLR"]
            .iter()
            .any(|tag| f.table(u32::from_be_bytes(*tag)).is_some());
        let id = FontId(s.fonts.len());
        let can_embolden = can_embolden(font);
        s.fonts.push(Face {
            font: font.clone(),
            coords: coords.to_vec(),
            key: f.key,
            emoji,
            can_embolden,
            synthesis: RasterSynthesis::for_run(run, can_embolden),
        });
        (id, emoji)
    }
}
impl State {
    fn image(&mut self, p: &RenderGlyphParams) -> anyhow::Result<Image> {
        let face = &self.fonts[p.font_id.0];
        let font = face.as_swash();
        let mut scaler = self
            .scale
            .builder(font)
            .size(f32::from(p.font_size) * p.scale_factor)
            .hint(true)
            .normalized_coords(face.coords.iter().copied())
            .build();
        let sources: &[Source] = if p.is_emoji {
            &[
                Source::ColorOutline(0),
                Source::ColorBitmap(StrikeWith::BestFit),
                Source::Outline,
            ]
        } else {
            &[Source::Bitmap(StrikeWith::ExactSize), Source::Outline]
        };
        let mut render = Render::new(sources);
        if face.synthesis.embolden {
            // Match FreeType's 1/24-em total stroke growth. Swash takes a
            // per-side offset, so passing the full growth doubles the weight.
            const SYNTHETIC_BOLD_SIDE_OFFSET_EM: f32 = 1.0 / 48.0;
            render
                .embolden(f32::from(p.font_size) * p.scale_factor * SYNTHETIC_BOLD_SIDE_OFFSET_EM);
        }
        if let Some(degrees) = face.synthesis.skew {
            render.transform(Some(swash::zeno::Transform::new(
                1.0,
                0.0,
                degrees.to_radians().tan(),
                1.0,
                0.0,
                0.0,
            )));
        }
        render
            .format(if p.subpixel_rendering {
                Format::subpixel_bgra()
            } else {
                Format::Alpha
            })
            .offset(Vector::new(
                p.subpixel_variant.x as f32 / SUBPIXEL_VARIANTS_X as f32 / p.scale_factor,
                p.subpixel_variant.y as f32 / SUBPIXEL_VARIANTS_Y as f32 / p.scale_factor,
            ));
        render
            .render(&mut scaler, p.glyph_id.0.try_into()?)
            .ok_or_else(|| anyhow::anyhow!("cannot rasterize {p:?}"))
    }
}
impl Raster {
    pub(crate) fn font_metrics(&self, id: FontId) -> FontMetrics {
        let s = self.0.lock();
        let f = &s.fonts[id.0];
        let m = f.as_swash().metrics(&f.coords);
        FontMetrics {
            units_per_em: m.units_per_em as u32,
            ascent: m.ascent,
            descent: -m.descent,
            line_gap: m.leading,
            underline_position: m.underline_offset,
            underline_thickness: m.stroke_size,
            cap_height: m.cap_height,
            x_height: m.x_height,
            bounding_box: Bounds::new(point(0.0, 0.0), size(m.max_width, m.ascent + m.descent)),
        }
    }
    pub(crate) fn advance(&self, id: FontId, g: GlyphId) -> anyhow::Result<Size<f32>> {
        let s = self.0.lock();
        let f = &s.fonts[id.0];
        let m = f.as_swash().glyph_metrics(&f.coords);
        Ok(size(
            m.advance_width(g.0 as u16),
            m.advance_height(g.0 as u16),
        ))
    }
    pub(crate) fn glyph_for_char(&self, id: FontId, ch: char) -> Option<GlyphId> {
        let s = self.0.lock();
        let g = s.fonts[id.0].as_swash().charmap().map(ch);
        (g != 0).then_some(GlyphId(g as u32))
    }
    pub(crate) fn glyph_raster_bounds(
        &self,
        p: &RenderGlyphParams,
    ) -> anyhow::Result<Bounds<DevicePixels>> {
        let mut s = self.0.lock();
        let image = s.image(p)?;
        let bounds = Bounds::new(
            point(
                DevicePixels(image.placement.left),
                DevicePixels(-image.placement.top),
            ),
            size(
                DevicePixels(image.placement.width as i32),
                DevicePixels(image.placement.height as i32),
            ),
        );
        s.pending =
            (image.placement.width > 0 && image.placement.height > 0).then(|| (p.clone(), image));
        Ok(bounds)
    }
    pub(crate) fn rasterize_glyph(
        &self,
        p: &RenderGlyphParams,
        bounds: Bounds<DevicePixels>,
    ) -> anyhow::Result<(Size<DevicePixels>, Vec<u8>)> {
        let mut s = self.0.lock();
        let mut image = match s.pending.take().filter(|(key, _)| key == p) {
            Some((_, i)) => i,
            None => s.image(p)?,
        };
        match image.content {
            Content::Color | Content::SubpixelMask => {
                for pixel in image.data.as_chunks_mut::<4>().0 {
                    pixel.swap(0, 2);
                }
            }
            Content::Mask if p.subpixel_rendering || p.is_emoji => {
                image.data = image.data.iter().flat_map(|&a| [a, a, a, a]).collect();
            }
            _ => {}
        }
        Ok((bounds.size, image.data))
    }
}
