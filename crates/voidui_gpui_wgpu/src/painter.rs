//! Logical-pixel scene writer replacing GPUI's Window/App paint entry points.
use crate::*;
use std::{borrow::Cow, sync::Arc};

#[derive(Clone, Debug)]
pub struct PaintQuad {
    pub bounds: Bounds<Pixels>,
    pub background: Background,
    pub corner_radii: Corners<Pixels>,
    pub border_widths: Edges<Pixels>,
    pub border_color: Hsla,
    pub border_style: BorderStyle,
}
/// Logical-pixel shadow geometry. Outer shadows exclude `element_bounds`;
/// inset shadows treat `bounds` as a hole and clip to `element_bounds`.
#[derive(Debug, Clone, Copy)]
pub struct PaintShadow {
    pub bounds: Bounds<Pixels>,
    pub corner_radii: Corners<Pixels>,
    pub element_bounds: Bounds<Pixels>,
    pub element_corner_radii: Corners<Pixels>,
    /// Gaussian standard deviation (half the CSS blur radius).
    pub sigma: Pixels,
    pub color: Hsla,
    pub inset: bool,
}

pub fn fill(bounds: Bounds<Pixels>, color: impl Into<Background>) -> PaintQuad {
    PaintQuad {
        bounds,
        background: color.into(),
        corner_radii: Default::default(),
        border_widths: Default::default(),
        border_color: Default::default(),
        border_style: Default::default(),
    }
}

/// Writes logical-pixel drawing operations into a device-scaled scene.
/// The atlas must belong to the renderer that will draw this scene.
/// Rebuild scenes after device recovery/atlas clearing; old tile IDs become invalid.
pub struct Painter<'a> {
    pub(crate) scene: &'a mut Scene,
    pub(crate) atlas: &'a dyn PlatformAtlas,
    text_system: Arc<TextSystem>,
    pub(crate) scale_factor: f32,
    pub(crate) mask: ContentMask<Pixels>,
}
impl<'a> Painter<'a> {
    pub fn new(
        scene: &'a mut Scene,
        atlas: &'a dyn PlatformAtlas,
        text_system: Arc<TextSystem>,
        viewport: Size<Pixels>,
        scale_factor: f32,
    ) -> Result<Self> {
        anyhow::ensure!(
            scale_factor.is_finite() && scale_factor > 0.,
            "scale_factor must be finite and positive"
        );
        anyhow::ensure!(
            f32::from(viewport.width).is_finite()
                && f32::from(viewport.height).is_finite()
                && viewport.width >= px(0.)
                && viewport.height >= px(0.),
            "invalid viewport"
        );
        Ok(Self {
            scene,
            atlas,
            text_system,
            scale_factor,
            mask: ContentMask {
                bounds: Bounds::new(point(px(0.), px(0.)), viewport),
            },
        })
    }
    pub fn text_system(&self) -> &Arc<TextSystem> {
        &self.text_system
    }
    pub fn content_mask(&self) -> ContentMask<Pixels> {
        self.mask
    }
    pub fn with_clip<R>(&mut self, bounds: Bounds<Pixels>, f: impl FnOnce(&mut Self) -> R) -> R {
        let previous = self.mask;
        self.mask = self.mask.intersect(&ContentMask { bounds });
        let result = f(self);
        self.mask = previous;
        result
    }
    /// Paint in a shared affine coordinate space without offscreen textures.
    /// Widget-local clips added inside the callback follow the same transform.
    pub fn with_space<R>(&mut self, space: &PaintSpace, f: impl FnOnce(&mut Self) -> R) -> R {
        let previous = self.scene.spatial_id;
        let mask = self.mask;
        let viewport = SpatialClip {
            inverse: Affine::IDENTITY,
            bounds: [
                mask.bounds.origin.x.0,
                mask.bounds.origin.y.0,
                mask.bounds.size.width.0,
                mask.bounds.size.height.0,
            ],
            axes: [true, true],
        };
        self.scene.spatial_id = self.scene.spatial.push(space, self.scale_factor, viewport);
        // A conservative local viewport keeps existing widget culling safe.
        if let Some(inverse) = space.inverse {
            self.mask.bounds = inverse.map_bounds(mask.bounds.scale(1.)).map(|v| px(v.0));
        }
        let result = f(self);
        self.mask = mask;
        self.scene.spatial_id = previous;
        result
    }
    pub fn paint_layer<R>(&mut self, bounds: Bounds<Pixels>, f: impl FnOnce(&mut Self) -> R) -> R {
        let bounds = bounds.intersect(&self.mask.bounds).scale(self.scale_factor);
        let bounds = if self.scene.spatial_id == 0 {
            bounds
        } else {
            self.scene
                .spatial
                .transform(self.scene.spatial_id)
                .map_bounds(bounds)
        };
        self.scene.push_layer(bounds);
        let result = f(self);
        self.scene.pop_layer();
        result
    }
    /// Retain caret ink separately from its visibility. Publication preserves its
    /// ordinary stacking/clip position; toggling never moves it above a popover.
    pub fn paint_caret(&mut self, quad: PaintQuad, visible: bool) {
        let before = self.scene.quads.len();
        self.paint_quad(quad);
        if self.scene.quads.len() == before {
            return;
        }
        if let Some(last) = self.scene.quads.last_mut() {
            last.spatial_pad = 1;
        }
        if let Some(crate::scene::PaintOperation::Primitive(crate::Primitive::Quad(last))) =
            self.scene.paint_operations.last_mut()
        {
            last.spatial_pad = 1;
        }
        self.scene.caret_visible = visible;
    }
    pub fn paint_quad(&mut self, quad: PaintQuad) {
        self.scene.insert_primitive(Quad {
            order: 0,
            spatial_id: 0,
            spatial_pad: 0,
            bounds: quad.bounds.scale(self.scale_factor),
            content_mask: self.mask.scale(self.scale_factor),
            background: quad.background,
            corner_radii: quad.corner_radii.scale(self.scale_factor),
            border_widths: quad.border_widths.scale(self.scale_factor),
            border_color: quad.border_color,
            border_style: quad.border_style,
        });
    }
    /// Paint a variable-length gradient using the same rounded-quad clipping as solids.
    pub fn paint_gradient(&mut self, mut quad: PaintQuad, gradient: &GradientPaint) {
        if gradient.stops.is_empty() {
            return;
        }
        quad.background = Background::css_gradient(gradient.encode(
            quad.bounds.origin,
            self.scale_factor,
            &mut self.scene.gradient_data,
        ));
        self.paint_quad(quad);
    }

