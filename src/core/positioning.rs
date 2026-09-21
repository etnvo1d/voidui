//! Layout ownership is distinct from DOM ownership. Positioned descendants use
//! their containing block in Taffy without changing selector or inheritance links.
use super::{
    context::LayoutContext,
    geometry::{Point, Size},
    layout::{self, *},
    widget::WidgetId,
    widget_tree::WidgetTree,
};
use crate::style::layer::Position as CssPosition;
use taffy::Dimension;
use voidui_gpui_wgpu::TextLayoutCache;

/// SlotMap keys are generational and never have this value. This synthetic node
/// exists only in the Taffy adapter, not in selectors, widget APIs or hit testing.
pub(crate) const VIEWPORT: NodeId = NodeId::new(0);
pub(crate) struct ViewportLayout {
    pub style: LayoutStyle,
    pub children: Vec<WidgetId>,
    pub layout: Layout,
    pub cache: Cache,
    pub origin: Point<f32>,
}
impl Default for ViewportLayout {
    fn default() -> Self {
        Self {
            style: LayoutStyle {
                display: Display::Block,
                box_sizing: BoxSizing::BorderBox,
                ..Default::default()
            },
            children: Vec::new(),
            layout: Layout::default(),
            cache: Cache::new(),
            origin: Point::default(),
        }
    }
}

impl WidgetTree {
    pub(crate) fn layout_children(&self, id: WidgetId) -> &[WidgetId] {
        self.nodes[id]
            .layout_children
            .as_deref()
            .map(Vec::as_slice)
            .unwrap_or(&self.nodes[id].children)
    }
    pub(crate) fn prepare_positioning(&mut self, root: WidgetId) {
        self.viewport.children.clear();
        self.viewport.cache.clear();
        for node in self.nodes.values_mut() {
            if let Some(children) = &mut node.layout_children {
                children.as_mut().clone_from(&node.children);
            }
            node.layout_parent = node.parent;
        }
        fn visit(
            tree: &mut WidgetTree,
            id: WidgetId,
            containing: Option<WidgetId>,
            fixed_containing: Option<WidgetId>,
            root: WidgetId,
            order: &mut usize,
        ) {
            tree.nodes[id].tree_order = *order;
            *order += 1;
            let position = tree.nodes[id].computed.layer.position;
            let top = tree.is_top_layer(id);
            let hidden = tree.nodes[id].computed.layout.display == Display::None;
            if id != root && !hidden {
                let owner = if top {
                    None
                } else if position == CssPosition::Fixed {
                    fixed_containing
                } else if position == CssPosition::Absolute {
                    containing
                } else {
                    tree.nodes[id].parent
                };
                if owner != tree.nodes[id].parent {
                    let parent = tree.nodes[id].parent.unwrap();
                    if tree.nodes[parent].layout_children.is_none() {
                        tree.nodes[parent].layout_children =
                            Some(Box::new(tree.nodes[parent].children.clone()));
                    }
                    tree.nodes[parent]
                        .layout_children
                        .as_mut()
                        .unwrap()
                        .retain(|child| *child != id);
                    if let Some(owner) = owner {
                        if tree.nodes[owner].layout_children.is_none() {
                            tree.nodes[owner].layout_children =
                                Some(Box::new(tree.nodes[owner].children.clone()));
                        }
                        tree.nodes[owner].layout_children.as_mut().unwrap().push(id);
                    } else {
                        tree.viewport.children.push(id);
                    }
                    tree.nodes[id].layout_parent = owner;
                }
            }
            if hidden {
                return;
            }
            let transformed = !tree.nodes[id].computed.transform.is_none();
            let fixed_containing = if transformed {
                Some(id)
            } else if top {
                None
            } else {
                fixed_containing
            };
            let containing = if position != CssPosition::Static || top || transformed {
                Some(id)
            } else {
                containing
            };
            for index in 0..tree.nodes[id].children.len() {
                let child = tree.nodes[id].children[index];
                visit(tree, child, containing, fixed_containing, root, order);
            }
        }
        visit(self, root, None, None, root, &mut 0);
        // Moved absolute boxes keep DOM order among the containing block's children.
        let parents: Vec<_> = self
            .nodes
            .iter()
            .filter(|(_, n)| n.layout_children.is_some())
            .map(|(id, _)| id)
            .collect();
        for parent in parents {
            let mut children = self.nodes[parent].layout_children.take().unwrap();
            children.sort_by_key(|id| self.nodes[*id].tree_order);
            self.nodes[parent].layout_children = Some(children);
        }
    }

    pub(crate) fn finish_positioning(
        &mut self,
        root: WidgetId,
        available: layout::Size<AvailableSpace>,
        origin: Point<f32>,
        text: &TextLayoutCache,
    ) {
        self.calc_layout_positions(root, origin);
        let root_size = self.nodes[root].global_bounds.size;
        let dimension = |space: AvailableSpace, fallback: f32| match space {
            AvailableSpace::Definite(v) => v.max(0.0),
            _ => fallback,
        };
        let size = Size::new(
            dimension(available.width, root_size.width),
            dimension(available.height, root_size.height),
        );
        self.viewport.style.size = layout::Size {
            width: Dimension::length(size.width),
            height: Dimension::length(size.height),
        };
        self.viewport.layout.size = layout::Size {
            width: size.width,
            height: size.height,
        };
        self.viewport.origin = origin;
        if !self.viewport.children.is_empty() {
            let mut context = LayoutContext::new(root, self, text, None);
            layout::layout_root(
                &mut context,
                VIEWPORT,
                layout::Size {
                    width: AvailableSpace::Definite(size.width),
                    height: AvailableSpace::Definite(size.height),
                },
            );
            for index in 0..self.viewport.children.len() {
                let child = self.viewport.children[index];
                self.calc_layout_positions(child, origin);
            }
        }
    }
    pub(crate) fn calc_layout_positions(&mut self, id: WidgetId, parent: Point<f32>) {
        let node = &mut self.nodes[id];
        node.global_bounds = super::geometry::Rect::from_xywh(
            parent.x + node.layout.location.x,
            parent.y + node.layout.location.y,
            node.layout.size.width,
            node.layout.size.height,
        );
        self.apply_sticky(id);
        self.update_visual_geometry(id);
        self.reposition_scrolled_children(id);
    }
    pub(crate) fn reposition_scrolled_children(&mut self, id: WidgetId) {
        let offset = self.scroll_offset(id);
        let b = self.nodes[id].global_bounds.origin;
        let origin = Point::new(b.x - offset.x + self.mirror_gutter(id), b.y - offset.y);
        for index in 0..self.layout_children(id).len() {
            let child = self.layout_children(id)[index];
            self.calc_layout_positions(child, origin);
        }
    }
}
