//! Shared box painting and cached background-image preparation.
use crate::style::{color::Color, gradient::BackgroundImages, gradient::GradientItem};
use crate::{
    core::{geometry::Rect, layout::Layout},
    style::{gradient::BackgroundImage, paint::PaintStyle, text::TextStyle},
};
use std::cell::RefCell;
use voidui_gpui_wgpu::{Bounds, Corners, Edges, Hsla, Painter, fill, point, px, size};

pub(crate) fn render_bounds(bounds: Rect<f32>) -> Bounds<voidui_gpui_wgpu::Pixels> {
    Bounds::new(
        point(px(bounds.origin.x), px(bounds.origin.y)),
        size(px(bounds.size.width), px(bounds.size.height)),
    )
}

/// CSS paint order: outer shadows, background layers, inner shadows, border.
/// Shadows do not enlarge the layout box and are clipped only by ancestors.
pub(crate) fn paint_box(
    painter: &mut Painter<'_>,
    bounds: Rect<f32>,
    layout: &Layout,
    style: &PaintStyle,
    text: &TextStyle,
    cache: &PaintCache,
) {
    if bounds.is_empty() {
        return;
    }
    let box_bounds = render_bounds(bounds);
    let radius = style
        .border_radius
        .min(bounds.size.width.min(bounds.size.height) * 0.5)
        .max(0.0);
    let corners = Corners::all(px(radius));
    let background: Hsla = style.background.resolve(text.color).into();
    let border_color: Hsla = style.border_color.resolve(text.color).into();
    let b = layout.border;
    let has_border =
        border_color.a > 0.0 && (b.left > 0.0 || b.right > 0.0 || b.top > 0.0 || b.bottom > 0.0);
    let widths = Edges {
        left: px(b.left),
        right: px(b.right),
        top: px(b.top),
        bottom: px(b.bottom),
    };
    let shadow_pass = |painter: &mut Painter<'_>, inset| {
        for shadow in style.box_shadow.iter().rev().filter(|s| s.inset == inset) {
            let color: Hsla = shadow.color.resolve(text.color).into();
            if color.a <= 0.0 {
                continue;
            }
            let element = if inset {
                Bounds::new(
                    box_bounds.origin + point(px(b.left), px(b.top)),
                    size(
                        px((bounds.size.width - b.left - b.right).max(0.0)),
                        px((bounds.size.height - b.top - b.bottom).max(0.0)),
                    ),
                )
            } else {
                box_bounds
            };
            let inner_corners = if inset {
                Corners {
                    top_left: px((radius - b.left.max(b.top)).max(0.0)),
                    top_right: px((radius - b.right.max(b.top)).max(0.0)),
                    bottom_left: px((radius - b.left.max(b.bottom)).max(0.0)),
                    bottom_right: px((radius - b.right.max(b.bottom)).max(0.0)),
                }
            } else {
                corners
            };
            let spread = if inset { -shadow.spread } else { shadow.spread };
            let shape = Bounds::new(
                element.origin + point(px(shadow.offset_x - spread), px(shadow.offset_y - spread)),
                size(
                    px((f32::from(element.size.width) + 2.0 * spread).max(0.0)),
                    px((f32::from(element.size.height) + 2.0 * spread).max(0.0)),
                ),
            );
            if !inset && shape.is_empty() {
                continue;
            }
            let expand = |r: voidui_gpui_wgpu::Pixels| {
                let r = f32::from(r);
                // CSS adjusts small outer radii smoothly as positive spread grows.
                px((r + if spread > 0.0 && r < spread {
                    spread * (1.0 - (1.0 - r / spread).powi(3))
                } else {
                    spread
                })
                .max(0.0))
            };
            painter.paint_shadow(voidui_gpui_wgpu::PaintShadow {
                bounds: shape,
                corner_radii: inner_corners.map(|r| expand(*r)),
                element_bounds: element,
                element_corner_radii: inner_corners,
                sigma: px(shadow.blur * 0.5),
                color,
                inset,
            });
        }
    };
    shadow_pass(painter, false);
    let layered = style
        .background_image
        .iter()
        .any(|image| matches!(image, BackgroundImage::Gradient(_)))
        || style.box_shadow.iter().any(|s| s.inset);
    if background.a > 0.0 || (has_border && !layered) {
        let mut quad = fill(box_bounds, background);
        quad.corner_radii = corners;
        if !layered {
            quad.border_color = border_color;
            quad.border_widths = widths;
        }
        painter.paint_quad(quad);
    }
    cache.paint_images(
        painter,
        box_bounds,
        corners,
        b,
        &style.background_image,
        text.color,
    );
    shadow_pass(painter, true);
    if has_border && layered {
        let mut quad = fill(box_bounds, voidui_gpui_wgpu::transparent_black());
        quad.corner_radii = corners;
        quad.border_color = border_color;
        quad.border_widths = widths;
        painter.paint_quad(quad);
    }
}

