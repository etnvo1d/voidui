//! Layout tests run without a window, GPU, or installed fonts.
use std::{cell::Cell, rc::Rc, sync::Arc};

use voidui::{
    core::{
        context::LayoutContext,
        element::{ElementProps, IntoElement},
        geometry::{Point as UiPoint, Size as UiSize},
        layout::*,
        widget::{Widget, WidgetBuilder, WidgetId},
        widget_tree::WidgetTree,
    },
    render::{ParleyTextSystem, TextLayoutCache, TextSystem},
    style::{color::Rgba8, style::Style as VisualStyle},
    widgets::div::div,
};

fn text_cache() -> TextLayoutCache {
    TextLayoutCache::new(Arc::new(TextSystem::new(Arc::new(
        ParleyTextSystem::new_without_system_fonts("sans-serif"),
    ))))
}

fn available(width: f32, height: f32) -> Size<AvailableSpace> {
    Size {
        width: AvailableSpace::Definite(width),
        height: AvailableSpace::Definite(height),
    }
}

fn compute(element: impl IntoElement, space: Size<AvailableSpace>) -> (WidgetTree, WidgetId) {
    let mut tree = WidgetTree::new();
    let root = tree.build_root(element);
    tree.layout(space, &text_cache());
    (tree, root)
}

fn assert_box(tree: &WidgetTree, id: WidgetId, x: f32, y: f32, width: f32, height: f32) {
    let actual = tree.bounds(id);
    let components = [
        actual.origin.x,
        actual.origin.y,
        actual.size.width,
        actual.size.height,
    ];
    for (actual, expected) in components.into_iter().zip([x, y, width, height]) {
        assert!(
            (actual - expected).abs() < 0.001,
            "expected {expected}, got {actual}; bounds: {:?}",
            tree.bounds(id)
        );
    }
}

#[test]
fn default_div_is_a_content_box_block_and_preserves_style() {
    let (tree, root) = compute(
        div()
            .width(200)
            .padding(10.0)
            .child(div().height(20))
            .child(div().height(30)),
        available(500.0, 300.0),
    );
    assert_eq!(tree.style(root).layout.display, Display::Block);
    assert_eq!(tree.style(root).layout.box_sizing, BoxSizing::ContentBox);
    assert_box(&tree, root, 0.0, 0.0, 220.0, 70.0);
    let children = tree.children(root);
    assert_box(&tree, children[0], 10.0, 10.0, 200.0, 20.0);
    assert_box(&tree, children[1], 10.0, 30.0, 200.0, 30.0);
    assert_eq!(tree.parent(children[0]), Some(root));
}

#[test]
fn empty_containers_and_unbounded_space_are_finite() {
    for display in [
        Display::Block,
        Display::FlowRoot,
        Display::Flex,
        Display::Grid,
    ] {
        let (tree, root) = compute(div().display(display), Size::MAX_CONTENT);
        assert_box(&tree, root, 0.0, 0.0, 0.0, 0.0);
    }
    let (tree, root) = compute(div(), available(300.0, 200.0));
    assert_box(&tree, root, 0.0, 0.0, 300.0, 0.0);
    let mut empty = WidgetTree::new();
    assert_eq!(
        empty.layout(Size::MAX_CONTENT, &text_cache()),
        UiSize::default()
    );
}

#[test]
fn percentages_use_parent_content_box_and_real_fraction() {
    let (tree, root) = compute(
        div().width(200).height(100).padding(10.0).child(
            div()
                .width(voidui::style::pct(25.0))
                .height(voidui::style::pct(50.0)),
        ),
        available(800.0, 600.0),
    );
    assert_box(&tree, tree.children(root)[0], 10.0, 10.0, 50.0, 50.0);
}

#[test]
#[ignore = "Taffy 0.14 resolves block child vertical percentage padding against height; see docs/layout.md"]
fn css_block_vertical_percentage_padding_uses_containing_width() {
    let (tree, root) = compute(
        div().width(200).height(100).padding(10.0).child(
            div()
                .width(voidui::style::pct(25.0))
                .height(voidui::style::pct(50.0))
                .padding_top(voidui::style::pct(10.0))
                .padding_bottom(voidui::style::pct(10.0)),
        ),
        available(800.0, 600.0),
    );
    assert_box(&tree, tree.children(root)[0], 10.0, 10.0, 50.0, 90.0);
}

