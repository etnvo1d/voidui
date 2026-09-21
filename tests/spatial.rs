//! Post-layout positioning follows CSS while preserving retained layout results.
use std::{sync::Arc, time::Instant};
use voidui::{
    core::{
        geometry::Point,
        layout::{AvailableSpace, Size},
        top_layer::HitTarget,
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
fn layout(t: &mut WidgetTree) {
    t.layout(
        Size {
            width: AvailableSpace::Definite(600.),
            height: AvailableSpace::Definite(400.),
        },
        &cache(),
    );
}
fn build(v: impl IntoElement, css: &str) -> WidgetTree {
    let mut t = WidgetTree::new();
    t.build_root(v);
    t.set_stylesheets(vec![Stylesheet::parse(css).unwrap()]);
    layout(&mut t);
    t
}
fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 0.01, "{a} != {b}");
}
#[test]
fn sticky_preserves_flow_and_stops_at_containing_block() {
    let mut t = build(
        div()
            .size(200, 100)
            .overflow(Overflow::Auto)
            .id("scroll")
            .child(
                div()
                    .id("section")
                    .child(div().id("head").size(200, 20).sticky().top(0))
                    .child(div().size(200, 180)),
            )
            .child(div().size(200, 200)),
        "",
    );
    let sc = t.find_by_id("scroll").unwrap();
    let head = t.find_by_id("head").unwrap();
    let normal = *t.layout_result(head);
    t.hit_test(Point::new(5., 5.));
    let order = t.paint_order_rebuilds();
    for (scroll, expected) in [(50., 0.), (190., -10.), (0., 0.)] {
        t.scroll_to(sc, Point::new(0., scroll));
        close(t.bounds(head).origin.y, expected);
        assert_eq!(t.layout_result(head).location, normal.location);
    }
    assert_eq!(
        t.hit_test(Point::new(5., 5.)),
        Some(HitTarget::Element(head))
    );
    assert_eq!(t.paint_order_rebuilds(), order);
    assert!(!t.update_styles(Instant::now()).layout);
}
#[test]
fn transform_composition_origin_hit_and_inverse_share_coordinates() {
    let t = build(
        div().size(600, 400).child(div().id("box").size(100, 40)),
        "#box {transform:translate(150px,100px) rotate(90deg);transform-origin:0 0}",
    );
    let id = t.find_by_id("box").unwrap();
    let b = t.visual_bounds(id);
    close(b.origin.x, 110.);
    close(b.origin.y, 100.);
    close(b.size.width, 40.);
    close(b.size.height, 100.);
    assert_eq!(
        t.hit_test(Point::new(130., 110.)),
        Some(HitTarget::Element(id))
    );
    let p = t.window_to_local(id, Point::new(130., 110.)).unwrap();
    close(p.x, 10.);
    close(p.y, 20.);
    close(t.bounds(id).size.width, 100.);
}
#[test]
fn transformed_overflow_clip_uses_local_shape_not_bounding_rect() {
    let t = build(
        div().size(600, 400).child(
            div()
                .id("clip")
                .size(100, 100)
                .overflow(Overflow::Hidden)
                .child(div().id("child").size(200, 200)),
        ),
        "#clip {transform:translate(200px,100px) rotate(45deg);transform-origin:0 0}",
    );
    let child = t.find_by_id("child").unwrap();
    assert_eq!(
        t.hit_test(Point::new(200., 150.)),
        Some(HitTarget::Element(child))
    );
    assert_ne!(
        t.hit_test(Point::new(140., 105.)),
        Some(HitTarget::Element(child))
    );
}
#[test]
fn transformed_ancestor_contains_fixed_and_absolute_descendants() {
    let t = build(
        div().size(600, 400).child(
            div()
                .id("cb")
                .size(200, 100)
                .child(div().id("fixed").fixed().left(10).top(20).size(20, 20)),
        ),
        "#cb {transform:translate(100px,50px)}",
    );
    let id = t.find_by_id("fixed").unwrap();
    close(t.visual_bounds(id).origin.x, 110.);
    close(t.visual_bounds(id).origin.y, 70.);
}
#[test]
fn percentage_translation_recomputes_after_resize_and_singular_is_not_hit() {
    let mut t = build(
        div().size(600, 400).child(div().id("box").size(100, 40)),
        "#box {transform:translateX(50%)}",
    );
    let id = t.find_by_id("box").unwrap();
    close(t.visual_bounds(id).origin.x, 50.);
    t.set_stylesheets(vec![
        Stylesheet::parse("#box {transform:scale(0)}").unwrap(),
    ]);
    layout(&mut t);
    assert!(t.window_to_local(id, Point::default()).is_none());
    assert_ne!(
        t.hit_test(Point::new(50., 20.)),
        Some(HitTarget::Element(id))
    );
}
#[test]
fn css_transform_parser_is_strict_and_retains_math() {
    for css in [
        "translate(10px, calc(50% - 2px)) rotate(.25turn) scale(-1,2)",
        "matrix(1,0,0,1,10,20)",
        "skewX(15grad)",
        "none",
    ] {
        assert!(
            css.parse::<Transform>().is_ok(),
            "{css}: {:?}",
            css.parse::<Transform>()
        );
    }
    for css in [
        "translate(1)",
        "rotate(3)",
        "scale(1,2,3)",
        "translate(10px 20px)",
        "none rotate(0)",
        "matrix3d(1)",
    ] {
        assert!(css.parse::<Transform>().is_err(), "{css}");
    }
    let tr: Transform = "translate(calc(50% - 2px),0)".parse().unwrap();
    close(tr.matrix(100., 20.).0[4], 48.);
}
#[test]
fn transform_overflow_expands_without_shrinking_normal_flow() {
    for (transform, width, height) in [
        ("translateX(150px)", 250., 100.),
        ("scale(3)", 300., 300.),
        ("scale(.5)", 200., 100.),
        ("translateX(-200px)", 200., 100.),
    ] {
        let t = build(
            div()
                .id("scroll")
                .size(200, 100)
                .overflow(Overflow::Auto)
                .child(div().id("box").size(100, 100)),
            &format!("#box {{transform:{transform};transform-origin:0 0}}"),
        );
        let m = t.scroll_metrics(t.find_by_id("scroll").unwrap()).unwrap();
        close(m.content.width, width);
        close(m.content.height, height);
    }
}
#[test]
fn sticky_auto_bottom_oversize_and_hidden_scrollport() {
    let mut t = build(
        div()
            .id("scroll")
            .size(200, 100)
            .overflow(Overflow::Auto)
            .child(div().id("head").size(100, 20))
            .child(div().size(200, 380)),
        "#head {position:sticky;top:10%}",
    );
    let sc = t.find_by_id("scroll").unwrap();
    let head = t.find_by_id("head").unwrap();
    t.scroll_to(sc, Point::new(0., 120.));
    close(t.bounds(head).origin.y, 10.);
    t.set_stylesheets(vec![
        Stylesheet::parse("#head {position:sticky;top:auto;bottom:auto}").unwrap(),
    ]);
    layout(&mut t);
    close(t.bounds(head).origin.y, -120.);
    let mut t = build(
        div()
            .id("scroll")
            .size(200, 100)
            .overflow(Overflow::Auto)
            .child(
                div()
                    .id("hidden")
                    .size(200, 200)
                    .overflow(Overflow::Hidden)
                    .child(div().id("head").size(100, 20).sticky().top(0)),
            )
            .child(div().size(200, 200)),
        "",
    );
    let sc = t.find_by_id("scroll").unwrap();
    let head = t.find_by_id("head").unwrap();
    t.scroll_to(sc, Point::new(0., 60.));
    close(t.bounds(head).origin.y, -60.);
    let mut t = build(
        div()
            .id("scroll")
            .size(200, 100)
            .overflow(Overflow::Auto)
            .child(div().id("head").size(100, 150).sticky().top(10).bottom(20))
            .child(div().size(200, 200)),
        "",
    );
    let sc = t.find_by_id("scroll").unwrap();
    let head = t.find_by_id("head").unwrap();
    t.scroll_to(sc, Point::new(0., 50.));
    close(t.bounds(head).origin.y, 10.);
}
#[test]
fn transform_transition_updates_geometry_without_relayout_or_reordering() {
    use std::time::Duration;
    let mut t = build(
        div().size(600, 400).child(div().id("box").size(100, 40)),
        "#box {transform:translateX(0);transition:transform 1s linear}#box.active {transform:translateX(100px)}",
    );
    let id = t.find_by_id("box").unwrap();
    let now = Instant::now();
    t.hit_test(Point::new(1., 1.));
    let order = t.paint_order_rebuilds();
    t.set_classes(id, "active");
    assert!(!t.update_styles(now).layout);
    assert!(!t.update_styles(now + Duration::from_millis(500)).layout);
    close(t.visual_bounds(id).origin.x, 50.);
    assert_eq!(
        t.hit_test(Point::new(120., 20.)),
        Some(HitTarget::Element(id))
    );
    assert_eq!(t.paint_order_rebuilds(), order);
    t.update_styles(now + Duration::from_secs(2));
    close(t.visual_bounds(id).origin.x, 100.);
    assert!(
        t.next_animation_frame(now + Duration::from_secs(2))
            .is_none()
    );
}
#[test]
fn transformed_scrollbar_and_pointer_events_use_inverse_coordinates() {
    use std::{cell::Cell, rc::Rc};
    let seen = Rc::new(Cell::new(Point::default()));
    let output = seen.clone();
    let mut t = build(
        div()
            .size(600, 400)
            .child(div().id("box").size(100, 40).on_mouse_down(
                move |e: core::event::MouseEvent| {
                    output.set(e.local_position);
                },
            )),
        "#box {transform:translate(100px,50px) scale(2);transform-origin:0 0}",
    );
    t.pointer_moved(Some(Point::new(120., 70.)));
    t.dispatch_mouse_button(core::event::MouseButton::Left, true, Default::default());
    close(seen.get().x, 10.);
    close(seen.get().y, 10.);
}

