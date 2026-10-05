//! A shared glass surface behind ordinary child widgets.
use crate::{
    core::{
        context::{DrawContext, LayoutContext},
        element::IntoElement,
        layout::{LayoutInput, LayoutOutput},
        widget::{Widget, WidgetBuilder, WidgetUpdate},
    },
    render::{Affine, Corners, GlassMaterial, Painter, Result, px},
};

/// One optical background for the visible direct children's border boxes.
/// Child backgrounds should remain transparent. Text and controls retain normal
/// hit regions; the visual liquid bridge does not invent an interactive widget.
#[derive(Clone, Debug, PartialEq)]
pub struct GlassGroup {
    material: GlassMaterial,
    spacing: f32,
}

/// Create a container whose direct children form a liquid glass surface.
/// `spacing` controls SDF smoothing in logical pixels. Use flex/grid/gap to move
/// children and `.border_radius()` to shape them. Translation is supported per
/// child; apply rotation or scale to the container as a whole.
pub fn glass_group(material: GlassMaterial, spacing: f32) -> WidgetBuilder<GlassGroup> {
    assert!(material.is_valid(), "invalid glass material");
    assert!(
        spacing.is_finite() && spacing >= 0.,
        "invalid glass spacing"
    );
    WidgetBuilder::from_widget(GlassGroup { material, spacing })
}
impl WidgetBuilder<GlassGroup> {
    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_element());
        self
    }
    pub fn children(mut self, children: impl IntoIterator<Item = impl IntoElement>) -> Self {
        self.children
            .extend(children.into_iter().map(IntoElement::into_element));
        self
    }
}
impl Widget for GlassGroup {
    fn tag_name(&self) -> &'static str {
        "glass-group"
    }
    fn reconcile(&mut self, next: &dyn Widget) -> WidgetUpdate {
        let Some(next) = (next as &dyn std::any::Any).downcast_ref::<Self>() else {
            return WidgetUpdate::Replace;
        };
        if self == next {
            WidgetUpdate::Unchanged
        } else {
            *self = next.clone();
            WidgetUpdate::Changed
        }
    }
    fn layout(&mut self, inputs: LayoutInput, context: LayoutContext<'_, '_>) -> LayoutOutput {
        context.layout_children(inputs)
    }
    fn paints_content(&self) -> bool {
        false
    }
    fn draw_background(&self, painter: &mut Painter<'_>, context: DrawContext) -> Result<()> {
        let Some((id, tree)) = context.element else {
            return Ok(());
        };
        let parent = tree.nodes[id]
            .visual
            .as_ref()
            .map_or(Affine::IDENTITY, |v| v.matrix);
        let Some(inverse) = parent.inverse() else {
            return Ok(());
        };
        let mut shapes = Vec::with_capacity(tree.children(id).len());
        for &child in tree.children(id) {
            let node = &tree.nodes[child];
            if tree.is_top_layer(child)
                || node.computed.layout.display == crate::core::layout::Display::None
                || node.computed.layer.visibility != crate::style::layer::Visibility::Visible
            {
                continue;
            }
            let matrix = node.visual.as_ref().map_or(Affine::IDENTITY, |v| v.matrix);
            let relative = inverse.compose(matrix);
            let [a, b, c, d, x, y] = relative.0;
            anyhow::ensure!(
                (a - 1.).abs() < 1e-5 && b.abs() < 1e-5 && c.abs() < 1e-5 && (d - 1.).abs() < 1e-5,
                "glass-group children support translation; apply rotation/scale to their glass-group container"
            );
            let mut bounds = tree.bounds(child);
            bounds.origin.x += x;
            bounds.origin.y += y;
            if bounds.is_empty() {
                continue;
            }
            let radius = node
                .computed
                .paint
                .border_radius
                .max(0.)
                .min(bounds.size.width.min(bounds.size.height) * 0.5);
            shapes.push((
                crate::core::paint::render_bounds(bounds),
                Corners::all(px(radius)),
            ));
        }
        // This surface belongs to the container's content, so it follows the
        // same scroll/rounded-overflow clip as its children despite painting early.
        if let Some(space) = tree.entry_space(id, crate::core::stacking::Phase::Content) {
            painter.with_space(&space, |p| {
                p.paint_liquid_glass_group(&shapes, self.material, self.spacing)
            });
        } else {
            let clip = tree
                .entry_clip(id, crate::core::stacking::Phase::Content)
                .bounds(painter.content_mask().bounds);
            painter.with_clip(clip, |p| {
                p.paint_liquid_glass_group(&shapes, self.material, self.spacing)
            });
        }
        Ok(())
    }
}