#[test]
fn percent_height_is_auto_in_an_indefinite_parent() {
    let (tree, root) = compute(
        div().width(200).child(
            div()
                .height(voidui::style::pct(50.0))
                .child(div().height(30)),
        ),
        available(800.0, 600.0),
    );
    assert_box(&tree, root, 0.0, 0.0, 200.0, 30.0);
    assert_box(&tree, tree.children(root)[0], 0.0, 0.0, 200.0, 30.0);
}

#[test]
fn block_margins_collapse_and_gap_does_not_apply() {
    let (tree, root) = compute(
        div()
            .gap(99.0)
            .width(200)
            .child(div().height(20).margin_bottom(30))
            .child(div().height(10).margin_top(20)),
        Size::MAX_CONTENT,
    );
    assert_box(&tree, root, 0.0, 0.0, 200.0, 60.0);
    assert_box(&tree, tree.children(root)[1], 0.0, 50.0, 200.0, 10.0);
}

#[test]
fn negative_and_nested_block_margins_collapse() {
    let (tree, root) = compute(
        div()
            .width(200)
            .child(div().height(20).margin_bottom(30))
            .child(div().margin_top(-10).child(div().height(10).margin_top(20))),
        Size::MAX_CONTENT,
    );
    assert_box(&tree, tree.children(root)[1], 0.0, 40.0, 200.0, 10.0);
    assert_box(&tree, root, 0.0, 0.0, 200.0, 50.0);
}

#[test]
fn block_auto_margins_center_fixed_width_children() {
    let (tree, root) = compute(
        div().width(200).child(
            div()
                .width(50)
                .height(20)
                .margin_left(voidui::style::AUTO)
                .margin_right(voidui::style::AUTO),
        ),
        Size::MAX_CONTENT,
    );
    assert_box(&tree, tree.children(root)[0], 75.0, 0.0, 50.0, 20.0);
}

#[test]
fn flex_grow_respects_gap_and_max_size() {
    let (tree, root) = compute(
        div()
            .flex()
            .width(300)
            .gap(10.0)
            .child(div().height(20).flex_basis(0).flex_grow(1.0).max_width(100))
            .child(div().height(30).flex_basis(0).flex_grow(1.0)),
        Size::MAX_CONTENT,
    );
    let children = tree.children(root);
    assert_box(&tree, children[0], 0.0, 0.0, 100.0, 20.0);
    assert_box(&tree, children[1], 110.0, 0.0, 190.0, 30.0);
}

#[test]
fn flex_shrink_is_weighted_by_basis_and_freezes_at_minimum() {
    let (tree, root) = compute(
        div()
            .flex()
            .width(180)
            .height(20)
            .child(div().width(200).min_width(150))
            .child(div().width(100).min_width(0)),
        Size::MAX_CONTENT,
    );
    let children = tree.children(root);
    assert_box(&tree, children[0], 0.0, 0.0, 150.0, 20.0);
    assert_box(&tree, children[1], 150.0, 0.0, 30.0, 20.0);
}

#[test]
fn flex_wrap_uses_separate_row_and_column_gaps() {
    let (tree, root) = compute(
        div()
            .flex()
            .width(120)
            .flex_wrap(FlexWrap::Wrap)
            .column_gap(10)
            .row_gap(7)
            .child(div().width(50).height(20))
            .child(div().width(50).height(20))
            .child(div().width(50).height(20)),
        Size::MAX_CONTENT,
    );
    let children = tree.children(root);
    assert_box(&tree, root, 0.0, 0.0, 120.0, 47.0);
    assert_box(&tree, children[1], 60.0, 0.0, 50.0, 20.0);
    assert_box(&tree, children[2], 0.0, 27.0, 50.0, 20.0);
}