#[test]
fn changing_transform_overflow_clamps_scroll_and_refreshes_positions() {
    let mut t = build(
        div()
            .id("scroll")
            .size(100, 100)
            .overflow(Overflow::Auto)
            .child(div().id("box").size(100, 100)),
        "#box {transform:translateY(200px)}#box.small {transform:translateY(20px)}",
    );
    let sc = t.find_by_id("scroll").unwrap();
    let b = t.find_by_id("box").unwrap();
    t.scroll_to(sc, Point::new(0., 200.));
    t.set_classes(b, "small");
    assert!(!t.update_styles(Instant::now()).layout);
    close(t.scroll_metrics(sc).unwrap().max.y, 20.);
    close(t.scroll_metrics(sc).unwrap().offset.y, 20.);
    close(t.bounds(b).origin.y, -20.);
    close(t.visual_bounds(b).origin.y, 0.);
}
#[test]
fn nested_transform_clipping_and_containing_blocks_survive_reconciliation() {
    let view = || {
        div().size(600, 400).child(
            div()
                .id("parent")
                .size(100, 100)
                .overflow(Overflow::Hidden)
                .child(div().id("child").size(100, 100)),
        )
    };
    let mut t = build(
        view(),
        "#parent{transform:translate(100px,50px) scale(2);transform-origin:0 0}#child{transform:translate(75px,0)}",
    );
    let child = t.find_by_id("child").unwrap();
    close(t.visual_bounds(child).origin.x, 250.);
    assert_eq!(
        t.hit_test(Point::new(275., 75.)),
        Some(HitTarget::Element(child))
    );
    assert_ne!(
        t.hit_test(Point::new(325., 75.)),
        Some(HitTarget::Element(child))
    );
    t.reconcile_root(view());
    layout(&mut t);
    assert_eq!(t.find_by_id("child"), Some(child));
    close(t.visual_bounds(child).origin.x, 250.);
}
#[test]
fn transform_none_and_identity_have_distinct_stacking_semantics() {
    let mut t = build(
        div()
            .size(300, 100)
            .child(
                div()
                    .id("context")
                    .size(100, 100)
                    .child(div().id("high").absolute().inset(0).z_index(10)),
            )
            .child(
                div()
                    .id("other")
                    .absolute()
                    .inset(0)
                    .size(100, 100)
                    .z_index(1),
            ),
        "#context {transform:translate(0)}#context.none{transform:none}",
    );
    let other = t.find_by_id("other").unwrap();
    assert_eq!(
        t.hit_test(Point::new(10., 10.)),
        Some(HitTarget::Element(other))
    );
    let context = t.find_by_id("context").unwrap();
    t.set_classes(context, "none");
    layout(&mut t);
    assert_eq!(
        t.hit_test(Point::new(10., 10.)),
        Some(HitTarget::Element(t.find_by_id("high").unwrap()))
    );
}
#[test]
fn sticky_percentage_math_and_inherited_insets_remain_owned() {
    let mut t = build(
        div()
            .id("scroll")
            .size(200, 100)
            .overflow(Overflow::Auto)
            .child(div().id("head").size(100, 20))
            .child(div().size(200, 200)),
        "#head {position:sticky;top:calc(10% + 2px)}",
    );
    let sc = t.find_by_id("scroll").unwrap();
    let head = t.find_by_id("head").unwrap();
    t.scroll_to(sc, Point::new(0., 60.));
    close(t.bounds(head).origin.y, 12.);
    let mut t = build(
        div()
            .id("scroll")
            .size(200, 100)
            .overflow(Overflow::Auto)
            .child(div().id("head").size(100, 20))
            .child(div().size(200, 200)),
        "#scroll {position:sticky;top:10px}#head {position:sticky;top:inherit}",
    );
    let sc = t.find_by_id("scroll").unwrap();
    let head = t.find_by_id("head").unwrap();
    t.scroll_to(sc, Point::new(0., 60.));
    close(t.bounds(head).origin.y, 20.);
}

