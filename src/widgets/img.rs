//! Replaced images: CSS controls the box and object placement; shared resources
//! own decoded pixels independently of the number of widgets using them.
use crate::media::view::*;
use crate::{
    core::{
        context::{DrawContext, LayoutContext},
        element::ElementProps,
        layout::{LayoutInput, LayoutOutput},
        widget::{Widget, WidgetBuilder, WidgetUpdate},
    },
    media::{Image, Raster, SvgRenderCache, SvgRenderPolicy},
    render::{self, Painter, Result},
    style::{media::object_rect, style::Style},
};
use std::{cell::RefCell, sync::Arc};

pub struct Img {
    source: Image,
    policy: SvgRenderPolicy,
    background: RefCell<Option<SvgRenderCache>>,
    raster: RefCell<Option<Arc<Raster>>>,
}
/// Display a shared image. Load with Image::from_file/from_bytes outside rendering.
pub fn img(source: Image) -> WidgetBuilder<Img> {
    WidgetBuilder {
        events: Default::default(),
        widget: Img {
            source,
            policy: SvgRenderPolicy::default(),
            background: RefCell::default(),
            raster: RefCell::new(None),
        },
        props: ElementProps::new(Style::default()),
        children: Vec::new(),
    }
}
impl WidgetBuilder<Img> {
    /// Choose how SVG images refresh their raster. Bitmap images are unaffected.
    pub fn render_policy(mut self, policy: SvgRenderPolicy) -> Self {
        self.widget.policy = policy;
        self.widget.background = RefCell::default();
        self
    }

    pub fn src(mut self, source: Image) -> Self {
        self.widget.source = source;
        self.widget.background = RefCell::default();
        self.widget.raster = RefCell::new(None);
        self
    }
    /// Expose alternative text as a selector-visible standard HTML attribute.
    pub fn alt(self, text: impl Into<winit::keyboard::SmolStr>) -> Self {
        self.attr("alt", text)
    }
}
impl Widget for Img {
    fn tag_name(&self) -> &'static str {
        "img"
    }
    fn reconcile(&mut self, next: &dyn Widget) -> WidgetUpdate {
        let Some(next) = (next as &dyn std::any::Any).downcast_ref::<Self>() else {
            return WidgetUpdate::Replace;
        };
        if self.source == next.source && self.policy == next.policy {
            return WidgetUpdate::Unchanged;
        }
        self.source = next.source.clone();
        self.policy = next.policy;
        self.background = RefCell::default();
        self.raster.get_mut().take();
        WidgetUpdate::Changed
    }
    fn layout(&mut self, input: LayoutInput, ctx: LayoutContext<'_, '_>) -> LayoutOutput {
        replaced_layout(ctx.layout_style(), input, self.source.intrinsic_size())
    }
    fn draw(&self, painter: &mut Painter<'_>, ctx: DrawContext) -> Result<()> {
        let media = ctx.media_style();
        let b = ctx.content_bounds;
        if b.size.width <= 0. || b.size.height <= 0. {
            self.raster.borrow_mut().take();
            return Ok(());
        }
        let [x, y, w, h] = object_rect(
            [b.size.width, b.size.height],
            self.source.intrinsic_size(),
            media.object_fit(),
            media.object_position(),
        );
        let bounds = render::Bounds::new(
            render::point(render::px(b.origin.x + x), render::px(b.origin.y + y)),
            render::size(render::px(w), render::px(h)),
        );
        if !visible(painter, bounds) || media.opacity() <= 0. {
            return Ok(());
        }
        let (pw, ph) = physical_size(w, h, painter.scale_factor())?;
        let image = render::PaintImage {
            bounds,
            clip_bounds: bounds_from_rect(b),
            corner_radii: render::Corners::all(render::px(ctx.content_radius())),
            opacity: media.opacity(),
            sampling: media.sampling(),
        };
        if self.policy == SvgRenderPolicy::ScaleWhileRendering
            && let Some(source) = self.source.svg_source()
        {
            let ready = self
                .background
                .borrow_mut()
                .get_or_insert_with(SvgRenderCache::default)
                .request(source, (pw, ph), &ctx)?;
            if let Some(ready) = ready {
                painter.paint_image(&ready.raster.texture, image, &mut || {
                    Ok((
                        ready.raster.size(),
                        std::borrow::Cow::Borrowed(&ready.pixels),
                    ))
                })?;
            }
            return Ok(());
        }
        let raster = self.source.raster(pw, ph)?;
        painter.paint_image(&raster.texture, image, &mut || self.source.pixels(&raster))?;
        *self.raster.borrow_mut() = Some(raster);
        Ok(())
    }
}