#[test]
fn flex_reverse_column_and_alignment() {
    let (tree, root) = compute(
        div()
            .flex()
            .width(100)
            .height(100)
            .flex_direction(FlexDirection::ColumnReverse)
            .justify_content(JustifyContent::SPACE_BETWEEN)
            .align_items(AlignItems::CENTER)
            .child(div().width(20).height(30))
            .child(div().width(40).height(20)),
        Size::MAX_CONTENT,
    );
    let children = tree.children(root);
    assert_box(&tree, children[0], 40.0, 70.0, 20.0, 30.0);
    assert_box(&tree, children[1], 30.0, 0.0, 40.0, 20.0);
}

#[test]
fn flex_auto_margin_consumes_free_space_before_justification() {
    let (tree, root) = compute(
        div()
            .flex()
            .width(200)
            .height(50)
            .gap(10.0)
            .child(div().width(50))
            .child(
                div()
                    .width(50)
                    .height(20)
                    .margin_left(voidui::style::AUTO)
                    .align_self(AlignSelf::END),
            ),
        Size::MAX_CONTENT,
    );
    assert_box(&tree, tree.children(root)[1], 150.0, 30.0, 50.0, 20.0);
}

#[test]
fn grid_fraction_tracks_and_implicit_rows() {
    let (tree, root) = compute(
        div()
            .grid()
            .width(310)
            .gap(10.0)
            .grid_template_columns(vec![fr(1.0), fr(2.0)])
            .grid_auto_rows(vec![length(30)])
            .child(div())
            .child(div())
            .child(div()),
        Size::MAX_CONTENT,
    );
    let children = tree.children(root);
    assert_box(&tree, root, 0.0, 0.0, 310.0, 70.0);
    assert_box(&tree, children[0], 0.0, 0.0, 100.0, 30.0);
    assert_box(&tree, children[1], 110.0, 0.0, 200.0, 30.0);
    assert_box(&tree, children[2], 0.0, 40.0, 100.0, 30.0);
}

#[test]
fn grid_explicit_placement_spans_and_nested_flex() {
    let (tree, root) = compute(
        div()
            .grid()
            .width(210)
            .gap(10.0)
            .grid_template_columns(vec![fr(1.0), fr(1.0)])
            .grid_auto_rows(vec![length(40)])
            .child(
                div()
                    .flex()
                    .grid_column(Line {
                        start: line(1),
                        end: span(2),
                    })
                    .grid_row(Line {
                        start: line(2),
                        end: auto(),
                    })
                    .justify_content(JustifyContent::END)
                    .child(div().width(20)),
            )
            .child(div()),
        Size::MAX_CONTENT,
    );
    let children = tree.children(root);
    assert_box(&tree, children[0], 0.0, 50.0, 210.0, 40.0);
    assert_box(
        &tree,
        tree.children(children[0])[0],
        190.0,
        50.0,
        20.0,
        40.0,
    );
    assert_box(&tree, children[1], 0.0, 0.0, 100.0, 40.0);
}

#[test]
fn absolute_children_do_not_take_up_flow_space() {
    for display in [Display::Block, Display::Flex, Display::Grid] {
        let (tree, root) = compute(
            div()
                .display(display)
                .width(200)
                .height(100)
                .child(
                    div()
                        .width(20)
                        .height(10)
                        .position(Position::Absolute)
                        .right(10)
                        .bottom(5),
                )
                .child(div().width(30).height(20)),
            Size::MAX_CONTENT,
        );
        let children = tree.children(root);
        assert_box(&tree, children[0], 170.0, 85.0, 20.0, 10.0);
        assert_box(&tree, children[1], 0.0, 0.0, 30.0, 20.0);
    }
}

#[test]
fn absolute_auto_size_uses_opposing_insets() {
    let (tree, root) = compute(
        div()
            .width(200)
            .height(100)
            .child(div().position(Position::Absolute).inset_edges(Rect {
                left: length(10),
                right: length(20),
                top: length(5),
                bottom: length(15),
            })),
        Size::MAX_CONTENT,
    );
    assert_box(&tree, tree.children(root)[0], 10.0, 5.0, 170.0, 80.0);
}