#[test]
fn shrinking_transform_removes_classic_auto_gutters() {
    let mut t = build(
        div()
            .size(200, 100)
            .id("scroll")
            .overflow(Overflow::Auto)
            .scrollbar_mode(ScrollbarMode::Classic)
            .child(div().id("box").size(100, 80)),
        "#box {transform:translateY(100px)}#box.small{transform:translateY(0)}",
    );
    let sc = t.find_by_id("scroll").unwrap();
    let b = t.find_by_id("box").unwrap();
    assert!(t.scrollbar_geometry(sc, ScrollAxis::Vertical).is_some());
    t.set_classes(b, "small");
    assert!(t.update_styles(Instant::now()).layout);
    layout(&mut t);
    assert!(t.scrollbar_geometry(sc, ScrollAxis::Vertical).is_none());
    close(t.scroll_metrics(sc).unwrap().viewport.width, 200.);
}

#[test]
fn horizontal_sticky_tracks_rtl_scroll_origin() {
    let mut t = build(
        div()
            .id("scroll")
            .size(200, 100)
            .overflow(Overflow::Auto)
            .flex_row()
            .child(
                div()
                    .id("head")
                    .size(100, 50)
                    .flex_shrink(0)
                    .sticky()
                    .right(10),
            )
            .child(div().size(400, 50).flex_shrink(0)),
        "#scroll {direction:rtl}",
    );
    let sc = t.find_by_id("scroll").unwrap();
    let h = t.find_by_id("head").unwrap();
    for x in [-80., -180., -250.] {
        t.scroll_to(sc, Point::new(x, 0.));
        close(t.bounds(h).origin.x - t.bounds(sc).origin.x, 90.);
    }
}

