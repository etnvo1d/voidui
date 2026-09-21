use slotmap::{Key, KeyData};
use voidui_gpui_wgpu::TextLayoutCache;

use crate::{
    core::{
        geometry::Rect,
        layout::{self, *},
        widget::{WidgetId, WidgetStatus},
        widget_tree::WidgetTree,
    },
    style::{style::Style, text::TextStyle},
};

/// Access to a widget's layout tree and text measurement service.
///
/// Container widgets can call `layout_children`; leaf widgets can call
/// `layout_leaf` with `layout_style()`. This adapter implements Taffy's low-level
/// tree traits directly, avoiding a second tree and synchronization between IDs.
pub struct LayoutContext<'tree, 'bfc> {
    pub status: WidgetStatus,
    node: WidgetId,
    tree: &'tree mut WidgetTree,
    text_layout: &'tree TextLayoutCache,
    block_context: Option<&'tree mut BlockContext<'bfc>>,
}

impl<'tree, 'bfc> LayoutContext<'tree, 'bfc> {
    pub(crate) fn new(
        node: WidgetId,
        tree: &'tree mut WidgetTree,
        text_layout: &'tree TextLayoutCache,
        block_context: Option<&'tree mut BlockContext<'bfc>>,
    ) -> Self {
        Self {
            status: tree.nodes[node].status,
            node,
            tree,
            text_layout,
            block_context,
        }
    }

    pub fn node_id(&self) -> NodeId {
        self.node.into()
    }

    /// Cascaded specified declarations. Use layout_style() for Taffy: it includes
    /// CSS inheritance, positioning normalization and sampled transitions.
    pub fn style(&self) -> &Style {
        self.tree.nodes[self.node]
            .cascaded_style
            .as_deref()
            .unwrap_or(&self.tree.nodes[self.node].props.style)
    }

    /// Layout properties after resolving inherited direction.
    pub fn layout_style(&self) -> &LayoutStyle {
        self.tree.used_layout_style(self.node)
    }

    /// Typography after resolving this element's inherited properties.
    pub fn text_style(&self) -> &TextStyle {
        &self.tree.nodes[self.node].computed.text
    }

    pub fn text_layout(&self) -> &TextLayoutCache {
        self.text_layout
    }

    pub fn children_count(&self) -> usize {
        self.tree.nodes[self.node].children.len()
    }

    /// Lay out this widget and its children using the selected CSS display mode.
    pub fn layout_children(mut self, inputs: LayoutInput) -> LayoutOutput {
        let node = self.node_id();
        let display = self.layout_style().display;
        let block_context = self.block_context.take();
        layout::layout_container(&mut self, node, display, inputs, block_context)
    }
}

// Preserve the complete generational key, so stale widget IDs cannot alias newly
// inserted nodes. No pointers or separate ID lookup table are needed.
impl From<WidgetId> for NodeId {
    fn from(id: WidgetId) -> Self {
        Self::new(id.data().as_ffi())
    }
}

impl From<NodeId> for WidgetId {
    fn from(id: NodeId) -> Self {
        KeyData::from_ffi(u64::from(id)).into()
    }
}

impl TraversePartialTree for LayoutContext<'_, '_> {
    type ChildIter<'a>
        = std::iter::Map<std::iter::Copied<std::slice::Iter<'a, WidgetId>>, fn(WidgetId) -> NodeId>
    where
        Self: 'a;

    fn child_ids(&self, node: NodeId) -> Self::ChildIter<'_> {
        let children = if node == super::positioning::VIEWPORT {
            &self.tree.viewport.children[..]
        } else {
            self.tree.layout_children(node.into())
        };
        children.iter().copied().map(NodeId::from)
    }
    fn child_count(&self, node: NodeId) -> usize {
        if node == super::positioning::VIEWPORT {
            self.tree.viewport.children.len()
        } else {
            self.tree.layout_children(node.into()).len()
        }
    }
    fn get_child_id(&self, node: NodeId, index: usize) -> NodeId {
        if node == super::positioning::VIEWPORT {
            self.tree.viewport.children[index].into()
        } else {
            self.tree.layout_children(node.into())[index].into()
        }
    }
}

impl TraverseTree for LayoutContext<'_, '_> {}

impl LayoutPartialTree for LayoutContext<'_, '_> {
    fn resolve_calc_value(&self, value: *const (), basis: f32) -> f32 {
        layout::resolve_calc(value, basis)
    }
    type CustomIdent = String;
    type CoreContainerStyle<'a>
        = &'a LayoutStyle
    where
        Self: 'a;

    fn get_core_container_style(&self, node: NodeId) -> &LayoutStyle {
        if node == super::positioning::VIEWPORT {
            &self.tree.viewport.style
        } else {
            self.tree.used_layout_style(node.into())
        }
    }

    fn set_unrounded_layout(&mut self, node: NodeId, layout: &Layout) {
        if node == super::positioning::VIEWPORT {
            self.tree.viewport.layout = *layout;
        } else {
            let id = WidgetId::from(node);
            let mirror = self.tree.mirror_gutter(id);
            self.tree.nodes[id].layout = *layout;
            self.tree.nodes[id].layout.scrollbar_size.width += mirror;
        }
    }

