//! Inline SVG viewport integration, styling and retained rendering caches.
use super::{SvgDocument, SvgNode};
use crate::{
    core::{
        context::{DrawContext, LayoutContext},
        element::ElementProps,
        layout::{LayoutInput, LayoutOutput},
        widget::{Widget, WidgetBuilder, WidgetUpdate},
    },
    media::{PreparedSvg, Raster, SvgOptions, SvgRenderCache, SvgRenderPolicy, SvgSource},
    render::{self, Painter},
    style::{css::Stylesheet, style::Style},
};
use anyhow::Result;
use std::{cell::RefCell, sync::Arc};

pub struct Svg {
    document: SvgDocument,
    sheets: Vec<Stylesheet>,
    options: SvgOptions,
    cache: RefCell<Option<SvgCache>>,
    policy: SvgRenderPolicy,
    background: RefCell<Option<SvgRenderCache>>,
}
struct SvgCache {
    revision: u64,
    viewport: [f32; 2],
    source: Arc<str>,
    exact: Option<(Arc<PreparedSvg>, Arc<Raster>)>,
}
/// Create an inline SVG viewport. Its children are compact SVG graph nodes.
pub fn svg() -> WidgetBuilder<Svg> {
    let document = SvgDocument::from_node(SvgNode::new("svg")).unwrap();
    WidgetBuilder {
        events: Default::default(),
        widget: Svg {
            document,
            sheets: Vec::new(),
            options: SvgOptions::default(),
            cache: RefCell::new(None),
            policy: SvgRenderPolicy::default(),
            background: RefCell::default(),
        },
        props: ElementProps::new(Style::default()),
        children: Vec::new(),
    }
}
/// Import the SVG root's attributes and inline style into a normal UI element.
pub fn svg_from_str(source: &str) -> Result<WidgetBuilder<Svg>> {
    Ok(svg().document(SvgDocument::parse(source)?))
}
impl WidgetBuilder<Svg> {
    /// Allow temporary texture scaling and background rasterization, or require
    /// exact synchronous rendering for appearance-sensitive animation.
    pub fn render_policy(mut self, policy: SvgRenderPolicy) -> Self {
        self.widget.policy = policy;
        self.widget.background = RefCell::default();
        self
    }

