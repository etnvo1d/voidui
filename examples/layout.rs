//! Run with `cargo run --example layout`; no window, GPU, or installed fonts needed.
use std::sync::Arc;

use voidui::{
    core::{
        layout::{AvailableSpace, Size, fr, length},
        widget::WidgetId,
        widget_tree::WidgetTree,
    },
    render::{ParleyTextSystem, TextLayoutCache, TextSystem},
    widgets::div::div,
};

fn print_bounds(tree: &WidgetTree, node: WidgetId, depth: usize) {
    println!(
        "{}{:?}: {:?}",
        "  ".repeat(depth),
        tree.style(node).layout.display,
        tree.bounds(node)
    );
    for child in tree.children(node) {
        print_bounds(tree, *child, depth + 1);
    }
}

fn main() {
    let text_cache = TextLayoutCache::new(Arc::new(TextSystem::new(Arc::new(
        ParleyTextSystem::new_without_system_fonts("sans-serif"),
    ))));
    let mut tree = WidgetTree::new();
    let root = tree.build_root(
        div()
            .grid()
            .gap(12.0)
            .size(600, 240)
            .padding(16.0)
            .grid_template_columns(vec![length(160), fr(1.0)])
            .child(div().child(div().height(40)).child(div().height(80)))
            .child(
                div()
                    .flex_col()
                    .gap(8.0)
                    .child(div().height(48))
                    .child(div().flex_grow(1.0)),
            ),
    );
    tree.layout(
        Size {
            width: AvailableSpace::Definite(800.0),
            height: AvailableSpace::Definite(600.0),
        },
        &text_cache,
    );
    print_bounds(&tree, root, 0);
}