/// Only elements that paint gradients allocate this cache. It keeps color-space
/// conversion and ramp subdivision out of unrelated paint animations (for example,
/// a changing border color), while still invalidating relative stops on resize.
#[derive(Default)]
pub(crate) struct PaintCache(RefCell<Option<Box<PreparedBackground>>>);
struct PreparedBackground {
    images: BackgroundImages,
    dimensions: [f32; 4],
    current: Option<Color>,
    gradients: Vec<voidui_gpui_wgpu::GradientPaint>,
}
impl PaintCache {
    fn paint_images(
        &self,
        painter: &mut Painter<'_>,
        bounds: Bounds<voidui_gpui_wgpu::Pixels>,
        corners: Corners<voidui_gpui_wgpu::Pixels>,
        border: crate::core::layout::Rect<f32>,
        images: &BackgroundImages,
        color: Color,
    ) {
        let mut cache = self.0.borrow_mut();
        if !images
            .iter()
            .any(|i| matches!(i, BackgroundImage::Gradient(_)))
        {
            *cache = None;
            return;
        }
        let width = (f32::from(bounds.size.width) - border.left - border.right).max(0.0);
        let height = (f32::from(bounds.size.height) - border.top - border.bottom).max(0.0);
        if width == 0.0 || height == 0.0 {
            *cache = None;
            return;
        }
        let dimensions = [width, height, border.left, border.top];
        if cache.as_ref().is_none_or(|c| {
            !c.images.ptr_eq(images)
                || c.dimensions != dimensions
                || c.current.is_some_and(|old| old != color)
        }) {
            let mut current = None;
            let mut gradients = Vec::new();
            for image in images.iter() {
                if let BackgroundImage::Gradient(g) = image {
                    if g.stops.iter().any(|s| {
                        matches!(
                            s,
                            GradientItem::Stop {
                                color: Color::CurrentColor,
                                ..
                            }
                        )
                    }) {
                        current = Some(color);
                    }
                    let mut prepared = g.prepare(width, height, color);
                    prepared.tile = Some(Bounds::new(
                        point(px(border.left), px(border.top)),
                        size(px(width), px(height)),
                    ));
                    gradients.push(prepared);
                }
            }
            *cache = Some(Box::new(PreparedBackground {
                images: images.clone(),
                dimensions,
                current,
                gradients,
            }));
        }
        for gradient in cache.as_ref().unwrap().gradients.iter().rev() {
            let mut quad = fill(bounds, voidui_gpui_wgpu::transparent_black());
            quad.corner_radii = corners;
            painter.paint_gradient(quad, gradient);
        }
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    use crate::style::gradient::Gradient;
    use std::{borrow::Cow, sync::Arc};
    use voidui_gpui_wgpu::*;
    struct NoGlyphs;
    impl PlatformAtlas for NoGlyphs {
        fn get_or_insert_with<'a>(
            &self,
            _: &AtlasKey,
            _: &mut dyn FnMut() -> Result<Option<(Size<DevicePixels>, Cow<'a, [u8]>)>>,
        ) -> Result<Option<AtlasTile>> {
            panic!("gradient must not rasterize a glyph")
        }
        fn remove(&self, _: &AtlasKey) {}
    }
    fn draw(cache: &PaintCache, images: &BackgroundImages, color: Color, width: f32) {
        let text = Arc::new(TextSystem::new(Arc::new(
            ParleyTextSystem::new_without_system_fonts("unused"),
        )));
        let mut scene = Scene::default();
        let mut painter =
            Painter::new(&mut scene, &NoGlyphs, text, size(px(300.0), px(100.0)), 1.0).unwrap();
        cache.paint_images(
            &mut painter,
            Bounds::new(point(px(0.0), px(0.0)), size(px(width), px(100.0))),
            Corners::default(),
            Default::default(),
            images,
            color,
        );
    }
    #[test]
    fn immutable_gradients_reuse_ramps_until_size_or_current_color_changes() {
        let cache = PaintCache::default();
        let red = "red".parse::<Color>().unwrap();
        let blue = "blue".parse::<Color>().unwrap();
        let images: BackgroundImages = "linear-gradient(in srgb,red,blue 100px)"
            .parse::<Gradient>()
            .unwrap()
            .into();
        draw(&cache, &images, red, 200.0);
        let pointer = cache.0.borrow().as_ref().unwrap().gradients.as_ptr();
        draw(&cache, &images, blue, 200.0);
        assert_eq!(
            pointer,
            cache.0.borrow().as_ref().unwrap().gradients.as_ptr()
        );
        draw(&cache, &images, red, 100.0);
        assert_eq!(cache.0.borrow().as_ref().unwrap().dimensions[0], 100.0);
        let images: BackgroundImages = "linear-gradient(in srgb,currentColor,transparent)"
            .parse::<Gradient>()
            .unwrap()
            .into();
        draw(&cache, &images, red, 200.0);
        draw(&cache, &images, blue, 200.0);
        assert_eq!(cache.0.borrow().as_ref().unwrap().current, Some(blue));
        draw(&cache, &BackgroundImages::default(), red, 200.0);
        assert!(cache.0.borrow().is_none());
    }
}
