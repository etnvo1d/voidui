//! Scroll behavior is verified against real layout and the shared paint/hit geometry.
use std::{sync::Arc, time::Instant};
use voidui::{
    core::{
        event::ModifiersState,
        geometry::Point,
        layout::{self, AvailableSpace, Size},
        top_layer::HitTarget,
        widget::WidgetId,
        widget_tree::WidgetTree,
    },
    render::{ParleyTextSystem, TextLayoutCache, TextSystem},
    style::css::Stylesheet,
    *,
};
fn cache() -> TextLayoutCache {
    TextLayoutCache::new(Arc::new(TextSystem::new(Arc::new(
        ParleyTextSystem::new_without_system_fonts("unused"),
    ))))
}
fn layout(tree: &mut WidgetTree) {
    tree.layout(
        Size {
            width: AvailableSpace::Definite(800.0),
            height: AvailableSpace::Definite(600.0),
        },
        &cache(),
    );
}
fn build(view: impl IntoElement, css: &str) -> WidgetTree {
    let mut tree = WidgetTree::new();
    tree.build_root(view);
    tree.set_stylesheets(vec![Stylesheet::parse(css).unwrap()]);
    layout(&mut tree);
    tree
}
fn id(tree: &WidgetTree, name: &str) -> WidgetId {
    tree.find_by_id(name).unwrap()
}
fn pane() -> impl IntoElement {
    div()
        .id("pane")
        .size(100, 100)
        .overflow(Overflow::Auto)
        .child(div().id("content").size(300, 400))
}
#[test]
fn scrolling_updates_geometry_hit_testing_without_relayout_or_reordering() {
    let mut tree = build(pane(), "");
    let p = id(&tree, "pane");
    let child = id(&tree, "content");
    let m = tree.scroll_metrics(p).unwrap();
    assert_eq!(m.max, Point::new(200., 300.));
    tree.hit_test(Point::new(10., 10.));
    let order = tree.paint_order_rebuilds();
    let before = *tree.layout_result(child);
    assert!(tree.scroll_to(p, Point::new(40., 80.)));
    assert_eq!(tree.bounds(child).origin, Point::new(-40., -80.));
    assert_eq!(
        tree.hit_test(Point::new(10., 10.)),
        Some(HitTarget::Element(child))
    );
    assert_eq!(tree.paint_order_rebuilds(), order);
    assert_eq!(tree.layout_result(child).location, before.location);
    assert!(!tree.update_styles(Instant::now()).layout);
    assert!(!tree.scroll_to(p, Point::new(40., 80.)));
    assert!(tree.next_animation_frame(Instant::now()).is_none());
}
#[test]
fn standard_css_values_cascade_and_normalize_axes() {
    let tree = build(
        div()
            .id("p")
            .size(100, 100)
            .child(div().id("c").size(200, 200)),
        "#p {overflow-x:visible;overflow-y:auto;scrollbar-width:thin;scrollbar-color:rgb(12, 34, 56) transparent;} #c{scrollbar-color:inherit;overflow:inherit}",
    );
    let p = tree.scroll_style(id(&tree, "p"));
    let c = tree.scroll_style(id(&tree, "c"));
    assert_eq!(
        p.overflow,
        layout::Point {
            x: Overflow::Auto,
            y: Overflow::Auto
        }
    );
    assert_eq!(p.colors, c.colors);
    assert_eq!(c.overflow, p.overflow);
    assert_eq!(c.width, ScrollbarWidth::Auto);
    assert!(Stylesheet::parse("div{scrollbar-width:10px}").is_err());
    assert!(Stylesheet::parse("div{overflow:overlay}").is_err());
    assert!(Stylesheet::parse("div{scrollbar-mode:overlay}").is_err());
}
#[test]
fn hidden_scrolls_programmatically_clip_does_not() {
    let mut tree = build(
        div().id("p").size(100, 100).child(div().size(300, 300)),
        "#p{overflow:hidden}",
    );
    let p = id(&tree, "p");
    assert!(tree.scroll_to(p, Point::new(100., 100.)));
    assert!(tree.scrollbar_geometry(p, ScrollAxis::Vertical).is_none());
    tree.pointer_moved(Some(Point::new(10., 10.)));
    assert!(!tree.scroll_wheel(
        MouseWheel {
            x: 0.,
            y: -10.,
            unit: WheelUnit::Pixels
        },
        ModifiersState::empty()
    ));
    tree.set_stylesheets(vec![Stylesheet::parse("#p{overflow:clip}").unwrap()]);
    layout(&mut tree);
    assert!(tree.scroll_metrics(p).is_none());
    assert!(!tree.scroll_to(p, Point::new(10., 10.)));
}
#[test]
fn classic_auto_discovers_cross_axis_gutters_and_removes_them_after_shrink() {
    let mut tree = build(
        div()
            .id("p")
            .size(100, 100)
            .overflow(Overflow::Auto)
            .scrollbar_mode(ScrollbarMode::Classic)
            .child(div().id("c").size(100, 200)),
        "",
    );
    let p = id(&tree, "p");
    let c = id(&tree, "c");
    let width = tree.scroll_options().width;
    assert_eq!(
        tree.layout_result(p).scrollbar_size,
        Size {
            width,
            height: width
        }
    );
    assert_eq!(tree.content_bounds(p).size.width, 100. - width);
    assert!(tree.scrollbar_geometry(p, ScrollAxis::Horizontal).is_some());
    tree.style_mut(c).layout.size = Size {
        width: layout::Dimension::length(30.).into(),
        height: layout::Dimension::length(30.).into(),
    };
    layout(&mut tree);
    assert_eq!(tree.layout_result(p).scrollbar_size, Size::ZERO);
    assert!(tree.scrollbar_geometry(p, ScrollAxis::Vertical).is_none());
}
#[test]
fn gutter_stable_hidden_and_width_none_obey_placement() {
    let mut tree = build(
        div()
            .id("p")
            .size(100, 100)
            .scrollbar_mode(ScrollbarMode::Classic),
        "#p{overflow:hidden;scrollbar-gutter:stable}",
    );
    let p = id(&tree, "p");
    assert_eq!(
        tree.layout_result(p).scrollbar_size.width,
        tree.scroll_options().width
    );
    assert!(tree.scrollbar_geometry(p, ScrollAxis::Vertical).is_none());
    tree.set_stylesheets(vec![
        Stylesheet::parse("#p{overflow:scroll;scrollbar-gutter:stable;scrollbar-width:none}")
            .unwrap(),
    ]);
    layout(&mut tree);
    assert_eq!(tree.layout_result(p).scrollbar_size, Size::ZERO);
}
#[test]
fn nested_wheel_consumes_distance_then_chains_and_containment_stops_it() {
    let mut tree = build(
        div()
            .id("outer")
            .size(200, 100)
            .overflow(Overflow::Auto)
            .child(
                div()
                    .id("inner")
                    .size(100, 100)
                    .overflow(Overflow::Auto)
                    .child(div().size(100, 150)),
            )
            .child(div().size(100, 200)),
        "",
    );
    let inner = id(&tree, "inner");
    let outer = id(&tree, "outer");
    tree.pointer_moved(Some(Point::new(20., 20.)));
    assert!(tree.scroll_wheel(
        MouseWheel {
            x: 0.,
            y: -70.,
            unit: WheelUnit::Pixels
        },
        ModifiersState::empty()
    ));
    assert_eq!(tree.scroll_metrics(inner).unwrap().offset.y, 50.);
    assert_eq!(tree.scroll_metrics(outer).unwrap().offset.y, 20.);
    tree.set_stylesheets(vec![
        Stylesheet::parse("#inner{overscroll-behavior:contain}").unwrap(),
    ]);
    layout(&mut tree);
    tree.scroll_to(outer, Point::default());
    assert!(!tree.scroll_wheel(
        MouseWheel {
            x: 0.,
            y: -30.,
            unit: WheelUnit::Pixels
        },
        ModifiersState::empty()
    ));
    assert_eq!(tree.scroll_metrics(outer).unwrap().offset.y, 0.);
}
#[test]
fn scrollbar_drag_is_captured_and_track_click_pages_without_activating_content() {
    let mut tree = build(pane(), "");
    let p = id(&tree, "pane");
    let g = tree.scrollbar_geometry(p, ScrollAxis::Vertical).unwrap();
    let start = Point::new(g.thumb.origin.x + 2., g.thumb.origin.y + 2.);
    tree.pointer_moved(Some(start));
    assert_eq!(tree.hit_test(start), Some(HitTarget::Element(p)));
    assert!(
        tree.dispatch_mouse_button(MouseButton::Left, true, ModifiersState::empty())
            .response
            .prevent_default
    );
    assert_eq!(tree.pointer_capture(), Some(p));
    tree.pointer_moved(Some(Point::new(start.x, 300.)));
    assert_eq!(tree.scroll_metrics(p).unwrap().offset.y, 300.);
    tree.dispatch_mouse_button(MouseButton::Left, false, ModifiersState::empty());
    assert_eq!(tree.pointer_capture(), None);
    tree.scroll_to(p, Point::default());
    tree.pointer_moved(Some(Point::new(start.x, 60.)));
    tree.dispatch_mouse_button(MouseButton::Left, true, ModifiersState::empty());
    assert_eq!(tree.scroll_metrics(p).unwrap().offset.y, 100.);
    tree.cancel_pointer_capture();
    assert_eq!(tree.pointer_capture(), None);
}
#[test]
fn rtl_offsets_are_negative_and_fixed_descendants_stay_at_viewport() {
    let mut tree = build(
        div()
            .id("p")
            .size(100, 100)
            .overflow(Overflow::Auto)
            .direction(layout::Direction::Rtl)
            .child(div().id("c").size(300, 300))
            .child(div().id("fixed").fixed().left(10).top(10).size(20, 20)),
        "",
    );
    let p = id(&tree, "p");
    let fixed = id(&tree, "fixed");
    let c = id(&tree, "c");
    let original = tree.bounds(c);
    let fb = tree.bounds(fixed);
    assert_eq!(tree.scroll_metrics(p).unwrap().min.x, -200.);
    assert!(tree.scroll_to(p, Point::new(-100., 50.)));
    assert_eq!(tree.bounds(c).origin.x, original.origin.x + 100.);
    assert_eq!(tree.bounds(fixed), fb);
}
#[test]
fn reconcile_retains_scroll_offset_and_clamps_after_resize() {
    let mut tree = build(pane(), "");
    let p = id(&tree, "pane");
    tree.scroll_to(p, Point::new(100., 250.));
    tree.reconcile_root(pane());
    layout(&mut tree);
    assert_eq!(
        tree.scroll_metrics(p).unwrap().offset,
        Point::new(100., 250.)
    );
    tree.style_mut(id(&tree, "content")).layout.size.height =
        layout::Dimension::length(120.).into();
    layout(&mut tree);
    assert_eq!(tree.scroll_metrics(p).unwrap().offset.y, 20.);
}