    fn compute_child_layout(&mut self, node: NodeId, inputs: LayoutInput) -> LayoutOutput {
        if node == super::positioning::VIEWPORT {
            layout::layout_block(self, node, inputs, None)
        } else {
            self.tree
                .layout_node(node.into(), inputs, self.text_layout, None)
        }
    }
}

impl CacheTree for LayoutContext<'_, '_> {
    fn cache_get(&mut self, node: NodeId, inputs: &LayoutInput) -> Option<LayoutOutput> {
        if node == super::positioning::VIEWPORT {
            self.tree.viewport.cache.get(inputs)
        } else {
            self.tree.nodes[WidgetId::from(node)].cache.get(inputs)
        }
    }

    fn cache_store(&mut self, node: NodeId, inputs: &LayoutInput, output: LayoutOutput) {
        if node == super::positioning::VIEWPORT {
            self.tree.viewport.cache.store(inputs, output);
        } else {
            self.tree.nodes[WidgetId::from(node)]
                .cache
                .store(inputs, output);
        }
    }

    fn cache_clear(&mut self, node: NodeId) {
        if node == super::positioning::VIEWPORT {
            self.tree.viewport.cache.clear();
        } else {
            self.tree.nodes[WidgetId::from(node)].cache.clear();
        }
    }
}

impl LayoutBlockContainer for LayoutContext<'_, '_> {
    type BlockContainerStyle<'a>
        = &'a LayoutStyle
    where
        Self: 'a;
    type BlockItemStyle<'a>
        = &'a LayoutStyle
    where
        Self: 'a;

    fn get_block_container_style(&self, node: NodeId) -> &LayoutStyle {
        self.get_core_container_style(node)
    }

    fn get_block_child_style(&self, node: NodeId) -> &LayoutStyle {
        self.get_core_container_style(node)
    }

    fn compute_block_child_layout(
        &mut self,
        node: NodeId,
        inputs: LayoutInput,
        block_context: Option<&mut BlockContext<'_>>,
    ) -> LayoutOutput {
        self.tree
            .layout_node(node.into(), inputs, self.text_layout, block_context)
    }
}

impl LayoutFlexboxContainer for LayoutContext<'_, '_> {
    type FlexboxContainerStyle<'a>
        = &'a LayoutStyle
    where
        Self: 'a;
    type FlexboxItemStyle<'a>
        = &'a LayoutStyle
    where
        Self: 'a;

    fn get_flexbox_container_style(&self, node: NodeId) -> &LayoutStyle {
        self.get_core_container_style(node)
    }

    fn get_flexbox_child_style(&self, node: NodeId) -> &LayoutStyle {
        self.get_core_container_style(node)
    }
}

impl LayoutGridContainer for LayoutContext<'_, '_> {
    type GridContainerStyle<'a>
        = &'a LayoutStyle
    where
        Self: 'a;
    type GridItemStyle<'a>
        = &'a LayoutStyle
    where
        Self: 'a;

    fn get_grid_container_style(&self, node: NodeId) -> &LayoutStyle {
        self.get_core_container_style(node)
    }

    fn get_grid_child_style(&self, node: NodeId) -> &LayoutStyle {
        self.get_core_container_style(node)
    }
}

pub struct DrawContext<'a> {
    pub(crate) element: Option<(WidgetId, &'a WidgetTree)>,
    /// The selected fragment of this text widget, independent of layout metrics.
    pub selection: Option<crate::core::text::TextSelectionPaint>,
    /// Current computed paint values, independent of cached text shaping.
    pub color: crate::style::color::Color,
    pub text_align: voidui_gpui_wgpu::TextAlign,
    pub status: WidgetStatus,
    pub bounds: Rect<f32>,
    /// Global content box after subtracting padding, border, and scrollbar space.
    pub content_bounds: Rect<f32>,
}

impl DrawContext<'_> {
    pub fn new(status: WidgetStatus, bounds: Rect<f32>) -> Self {
        Self {
            element: None,
            selection: None,
            status,
            bounds,
            content_bounds: bounds,
            color: crate::style::text::TextStyle::default().color,
            text_align: voidui_gpui_wgpu::TextAlign::Left,
        }
    }
}

impl DrawContext<'_> {
    pub fn media_style(&self) -> crate::style::media::MediaStyle {
        self.element
            .map(|(id, tree)| tree.nodes[id].computed.media.clone())
            .unwrap_or_default()
    }
    /// Resolve the inner rounding after borders and padding, not the image's
    /// fitted bounds (cover and object-position may extend beyond this box).
    pub fn content_radius(&self) -> f32 {
        self.element
            .map(|(id, tree)| {
                let radius = tree.nodes[id].computed.paint.border_radius;
                (radius
                    - (self.content_bounds.origin.x - self.bounds.origin.x)
                        .max(self.content_bounds.origin.y - self.bounds.origin.y))
                .max(0.)
                .min(
                    self.content_bounds
                        .size
                        .width
                        .min(self.content_bounds.size.height)
                        * 0.5,
                )
            })
            .unwrap_or(0.)
    }
}