#[test]
fn relative_offsets_preserve_the_original_flow_slot() {
    let (tree, root) = compute(
        div()
            .width(100)
            .child(div().relative().height(20).left(10).top(5))
            .child(div().height(30)),
        Size::MAX_CONTENT,
    );
    let children = tree.children(root);
    assert_box(&tree, children[0], 10.0, 5.0, 100.0, 20.0);
    assert_box(&tree, children[1], 0.0, 20.0, 100.0, 30.0);
    assert_box(&tree, root, 0.0, 0.0, 100.0, 50.0);
}

#[test]
fn border_box_and_content_box_apply_padding_and_border_once() {
    for (box_sizing, outer_width, child_width) in [
        (BoxSizing::ContentBox, 130.0, 100.0),
        (BoxSizing::BorderBox, 100.0, 70.0),
    ] {
        let (tree, root) = compute(
            div()
                .width(100)
                .box_sizing(box_sizing)
                .padding(10.0)
                .border_width(5.0)
                .child(div().height(20)),
            Size::MAX_CONTENT,
        );
        assert_box(&tree, root, 0.0, 0.0, outer_width, 50.0);
        assert_box(&tree, tree.children(root)[0], 15.0, 15.0, child_width, 20.0);
    }
}

#[test]
fn min_max_and_aspect_ratio_are_applied_before_children() {
    let (tree, root) = compute(
        div().width(500).max_width(200).aspect_ratio(2.0).child(
            div()
                .width(voidui::style::pct(50.0))
                .height(voidui::style::pct(50.0)),
        ),
        Size::MAX_CONTENT,
    );
    assert_box(&tree, root, 0.0, 0.0, 200.0, 100.0);
    assert_box(&tree, tree.children(root)[0], 0.0, 0.0, 100.0, 50.0);
}

#[test]
fn repeated_layout_and_subtree_origins_do_not_accumulate() {
    let (mut tree, root) = compute(
        div()
            .width(200)
            .padding(10.0)
            .child(div().padding(5.0).child(div().height(20))),
        Size::MAX_CONTENT,
    );
    let child = tree.children(root)[0];
    let grandchild = tree.children(child)[0];
    let cache = text_cache();
    for _ in 0..3 {
        tree.layout_subtree(root, Size::MAX_CONTENT, UiPoint::new(30.0, 40.0), &cache);
        assert_box(&tree, grandchild, 45.0, 55.0, 190.0, 20.0);
    }
    tree.layout(Size::MAX_CONTENT, &cache);
    assert_box(&tree, grandchild, 15.0, 15.0, 190.0, 20.0);
    tree.layout_subtree(
        child,
        available(100.0, 100.0),
        UiPoint::new(60.0, 70.0),
        &cache,
    );
    assert_box(&tree, grandchild, 65.0, 75.0, 90.0, 20.0);
    tree.layout(Size::MAX_CONTENT, &cache);
    assert_box(&tree, grandchild, 15.0, 15.0, 190.0, 20.0);
}

#[test]
fn changed_styles_and_available_space_reflow_descendants() {
    let (mut tree, root) = compute(
        div().child(div().width(voidui::style::pct(50.0)).height(20)),
        available(200.0, 100.0),
    );
    let child = tree.children(root)[0];
    assert_box(&tree, child, 0.0, 0.0, 100.0, 20.0);
    tree.style_mut(child).layout.size.height = length(40);
    tree.layout(available(400.0, 100.0), &text_cache());
    assert_box(&tree, child, 0.0, 0.0, 200.0, 40.0);
    assert_box(&tree, root, 0.0, 0.0, 400.0, 40.0);
}

#[test]
fn hidden_subtrees_are_zeroed_and_can_be_shown_again() {
    let (mut tree, root) = compute(
        div()
            .flex()
            .width(200)
            .child(div().width(50).child(div().height(20)))
            .child(div().width(30).height(10)),
        Size::MAX_CONTENT,
    );
    let child = tree.children(root)[0];
    let grandchild = tree.children(child)[0];
    tree.style_mut(child).layout.display = Display::None;
    tree.layout(Size::MAX_CONTENT, &text_cache());
    assert_box(&tree, child, 0.0, 0.0, 0.0, 0.0);
    assert_box(&tree, grandchild, 0.0, 0.0, 0.0, 0.0);
    assert_box(&tree, tree.children(root)[1], 0.0, 0.0, 30.0, 10.0);
    tree.style_mut(child).layout.display = Display::Block;
    tree.layout(Size::MAX_CONTENT, &text_cache());
    assert_box(&tree, grandchild, 0.0, 0.0, 50.0, 20.0);
}