#[test]
fn stable_both_edges_insets_content_without_changing_outer_size() {
    for display in [
        layout::Display::Block,
        layout::Display::Flex,
        layout::Display::Grid,
    ] {
        for css in [
            "width:200px",
            "width:50%",
            "width:200px;box-sizing:border-box;padding:10px",
        ] {
            let tree = build(
                div().size(400, 300).child(
                    div()
                        .id("p")
                        .height(100)
                        .display(display)
                        .scrollbar_mode(ScrollbarMode::Classic)
                        .child(div().id("c").width(style::pct(100.0)).height(300)),
                ),
                &format!("#p{{{css};overflow:auto;scrollbar-gutter:stable both-edges}}"),
            );
            let p = id(&tree, "p");
            let c = id(&tree, "c");
            let w = tree.scroll_options().width;
            assert_eq!(tree.bounds(p).size.width, 200., "{display:?} {css}");
            assert_eq!(tree.layout_result(p).scrollbar_size.width, w * 2.);
            assert_eq!(tree.bounds(c).origin.x, tree.content_bounds(p).origin.x);
            assert_eq!(
                tree.bounds(c).size.width,
                tree.content_bounds(p).size.width,
                "{display:?} {css}"
            );
            assert_eq!(tree.scroll_metrics(p).unwrap().max.x, 0.);
        }
    }
}