    /// Paint a CSS shadow. All lengths, including Gaussian sigma, are logical pixels.
    pub fn paint_shadow(&mut self, shadow: PaintShadow) {
        self.scene.insert_primitive(Shadow {
            order: 0,
            spatial_id: 0,
            spatial_pad: 0,
            blur_radius: shadow.sigma.scale(self.scale_factor),
            bounds: shadow.bounds.scale(self.scale_factor),
            corner_radii: shadow.corner_radii.scale(self.scale_factor),
            content_mask: self.mask.scale(self.scale_factor),
            color: shadow.color,
            element_bounds: shadow.element_bounds.scale(self.scale_factor),
            element_corner_radii: shadow.element_corner_radii.scale(self.scale_factor),
            inset: u32::from(shadow.inset),
            pad: 0,
        });
    }
    pub fn paint_path(&mut self, mut path: Path<Pixels>, color: impl Into<Background>) {
        path.content_mask = self.mask;
        path.color = color.into();
        self.scene.insert_primitive(path.scale(self.scale_factor));
    }
    pub fn paint_underline(
        &mut self,
        origin: Point<Pixels>,
        width: Pixels,
        style: &UnderlineStyle,
    ) {
        let height = if style.wavy {
            style.thickness * 3.
        } else {
            style.thickness
        };
        self.scene.insert_primitive(Underline {
            order: 0,
            spatial_id: 0,
            spatial_pad: 0,
            pad: 0,
            bounds: Bounds::new(origin, size(width, height)).scale(self.scale_factor),
            content_mask: self.mask.scale(self.scale_factor),
            thickness: style.thickness.scale(self.scale_factor),
            color: style.color.unwrap_or_default(),
            wavy: style.wavy.into(),
        });
    }
    pub fn paint_strikethrough(
        &mut self,
        origin: Point<Pixels>,
        width: Pixels,
        style: &StrikethroughStyle,
    ) {
        self.paint_underline(
            origin,
            width,
            &UnderlineStyle {
                thickness: style.thickness,
                color: style.color,
                wavy: false,
            },
        );
    }
    /// Paint one glyph at a baseline origin. Uses grayscale AA, safe on transparent windows.
    pub fn paint_glyph(
        &mut self,
        origin: Point<Pixels>,
        font_id: FontId,
        glyph_id: GlyphId,
        font_size: Pixels,
        color: Hsla,
    ) -> Result<()> {
        self.paint_glyph_inner(origin, font_id, glyph_id, font_size, color, false)
    }
    pub fn paint_emoji(
        &mut self,
        origin: Point<Pixels>,
        font_id: FontId,
        glyph_id: GlyphId,
        font_size: Pixels,
    ) -> Result<()> {
        self.paint_glyph_inner(origin, font_id, glyph_id, font_size, black(), true)
    }
    fn paint_glyph_inner(
        &mut self,
        origin: Point<Pixels>,
        font_id: FontId,
        glyph_id: GlyphId,
        font_size: Pixels,
        color: Hsla,
        emoji: bool,
    ) -> Result<()> {
        let scaled = origin.scale(self.scale_factor);
        // Floor-based quantization also handles glyphs left/above the viewport correctly.
        let quantize = |v: f32, variants: u8| (v * variants as f32).round() / variants as f32;
        let q = point(
            quantize(scaled.x.0, if emoji { 1 } else { SUBPIXEL_VARIANTS_X }),
            quantize(scaled.y.0, SUBPIXEL_VARIANTS_Y),
        );
        let params = RenderGlyphParams {
            font_id,
            glyph_id,
            font_size,
            subpixel_variant: point(
                ((q.x - q.x.floor()) * SUBPIXEL_VARIANTS_X as f32) as u8,
                ((q.y - q.y.floor()) * SUBPIXEL_VARIANTS_Y as f32) as u8,
            ),
            scale_factor: self.scale_factor,
            is_emoji: emoji,
            subpixel_rendering: false,
            dilation: self.text_system.glyph_dilation_for_color(color),
        };
        let raster = self.text_system.raster_bounds(&params)?;
        if raster.size.width.0 <= 0 || raster.size.height.0 <= 0 {
            return Ok(());
        }
        let tile = self
            .atlas
            .get_or_insert_with(&params.clone().into(), &mut || {
                let (size, bytes) = self.text_system.rasterize_glyph(&params)?;
                Ok(Some((size, Cow::Owned(bytes))))
            })?
            .ok_or_else(|| anyhow::anyhow!("glyph atlas allocation returned no tile"))?;
        if let Some(lease) = self.atlas.pin(&params.into()) {
            self.scene.atlas_leases.push((self.scene.len(), lease));
        }
        let bounds = Bounds {
            origin: point(ScaledPixels(q.x.floor()), ScaledPixels(q.y.floor()))
                + raster.origin.map(Into::into),
            size: tile.bounds.size.map(Into::into),
        };
        let content_mask = self.mask.scale(self.scale_factor);
        if emoji {
            self.scene.insert_primitive(PolychromeSprite {
                order: 0,
                spatial_id: 0,
                spatial_pad: 0,
                pad: 0,
                grayscale: false.into(),
                opacity: 1.,
                bounds,
                content_mask,
                corner_radii: Default::default(),
                rounded_bounds: bounds,
                tile,
            });
        } else {
            self.scene.insert_primitive(MonochromeSprite {
                order: 0,
                spatial_id: 0,
                spatial_pad: 0,
                pad: 0,
                bounds,
                content_mask,
                color,
                tile,
                transformation: TransformationMatrix::unit(),
            });
        }
        Ok(())
    }
}