/// A synthetic wrapping leaf exercises intrinsic sizing without text/font state.
struct WrappingLeaf {
    natural_width: Rc<Cell<f32>>,
    calls: Rc<Cell<usize>>,
}

impl Widget for WrappingLeaf {
    fn layout(&mut self, inputs: LayoutInput, ctx: LayoutContext<'_, '_>) -> LayoutOutput {
        layout_leaf(&ctx.style().layout, inputs, |known, available| {
            self.calls.set(self.calls.get() + 1);
            let natural = self.natural_width.get();
            let width = known
                .width
                .unwrap_or_else(|| match available.width {
                    AvailableSpace::Definite(width) => width.min(natural),
                    AvailableSpace::MinContent => 20.0,
                    AvailableSpace::MaxContent => natural,
                })
                .max(1.0);
            Size {
                width,
                height: (natural / width).ceil() * 10.0,
            }
        })
    }
}

fn wrapping_leaf(width: &Rc<Cell<f32>>, calls: &Rc<Cell<usize>>) -> WidgetBuilder<WrappingLeaf> {
    WidgetBuilder {
        events: Default::default(),
        widget: WrappingLeaf {
            natural_width: width.clone(),
            calls: calls.clone(),
        },
        props: ElementProps::new(VisualStyle::new(
            Rgba8::from_rgb8(0, 0, 0),
            Rgba8::from_rgb8(255, 255, 255),
        )),
        children: vec![],
    }
}

#[test]
fn intrinsic_leaf_remeasures_at_flex_width_and_after_content_changes() {
    let width = Rc::new(Cell::new(200.0));
    let calls = Rc::new(Cell::new(0));
    let (mut tree, root) = compute(
        div().flex().width(100).child(wrapping_leaf(&width, &calls)),
        Size::MAX_CONTENT,
    );
    let child = tree.children(root)[0];
    assert_box(&tree, child, 0.0, 0.0, 100.0, 20.0);
    assert!(calls.get() > 0);
    width.set(300.0);
    tree.layout(Size::MAX_CONTENT, &text_cache());
    assert_box(&tree, child, 0.0, 0.0, 100.0, 30.0);
    assert_box(&tree, root, 0.0, 0.0, 100.0, 30.0);
}

#[test]
fn intrinsic_leaf_distinguishes_min_content_and_max_content() {
    let width = Rc::new(Cell::new(200.0));
    let calls = Rc::new(Cell::new(0));
    for (available, expected_width, expected_height) in [
        (AvailableSpace::MinContent, 20.0, 100.0),
        (AvailableSpace::MaxContent, 200.0, 10.0),
    ] {
        let (tree, root) = compute(
            wrapping_leaf(&width, &calls),
            Size {
                width: available,
                height: AvailableSpace::MaxContent,
            },
        );
        assert_box(&tree, root, 0.0, 0.0, expected_width, expected_height);
    }
}

#[test]
fn hidden_ancestors_skip_custom_widget_measurement() {
    let width = Rc::new(Cell::new(200.0));
    let calls = Rc::new(Cell::new(0));
    let (tree, root) = compute(
        div()
            .display(Display::None)
            .child(wrapping_leaf(&width, &calls)),
        available(200.0, 100.0),
    );
    assert_eq!(calls.get(), 0);
    assert_box(&tree, tree.children(root)[0], 0.0, 0.0, 0.0, 0.0);
}

#[test]
fn replacing_root_discards_old_nodes_and_preserves_key_generations() {
    let mut tree = WidgetTree::new();
    let old = tree.build_root(div().child(div()));
    let new = tree.build_root(div().width(20).height(10));
    assert_ne!(old, new);
    assert_eq!(tree.root(), Some(new));
    assert_eq!(tree.parent(new), None);
    assert!(tree.children(new).is_empty());
    tree.layout(Size::MAX_CONTENT, &text_cache());
    assert_box(&tree, new, 0.0, 0.0, 20.0, 10.0);
    let round_trip = WidgetId::from(NodeId::from(new));
    assert_eq!(round_trip, new);
}