#[test]
fn wheel_default_respects_prevention_and_scrollbar_layers_cover_positioned_content() {
    let mut tree = build(
        div()
            .size(100, 100)
            .id("p")
            .overflow(Overflow::Auto)
            .on_mouse_scroll(|_: MouseEvent| EventResponse::PREVENT_DEFAULT)
            .child(div().relative().z_index(100).size(300, 300)),
        "",
    );
    let p = id(&tree, "p");
    tree.pointer_moved(Some(Point::new(10., 10.)));
    tree.dispatch_mouse_scroll(
        MouseWheel {
            x: 0.,
            y: -20.,
            unit: WheelUnit::Pixels,
        },
        ModifiersState::empty(),
    );
    assert_eq!(tree.scroll_metrics(p).unwrap().offset, Point::default());
    let bar = tree.scrollbar_geometry(p, ScrollAxis::Vertical).unwrap();
    assert_eq!(
        tree.hit_test(Point::new(bar.track.origin.x + 1., 10.)),
        Some(HitTarget::Element(p))
    );
}
#[test]
fn focus_reveals_offscreen_controls_and_keys_scroll_the_focused_container() {
    let mut tree = build(
        div()
            .id("p")
            .size(100, 100)
            .overflow(Overflow::Auto)
            .child(div().size(50, 250))
            .child(div().id("button").tag("button").size(50, 30)),
        "",
    );
    let p = id(&tree, "p");
    let button = id(&tree, "button");
    tree.set_focused(Some(button));
    assert_eq!(tree.scroll_metrics(p).unwrap().offset.y, 180.);
    tree.set_focused(Some(p));
    assert!(tree.scroll_key(
        &core::event::Key::Named(core::event::NamedKey::Home),
        ModifiersState::empty()
    ));
    assert_eq!(tree.scroll_metrics(p).unwrap().offset.y, 0.);
}
#[test]
fn large_list_scroll_never_invokes_widget_layout_again() {
    use std::{cell::Cell, rc::Rc};
    use voidui::core::{
        context::LayoutContext,
        layout::{LayoutInput, LayoutOutput},
        widget::{Widget, WidgetBuilder},
    };
    struct Probe(Rc<Cell<usize>>);
    impl Widget for Probe {
        fn layout(&mut self, inputs: LayoutInput, cx: LayoutContext<'_, '_>) -> LayoutOutput {
            self.0.set(self.0.get() + 1);
            layout::layout_leaf(cx.layout_style(), inputs, |_, _| Size {
                width: 100.,
                height: 20.,
            })
        }
    }
    let count = Rc::new(Cell::new(0));
    let mut pane = div().id("p").size(100, 100).overflow(Overflow::Auto);
    for _ in 0..1000 {
        pane = pane.child(WidgetBuilder::from_widget(Probe(count.clone())));
    }
    let mut tree = build(pane, "");
    let p = id(&tree, "p");
    let before = count.get();
    for i in 0..100 {
        tree.scroll_to(p, Point::new(0., i as f32));
        tree.hit_test(Point::new(10., 10.));
        assert!(!tree.update_styles(Instant::now()).layout);
    }
    assert_eq!(count.get(), before);
}
#[cfg(feature = "editing")]
#[test]
fn textarea_scrollbars_follow_editing_and_chain_at_the_boundary() {
    use std::borrow::Cow;
    let fonts = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    fonts
        .add_fonts(vec![Cow::Borrowed(include_bytes!(
            "../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
        ))])
        .unwrap();
    let cache = TextLayoutCache::new(Arc::new(TextSystem::new(Arc::new(fonts))));
    let editor = Editor::new("one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten");
    let mut tree = WidgetTree::new();
    tree.build_root(
        div()
            .id("p")
            .size(200, 100)
            .overflow(Overflow::Auto)
            .child(textarea(&editor).id("edit").width(180).height(60))
            .child(div().size(100, 200)),
    );
    tree.layout(
        Size {
            width: AvailableSpace::Definite(400.),
            height: AvailableSpace::Definite(300.),
        },
        &cache,
    );
    let p = id(&tree, "p");
    let edit = id(&tree, "edit");
    let m = tree.scroll_metrics(edit).unwrap();
    assert!(m.max.y > 100.);
    assert!(
        tree.scrollbar_geometry(edit, ScrollAxis::Vertical)
            .is_some()
    );
    tree.pointer_moved(Some(Point::new(10., 10.)));
    tree.dispatch_mouse_scroll(
        MouseWheel {
            x: 0.,
            y: -(m.max.y + 10.),
            unit: WheelUnit::Pixels,
        },
        ModifiersState::empty(),
    );
    assert_eq!(tree.scroll_metrics(edit).unwrap().offset.y, m.max.y);
    assert_eq!(tree.scroll_metrics(p).unwrap().offset.y, 10.);
    tree.dispatch_input(
        edit,
        &core::input::InputEvent::Key(core::input::KeyInput {
            key: core::event::Key::Named(core::event::NamedKey::Home),
            modifiers: ModifiersState::SUPER,
            repeat: false,
        }),
        &cache,
    );
    tree.refresh_scroll_content(&cache);
    assert!(tree.scroll_metrics(edit).unwrap().offset.y < m.max.y);
}