    pub fn document(mut self, document: SvgDocument) -> Self {
        let root = document.node(0);
        for (name, value) in &root.attrs {
            match name.as_str() {
                "id" => self.props.id = Some(value.clone()),
                "class" => {
                    self.props.classes = value.split_ascii_whitespace().map(Into::into).collect()
                }
                _ => {
                    self.props.attributes.insert(name.clone(), value.clone());
                }
            }
        }
        for (d, _) in root.inline.iter() {
            d.apply(&mut self.props.style);
        }
        self.widget.sheets = document.stylesheets();
        self.widget.document = document;
        self.widget.cache.get_mut().take();
        self
    }
    pub fn options(mut self, options: SvgOptions) -> Self {
        self.widget.options = options;
        self.widget.cache.get_mut().take();
        self
    }
    pub fn view_box(
        mut self,
        x: impl Into<f64>,
        y: impl Into<f64>,
        width: impl Into<f64>,
        height: impl Into<f64>,
    ) -> Self {
        let (x, y, w, h) = (x.into(), y.into(), width.into(), height.into());
        assert!(
            x.is_finite() && y.is_finite() && w.is_finite() && h.is_finite() && w >= 0. && h >= 0.,
            "invalid viewBox"
        );
        self.widget
            .document
            .root_attr("viewBox", format!("{x} {y} {w} {h}"));
        self
    }
    pub fn preserve_aspect_ratio(mut self, value: &str) -> Self {
        value
            .parse::<svgtypes::AspectRatio>()
            .expect("invalid preserveAspectRatio");
        self.widget
            .document
            .root_attr("preserveAspectRatio", value.into());
        self
    }
    pub fn child(mut self, node: SvgNode) -> Self {
        if self
            .widget
            .document
            .append_child(node)
            .expect("invalid SVG child")
        {
            self.widget.sheets = self.widget.document.stylesheets();
        }
        self
    }
    pub fn children(mut self, nodes: impl IntoIterator<Item = SvgNode>) -> Self {
        let mut sheets_changed = false;
        for node in nodes {
            sheets_changed |= self
                .widget
                .document
                .append_child(node)
                .expect("invalid SVG child");
        }
        if sheets_changed {
            self.widget.sheets = self.widget.document.stylesheets();
        }
        self
    }
}
impl Widget for Svg {
    fn tag_name(&self) -> &'static str {
        "svg"
    }
    fn svg_document(&self) -> Option<&SvgDocument> {
        Some(&self.document)
    }
    fn svg_stylesheets(&self) -> &[Stylesheet] {
        &self.sheets
    }
    fn default_style(&self) -> Option<Style> {
        Some(self.document.node(0).base_style())
    }
    fn reconcile(&mut self, next: &dyn Widget) -> WidgetUpdate {
        let Some(next) = (next as &dyn std::any::Any).downcast_ref::<Self>() else {
            return WidgetUpdate::Replace;
        };
        if self.document.same(&next.document)
            && self.policy == next.policy
            && self.options.limits == next.options.limits
            && Arc::ptr_eq(&self.options.fontdb, &next.options.fontdb)
        {
            return WidgetUpdate::Unchanged;
        }
        self.document = next.document.clone();
        self.sheets = next.sheets.clone();
        self.options = next.options.clone();
        self.policy = next.policy;
        self.background = RefCell::default();
        self.cache.get_mut().take();
        WidgetUpdate::Changed
    }
    fn layout(&mut self, input: LayoutInput, ctx: LayoutContext<'_, '_>) -> LayoutOutput {
        crate::media::view::replaced_layout(
            ctx.layout_style(),
            input,
            self.document.intrinsic_size(),
        )
    }
    fn draw(&self, painter: &mut Painter<'_>, ctx: DrawContext) -> Result<()> {
        let bounds = crate::media::view::bounds_from_rect(ctx.content_bounds);
        if self.document.disabled()
            || bounds.is_empty()
            || ctx.media_style().opacity() <= 0.
            || !crate::media::view::visible(painter, bounds)
        {
            return Ok(());
        }
        let viewport = [
            ctx.content_bounds.size.width,
            ctx.content_bounds.size.height,
        ];
        let (w, h) =
            crate::media::view::physical_size(viewport[0], viewport[1], painter.scale_factor())?;
        self.options.limits.check_size(w, h)?;
        let Some((id, tree)) = ctx.element else {
            anyhow::bail!("inline SVG requires a mounted widget context");
        };
        let revision = tree.nodes[id].style_revision;
        let mut cache = self.cache.borrow_mut();
        if cache
            .as_ref()
            .is_none_or(|c| c.revision != revision || c.viewport != viewport)
        {
            let styles =
                crate::style::css::sheet::svg_styles(tree, id, &self.document, &self.sheets);
            let mut source = String::new();
            self.document
                .write_node(0, &mut source, Some(&styles), Some(viewport));
            if let Some(c) = cache.as_mut().filter(|c| c.source.as_ref() == source) {
                c.revision = revision;
                c.viewport = viewport;
            } else {
                *cache = Some(SvgCache {
                    revision,
                    viewport,
                    source: source.into(),
                    exact: None,
                });
            }
        }
        let c = cache.as_mut().unwrap();
        let image = render::PaintImage {
            bounds,
            clip_bounds: bounds,
            corner_radii: render::Corners::all(render::px(ctx.content_radius())),
            opacity: 1.,
            sampling: render::ImageSampling::Smooth,
        };
        if self.policy == SvgRenderPolicy::ScaleWhileRendering {
            let ready = self
                .background
                .borrow_mut()
                .get_or_insert_with(SvgRenderCache::default)
                .request(
                    SvgSource::Inline(c.source.clone(), self.options.clone()),
                    (w, h),
                    &ctx,
                )?;
            if let Some(ready) = ready {
                return painter.paint_image(&ready.raster.texture, image, &mut || {
                    Ok((
                        ready.raster.size(),
                        std::borrow::Cow::Borrowed(&ready.pixels),
                    ))
                });
            }
            return Ok(());
        }
        if c.exact.is_none() {
            let parsed = PreparedSvg::parse(&c.source, &self.options)?;
            let raster = parsed.raster(w, h)?;
            c.exact = Some((parsed, raster));
        }
        let (parsed, raster) = c.exact.as_mut().unwrap();
        if raster.width != w || raster.height != h {
            *raster = parsed.raster(w, h)?;
        }
        painter.paint_image(&raster.texture, image, &mut || {
            Ok((
                raster.size(),
                std::borrow::Cow::Owned(parsed.pixels(raster)?),
            ))
        })
    }
}