#[test]
fn upstream_reference_reproduces_block_percentage_padding_limitation() {
    let child_style = LayoutStyle {
        display: Display::Block,
        box_sizing: BoxSizing::ContentBox,
        size: Size {
            width: percent(0.25),
            height: percent(0.5),
        },
        padding: Rect {
            top: percent(0.1),
            bottom: percent(0.1),
            ..Rect::zero()
        },
        ..LayoutStyle::default()
    };
    let parent_style = LayoutStyle {
        display: Display::Block,
        box_sizing: BoxSizing::ContentBox,
        size: Size {
            width: length(200),
            height: length(100),
        },
        padding: Rect::length(10.0),
        ..LayoutStyle::default()
    };

    // Run the same styles through the unmodified dependency without any voidui
    // layout adapter. This distinguishes an upstream limitation from an adapter bug.
    let mut reference: taffy::TaffyTree<()> = taffy::TaffyTree::new();
    reference.disable_rounding();
    let child = reference.new_leaf(child_style.clone()).unwrap();
    let root = reference
        .new_with_children(parent_style.clone(), &[child])
        .unwrap();
    reference
        .compute_layout(root, available(800.0, 600.0))
        .unwrap();

    let (tree, root) = compute(
        div()
            .layout_style(parent_style)
            .child(div().layout_style(child_style)),
        available(800.0, 600.0),
    );
    let actual = tree.layout_result(tree.children(root)[0]);
    let upstream = reference.layout(child).unwrap();
    assert_eq!(actual.size, upstream.size);
    assert_eq!(actual.padding, upstream.padding);
    // Preserve the CSS expectation in the ignored test above, not as a passing
    // assertion of incorrect browser behavior. Recheck it when upgrading Taffy.
    eprintln!(
        "Taffy block percentage-padding reproduction: actual height {}, CSS expected 90",
        upstream.size.height
    );
}

#[test]
fn flow_root_prevents_parent_child_margin_collapsing() {
    let (tree, root) = compute(
        div()
            .width(100)
            .child(div().height(20).margin_bottom(30))
            .child(
                div()
                    .display(Display::FlowRoot)
                    .child(div().height(10).margin_top(20)),
            ),
        Size::MAX_CONTENT,
    );
    let child = tree.children(root)[1];
    assert_box(&tree, child, 0.0, 50.0, 100.0, 30.0);
    assert_box(&tree, tree.children(child)[0], 0.0, 70.0, 100.0, 10.0);
}

#[test]
fn percentage_padding_uses_inline_size_in_flex_and_grid() {
    for display in [Display::Flex, Display::Grid] {
        let (tree, root) = compute(
            div().display(display).width(200).height(100).child(
                div()
                    .width(voidui::style::pct(25.0))
                    .height(voidui::style::pct(50.0))
                    .padding_top(voidui::style::pct(10.0))
                    .padding_bottom(voidui::style::pct(10.0)),
            ),
            Size::MAX_CONTENT,
        );
        assert_box(&tree, tree.children(root)[0], 0.0, 0.0, 50.0, 90.0);
    }
}

#[test]
fn leaf_layout_is_reusable_without_a_widget_tree() {
    let style = LayoutStyle {
        box_sizing: BoxSizing::ContentBox,
        padding: Rect::length(5.0),
        ..LayoutStyle::default()
    };
    let inputs = LayoutInput {
        run_mode: RunMode::PerformLayout,
        ..LayoutInput::HIDDEN
    };
    let output = layout_leaf(&style, inputs, |_, _| Size {
        width: 20.0,
        height: 10.0,
    });
    assert_eq!(
        output.size,
        Size {
            width: 30.0,
            height: 20.0
        }
    );
    let output = layout_leaf(&style, LayoutInput::HIDDEN, |_, _| {
        panic!("hidden leaves must not be measured")
    });
    assert_eq!(output.size, Size::zero());
}