#[test]
fn runtime_and_reconciled_placement_changes_reflow_without_rebuilding_elements() {
    let view = |mode| {
        div()
            .id("p")
            .size(100, 100)
            .overflow(Overflow::Auto)
            .scrollbar_mode(mode)
            .child(div().height(400))
    };
    let mut tree = build(view(ScrollbarMode::Overlay), "");
    let p = id(&tree, "p");
    tree.scroll_to(p, Point::new(0., 100.));
    tree.reconcile_root(view(ScrollbarMode::Classic));
    assert!(tree.update_styles(Instant::now()).layout);
    layout(&mut tree);
    assert_eq!(id(&tree, "p"), p);
    assert_eq!(tree.scroll_metrics(p).unwrap().offset.y, 100.);
    assert!(tree.layout_result(p).scrollbar_size.width > 0.);
    assert!(tree.set_scrollbar_mode(p, Some(ScrollbarMode::Overlay)));
    layout(&mut tree);
    assert_eq!(tree.layout_result(p).scrollbar_size.width, 0.);
    let mut options = tree.scroll_options();
    options.min_thumb_length += 1.;
    tree.set_scroll_options(options);
    assert!(!tree.update_styles(Instant::now()).layout);
}
#[test]
fn both_edges_respects_min_max_and_zero_width_boxes() {
    for (css, expected) in [
        ("width:0", 0.),
        ("width:50px;min-width:120px", 120.),
        ("width:200px;max-width:120px", 120.),
        ("width:200px;padding:10px", 220.),
    ] {
        let tree = build(
            div()
                .id("p")
                .height(80)
                .scrollbar_mode(ScrollbarMode::Classic)
                .child(div().size(300, 300)),
            &format!("#p{{{css};overflow:auto;scrollbar-gutter:stable both-edges}}"),
        );
        assert_eq!(tree.bounds(id(&tree, "p")).size.width, expected, "{css}");
    }
}
#[test]
fn hidden_ancestors_and_key_scrolling_respect_overscroll_containment() {
    let mut tree = build(
        div()
            .id("outer")
            .size(100, 100)
            .overflow(Overflow::Auto)
            .child(
                div()
                    .id("inner")
                    .size(90, 90)
                    .overflow(Overflow::Auto)
                    .child(div().size(70, 200)),
            )
            .child(div().height(200)),
        "#inner{overscroll-behavior:contain}",
    );
    let inner = id(&tree, "inner");
    let outer = id(&tree, "outer");
    tree.set_focused(Some(inner));
    tree.scroll_to(inner, Point::new(0., 1000.));
    assert!(tree.scroll_key(
        &core::event::Key::Named(core::event::NamedKey::ArrowDown),
        ModifiersState::empty()
    ));
    assert_eq!(tree.scroll_metrics(outer).unwrap().offset.y, 0.);
}