#[test]
fn sticky_inset_transition_and_resized_transform_do_not_restart() {
    use std::time::Duration;
    let mut t = build(
        div()
            .id("scroll")
            .size(200, 100)
            .overflow(Overflow::Auto)
            .child(div().id("head").size(100, 20))
            .child(div().size(200, 300)),
        "#head {position:sticky;top:0;transition:top 1s linear}#head.active{top:20px}",
    );
    let sc = t.find_by_id("scroll").unwrap();
    let h = t.find_by_id("head").unwrap();
    t.scroll_to(sc, Point::new(0., 50.));
    let now = Instant::now();
    t.set_classes(h, "active");
    assert!(!t.update_styles(now).layout);
    assert!(!t.update_styles(now + Duration::from_millis(500)).layout);
    close(t.bounds(h).origin.y, 10.);
    let mut t = build(
        div().size(600, 400).child(div().id("box").height(40)),
        "#box {width:100px;transform:translateX(0);transition:transform 1s linear}#box.active{transform:translateX(100px)}#box.wide{width:200px}",
    );
    let b = t.find_by_id("box").unwrap();
    let now = Instant::now();
    t.set_classes(b, "active");
    t.update_styles(now);
    t.update_styles(now + Duration::from_millis(300));
    t.set_classes(b, "active wide");
    t.update_styles(now + Duration::from_millis(300));
    t.layout_computed(
        Size {
            width: AvailableSpace::Definite(600.),
            height: AvailableSpace::Definite(400.),
        },
        &cache(),
    );
    t.update_styles(now + Duration::from_millis(500));
    close(t.visual_bounds(b).origin.x, 50.);
}
