//! Reusable CSS layout entry points backed by Taffy.
//!
//! These functions work with any tree implementing Taffy's layout traits; they
//! do not depend on widgets, rendering, or window state. Styles and layout inputs
//! use Taffy's types, with a public Dimension input that converts CSS percentage
//! numbers into Taffy fractions. The engine retains distinct min/max-content queries
//! and the containing block size used to resolve percentages.
//!
//! Block/flow-root, Flexbox, Grid, relative/absolute positioning, and the CSS box
//! model are supported. Inline formatting, tables, floats, fixed/sticky positioning,
//! painting clips, and interactive scrolling are outside this module's scope.

#![doc = include_str!("../../docs/layout.md")]

mod dimension;
pub use crate::style::math::resolve_calc;
pub use dimension::Dimension;
pub use taffy::geometry::{Line, Point, Rect, Size};
pub use taffy::style::*;
pub use taffy::style_helpers::*;
pub use taffy::{
    BlockContext, Cache, CacheTree, Layout, LayoutBlockContainer, LayoutFlexboxContainer,
    LayoutGridContainer, LayoutInput, LayoutOutput, LayoutPartialTree, NodeId, RequestedAxis,
    RunMode, SizingMode, Style as LayoutStyle, TraversePartialTree, TraverseTree,
    compute_block_layout as layout_block, compute_flexbox_layout as layout_flex,
    compute_grid_layout as layout_grid, compute_hidden_layout as layout_hidden,
    compute_root_layout as layout_root,
};

/// Lay out a container using its display mode, even when it has no children.
///
/// Pass the parent's block context through ordinary blocks so adjoining margins
/// can collapse across nested containers. Flow-root explicitly starts a new block
/// formatting context. Keep caching in the host tree so repeated measurements
/// within one layout pass can reuse results.
pub fn layout_container(
    tree: &mut (impl LayoutBlockContainer + LayoutFlexboxContainer + LayoutGridContainer + CacheTree),
    node: NodeId,
    display: Display,
    inputs: LayoutInput,
    block_context: Option<&mut BlockContext<'_>>,
) -> LayoutOutput {
    if inputs.run_mode == RunMode::PerformHiddenLayout || display == Display::None {
        return layout_hidden(tree, node);
    }

    match display {
        Display::Block => layout_block(tree, node, inputs, block_context),
        Display::FlowRoot => layout_block(tree, node, inputs, None),
        Display::Flex => layout_flex(tree, node, inputs),
        Display::Grid => layout_grid(tree, node, inputs),
        Display::None => unreachable!("hidden containers are handled before dispatch"),
    }
}

/// Measure intrinsic content while Taffy applies sizing, padding, and borders.
///
/// The callback measures the content box in logical pixels. It can be called
/// several times with different available widths (for example when text wraps).
/// Handle `MinContent` and `MaxContent` separately; neither means a fixed width.
/// Hidden nodes never invoke the callback.
pub fn layout_leaf(
    style: &LayoutStyle,
    inputs: LayoutInput,
    measure: impl FnOnce(Size<Option<f32>>, Size<AvailableSpace>) -> Size<f32>,
) -> LayoutOutput {
    if inputs.run_mode == RunMode::PerformHiddenLayout || style.display == Display::None {
        return LayoutOutput::HIDDEN;
    }

    taffy::compute_leaf_layout(inputs, style, resolve_calc, measure)
}

/// Resolve a single out-of-flow, empty CSS box against a containing block. This
/// shares Taffy's sizing, insets, margins, borders and padding with widget layout;
/// it is useful for generated boxes such as ::backdrop without inventing widgets.
pub fn layout_positioned_box(style: &LayoutStyle, containing: Size<f32>) -> Layout {
    struct BoxTree {
        root: LayoutStyle,
        child: LayoutStyle,
        layout: Layout,
        caches: [Cache; 2],
    }
    const ROOT: NodeId = NodeId::new(0);
    const CHILD: NodeId = NodeId::new(1);
    impl TraversePartialTree for BoxTree {
        type ChildIter<'a> = std::iter::Copied<std::slice::Iter<'a, NodeId>>;
        fn child_ids(&self, id: NodeId) -> Self::ChildIter<'_> {
            if id == ROOT {
                [CHILD].as_slice().iter().copied()
            } else {
                [].as_slice().iter().copied()
            }
        }
        fn child_count(&self, id: NodeId) -> usize {
            usize::from(id == ROOT)
        }
        fn get_child_id(&self, _: NodeId, _: usize) -> NodeId {
            CHILD
        }
    }
    impl TraverseTree for BoxTree {}
    impl LayoutPartialTree for BoxTree {
        fn resolve_calc_value(&self, value: *const (), basis: f32) -> f32 {
            resolve_calc(value, basis)
        }
        type CustomIdent = String;
        type CoreContainerStyle<'a> = &'a LayoutStyle;
        fn get_core_container_style(&self, id: NodeId) -> &LayoutStyle {
            if id == ROOT { &self.root } else { &self.child }
        }
        fn set_unrounded_layout(&mut self, id: NodeId, layout: &Layout) {
            if id == CHILD {
                self.layout = *layout;
            }
        }
        fn compute_child_layout(&mut self, id: NodeId, input: LayoutInput) -> LayoutOutput {
            if id == ROOT {
                layout_block(self, id, input, None)
            } else {
                layout_leaf(&self.child, input, |_, _| Size::ZERO)
            }
        }
    }
    impl CacheTree for BoxTree {
        fn cache_get(&mut self, id: NodeId, input: &LayoutInput) -> Option<LayoutOutput> {
            self.caches[usize::from(id == CHILD)].get(input)
        }
        fn cache_store(&mut self, id: NodeId, input: &LayoutInput, output: LayoutOutput) {
            self.caches[usize::from(id == CHILD)].store(input, output);
        }
        fn cache_clear(&mut self, id: NodeId) {
            self.caches[usize::from(id == CHILD)].clear();
        }
    }
    impl LayoutBlockContainer for BoxTree {
        type BlockContainerStyle<'a> = &'a LayoutStyle;
        type BlockItemStyle<'a> = &'a LayoutStyle;
        fn get_block_container_style(&self, id: NodeId) -> &LayoutStyle {
            self.get_core_container_style(id)
        }
        fn get_block_child_style(&self, id: NodeId) -> &LayoutStyle {
            self.get_core_container_style(id)
        }
        fn compute_block_child_layout(
            &mut self,
            id: NodeId,
            input: LayoutInput,
            _: Option<&mut BlockContext<'_>>,
        ) -> LayoutOutput {
            self.compute_child_layout(id, input)
        }
    }
    let mut child = style.clone();
    child.position = Position::Absolute;
    let mut tree = BoxTree {
        root: LayoutStyle {
            display: Display::Block,
            box_sizing: BoxSizing::BorderBox,
            size: Size {
                width: taffy::Dimension::length(containing.width),
                height: taffy::Dimension::length(containing.height),
            },
            ..Default::default()
        },
        child,
        layout: Layout::default(),
        caches: [Cache::new(), Cache::new()],
    };
    layout_root(
        &mut tree,
        ROOT,
        Size {
            width: AvailableSpace::Definite(containing.width),
            height: AvailableSpace::Definite(containing.height),
        },
    );
    tree.layout
}
