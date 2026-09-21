//! Stacking and positioning share assertions for pixels/scene order and hit order.
use std::{borrow::Cow, sync::Arc, time::Instant};
use voidui::{
    core::{
        geometry::{Point, Rect},
        layout::{self, AvailableSpace, Size},
        top_layer::HitTarget,
        widget_tree::WidgetTree,
    },
    div,
    render::{
        self, AtlasKey, AtlasTile, DevicePixels, Painter, ParleyTextSystem, PlatformAtlas, Scene,
        TextLayoutCache, TextSystem, px, size,
    },
    style::{color::Color, css::Stylesheet},
};
fn cache() -> TextLayoutCache {
    TextLayoutCache::new(Arc::new(TextSystem::new(Arc::new(
        ParleyTextSystem::new_without_system_fonts("unused"),
    ))))
}
fn space() -> Size<AvailableSpace> {
    Size {
        width: AvailableSpace::Definite(400.0),
        height: AvailableSpace::Definite(300.0),
    }
}
fn build(root: impl voidui::core::element::IntoElement, css: &str) -> WidgetTree {
    let mut tree = WidgetTree::new();
    tree.build_root(root);
    tree.set_stylesheets(vec![Stylesheet::parse(css).unwrap()]);
    tree.layout(space(), &cache());
    tree
}
fn update(tree: &mut WidgetTree) {
    let change = tree.update_styles(Instant::now());
    let _ = change;
    tree.layout_computed(space(), &cache());
}
fn hit(tree: &WidgetTree, x: f32, y: f32) -> Option<HitTarget> {
    tree.hit_test(Point::new(x, y))
}
fn assert_box(
    tree: &WidgetTree,
    id: voidui::core::widget::WidgetId,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    let actual = tree.bounds(id);
    let expected = Rect::from_xywh(x, y, w, h);
    assert_eq!(actual, expected);
}
struct NoGlyphs;
impl PlatformAtlas for NoGlyphs {
    fn get_or_insert_with<'a>(
        &self,
        _: &AtlasKey,
        _: &mut dyn FnMut() -> render::Result<Option<(render::Size<DevicePixels>, Cow<'a, [u8]>)>>,
    ) -> render::Result<Option<AtlasTile>> {
        panic!("no text")
    }
    fn remove(&self, _: &AtlasKey) {}
}
fn paint(tree: &WidgetTree) -> Scene {
    let text = Arc::new(TextSystem::new(Arc::new(
        ParleyTextSystem::new_without_system_fonts("unused"),
    )));
    let mut scene = Scene::default();
    let mut painter =
        Painter::new(&mut scene, &NoGlyphs, text, size(px(400.0), px(300.0)), 1.0).unwrap();
    tree.draw(&mut painter).unwrap();
    drop(painter);
    scene.finish();
    scene
}
fn colors(scene: &Scene) -> Vec<render::Hsla> {
    scene
        .quads
        .iter()
        .filter_map(|q| q.background.as_solid())
        .collect()
}
fn color(s: &str) -> render::Hsla {
    s.parse::<Color>().unwrap().into()
}

#[test]
fn static_is_the_initial_position_and_insets_do_not_move_it() {
    let tree = build(
        div().child(div().id("box")),
        "#box{width:20px;height:20px;left:100px;top:100px;z-index:100}",
    );
    let root = tree.root().unwrap();
    assert_box(&tree, tree.children(root)[0], 0.0, 0.0, 20.0, 20.0);
}
#[test]
fn integer_z_order_and_hit_testing_agree_with_dom_ties() {
    let tree = build(
        div().id("root").child(div().id("a")).child(div().id("b")),
        "#root{width:100px;height:100px;background:white;position:relative}#a,#b{position:absolute;inset:0}#a{background:red;z-index:3}#b{background:blue;z-index:1}",
    );
    let root = tree.root().unwrap();
    let a = tree.children(root)[0];
    assert_eq!(hit(&tree, 20.0, 20.0), Some(HitTarget::Element(a)));
    assert_eq!(
        colors(&paint(&tree)),
        vec![color("white"), color("blue"), color("red")]
    );
}
#[test]
fn negative_contexts_are_above_root_background_below_normal_flow() {
    let tree = build(
        div().id("root").child(div().id("a")).child(div().id("b")),
        "#root{width:100px;height:100px;background:white;position:relative}#a{position:absolute;inset:0;background:red;z-index:-1}#b{width:100px;height:100px;background:blue}",
    );
    let b = tree.children(tree.root().unwrap())[1];
    assert_eq!(hit(&tree, 20.0, 20.0), Some(HitTarget::Element(b)));
    assert_eq!(
        colors(&paint(&tree)),
        vec![color("white"), color("red"), color("blue")]
    );
}
#[test]
fn descendants_escape_auto_but_not_integer_or_isolated_contexts() {
    for (parent, top) in [
        ("position:relative", true),
        ("position:relative;z-index:0", false),
        ("isolation:isolate", false),
    ] {
        let css = format!(
            "#root{{width:100px;height:100px;position:relative}}#parent{{{parent};width:100px;height:100px}}#child{{position:absolute;inset:0;z-index:999;background:red}}#sibling{{position:absolute;inset:0;z-index:1;background:blue}}"
        );
        let tree = build(
            div()
                .id("root")
                .child(div().id("parent").child(div().id("child")))
                .child(div().id("sibling")),
            &css,
        );
        let root = tree.root().unwrap();
        let parent = tree.children(root)[0];
        let child = tree.children(parent)[0];
        let sibling = tree.children(root)[1];
        assert_eq!(
            hit(&tree, 10.0, 10.0),
            Some(HitTarget::Element(if top { child } else { sibling })),
            "{css}"
        );
    }
}
#[test]
fn z_index_applies_to_unpositioned_flex_and_grid_items() {
    for display in ["flex", "grid"] {
        let css = format!(
            "#root{{display:{display};width:100px;height:100px}}#a{{width:100px;height:100px;flex-shrink:0;z-index:2;background:red}}#b{{width:100px;height:100px;flex-shrink:0;margin-left:-100px;z-index:1;background:blue}}"
        );
        let tree = build(
            div().id("root").child(div().id("a")).child(div().id("b")),
            &css,
        );
        assert_eq!(colors(&paint(&tree)).last(), Some(&color("red")));
    }
}
#[test]
fn fixed_boxes_use_viewport_and_escape_ancestor_overflow() {
    let tree = build(
        div()
            .id("root")
            .child(div().id("clip").child(div().id("fixed"))),
        "#root{width:100px;height:100px;padding:20px}#clip{width:20px;height:20px;overflow:hidden;position:relative}#fixed{position:fixed;right:10px;bottom:15px;width:40px;height:30px;background:red}",
    );
    let root = tree.root().unwrap();
    let fixed = tree.children(tree.children(root)[0])[0];
    assert_box(&tree, fixed, 350.0, 255.0, 40.0, 30.0);
    assert_eq!(hit(&tree, 360.0, 260.0), Some(HitTarget::Element(fixed)));
}
#[test]
fn absolute_uses_nearest_positioned_ancestor_not_static_parent() {
    let tree = build(
        div()
            .id("root")
            .child(div().id("static").child(div().id("absolute"))),
        "#root{position:relative;width:200px;height:100px;padding:10px;border:2px solid black}#static{width:30px;height:30px;padding:10px;overflow:hidden}#absolute{position:absolute;right:5px;bottom:5px;width:20px;height:10px;background:red}",
    );
    let root = tree.root().unwrap();
    let abs = tree.children(tree.children(root)[0])[0];
    assert_box(&tree, abs, 197.0, 107.0, 20.0, 10.0);
    assert_eq!(hit(&tree, 200.0, 110.0), Some(HitTarget::Element(abs)));
}
#[test]
fn visibility_and_pointer_events_inherit_but_children_can_override() {
    let tree = build(
        div()
            .id("root")
            .child(div().id("front").child(div().id("child"))),
        "#root{position:relative;width:100px;height:100px}#front{position:absolute;inset:0;pointer-events:none;visibility:hidden}#child{position:absolute;left:10px;top:10px;width:20px;height:20px;pointer-events:auto;visibility:visible;background:red}",
    );
    let root = tree.root().unwrap();
    let child = tree.children(tree.children(root)[0])[0];
    assert_eq!(hit(&tree, 15.0, 15.0), Some(HitTarget::Element(child)));
    assert_eq!(hit(&tree, 50.0, 50.0), Some(HitTarget::Element(root)));
}
#[test]
fn modal_top_layer_ignores_z_index_escapes_clips_and_restores_focus() {
    let mut tree = build(
        div()
            .id("root")
            .child(div().id("button").attr("tabindex", "0"))
            .child(
                div().id("clip").child(
                    div()
                        .id("first")
                        .tag("dialog")
                        .child(div().id("second").tag("dialog")),
                ),
            )
            .child(div().id("huge")),
        "#root{width:400px;height:300px}#clip{position:relative;overflow:hidden;width:10px;height:10px}dialog{width:100px;height:80px}#first{background:red}#second{background:blue;z-index:-999}#huge{position:fixed;inset:0;z-index:2147483647;background:green}dialog::backdrop{background:rgb(0 0 0 / .2)}",
    );
    let root = tree.root().unwrap();
    let button = tree.children(root)[0];
    let clip = tree.children(root)[1];
    let first = tree.children(clip)[0];
    let second = tree.children(first)[0];
    tree.set_focused(Some(button));
    tree.show_modal(first).unwrap();
    update(&mut tree);
    assert_box(&tree, first, 150.0, 110.0, 100.0, 80.0);
    assert!(tree.is_inert(button));
    assert!(!tree.set_focused(Some(button)));
    assert_eq!(hit(&tree, 160.0, 120.0), Some(HitTarget::Element(first)));
    tree.show_modal(second).unwrap();
    update(&mut tree);
    assert_eq!(hit(&tree, 160.0, 120.0), Some(HitTarget::Element(second)));
    assert_eq!(hit(&tree, 5.0, 5.0), Some(HitTarget::Backdrop(second)));
    assert_eq!(colors(&paint(&tree)).last(), Some(&color("blue")));
    tree.close_top_layer(second);
    update(&mut tree);
    assert_eq!(tree.active_modal(), Some(first));
    assert_eq!(tree.focused(), Some(first));
    tree.close_top_layer(first);
    update(&mut tree);
    assert_eq!(tree.focused(), Some(button));
    assert!(!tree.is_inert(button));
}
#[test]
fn manual_popover_styles_use_standard_pseudo_class_and_backdrop() {
    let mut tree = build(
        div().child(div().id("tip").attr("popover", "manual")),
        "#tip{width:100px;height:40px;background:red}#tip:popover-open{background:blue}#tip::backdrop{pointer-events:none}",
    );
    let tip = tree.children(tree.root().unwrap())[0];
    assert_eq!(tree.layout_style(tip).display, layout::Display::None);
    tree.show_popover(tip).unwrap();
    update(&mut tree);
    assert_eq!(tree.top_layer().collect::<Vec<_>>(), vec![tip]);
    assert_eq!(
        tree.paint_style(tip).background,
        "blue".parse::<Color>().unwrap()
    );
    assert_eq!(tree.active_modal(), None);
    assert_eq!(hit(&tree, 160.0, 140.0), Some(HitTarget::Element(tip)));
    assert_ne!(hit(&tree, 0.0, 0.0), Some(HitTarget::Backdrop(tip)));
}
#[test]
fn z_index_change_is_paint_only_and_paint_order_is_cached() {
    let mut tree = build(
        div().id("root").child(div().id("a")).child(div().id("b")),
        "#root{position:relative;width:100px;height:100px}#a,#b{position:absolute;inset:0}#a{background:red;z-index:0}#b{background:blue;z-index:1}#a.front{z-index:2}",
    );
    let a = tree.children(tree.root().unwrap())[0];
    paint(&tree);
    let count = tree.paint_order_rebuilds();
    paint(&tree);
    assert_eq!(count, tree.paint_order_rebuilds());
    tree.set_classes(a, "front");
    let change = tree.update_styles(Instant::now());
    assert!(change.paint && !change.layout);
    assert_eq!(hit(&tree, 10.0, 10.0), Some(HitTarget::Element(a)));
    assert_eq!(tree.paint_order_rebuilds(), count + 1);
}
#[test]
fn rejects_non_css_overlay_shortcuts_and_bad_z_index() {
    for css in [
        "z-index:1.5",
        "position:overlay",
        "overlay:modal",
        "visibility:inert",
        "pointer-events:click-through",
    ] {
        assert!(
            Stylesheet::parse(&format!("div{{{css}}}")).is_err(),
            "{css}"
        );
    }
}

#[test]
fn dynamic_nested_modals_keep_ids_and_focus_scope_without_a_depth_limit() {
    let mut tree = build(
        div()
            .id("root")
            .child(div().id("launch").attr("tabindex", "0")),
        "dialog{width:80px;height:40px}dialog::backdrop{background:transparent}",
    );
    let root = tree.root().unwrap();
    let launch = tree.find_by_id("launch").unwrap();
    tree.set_focused(Some(launch));
    let mut parent = root;
    let mut ids = Vec::new();
    for depth in 0..96 {
        let modal = tree
            .append_child(
                parent,
                div()
                    .tag("dialog")
                    .id(format!("modal-{depth}"))
                    .child(div().tag("button")),
            )
            .unwrap();
        tree.show_modal(modal).unwrap();
        ids.push(modal);
        parent = modal;
    }
    update(&mut tree);
    assert_eq!(tree.top_layer().count(), 96);
    assert_eq!(tree.active_modal(), ids.last().copied());
    assert!(tree.is_inert(launch));
    assert!(tree.focus_next(false));
    assert_eq!(tree.focused(), Some(tree.children(*ids.last().unwrap())[0]));
    tree.close_top_layer(ids[0]);
    update(&mut tree);
    assert_eq!(tree.top_layer().count(), 0);
    assert_eq!(tree.focused(), Some(launch));
    assert!(tree.remove_subtree(ids[0]));
    update(&mut tree);
    assert_eq!(tree.find_by_id("launch"), Some(launch));
    assert!(!tree.remove_subtree(ids[0]));
    assert!(tree.show_modal(ids[0]).is_err());
}
#[test]
fn pseudo_styles_do_not_leak_and_backdrop_layout_is_independent() {
    let mut tree = build(
        div()
            .color("red".parse::<Color>().unwrap())
            .child(div().tag("dialog").id("m")),
        "dialog{width:80px;height:40px;background:blue;color:green}dialog:modal::backdrop{inset:10px;background:currentColor}",
    );
    let m = tree.find_by_id("m").unwrap();
    tree.show_modal(m).unwrap();
    update(&mut tree);
    let scene = paint(&tree);
    assert!(colors(&scene).contains(&color("black")));
    assert_eq!(colors(&scene).last(), Some(&color("blue")));
    assert_eq!(hit(&tree, 15.0, 15.0), Some(HitTarget::Backdrop(m)));
    assert_eq!(hit(&tree, 5.0, 5.0), None);
}
#[test]
fn cached_order_survives_color_animation() {
    use std::time::Duration;
    let mut tree = build(
        div().id("root").child(div().id("a")),
        "#a{width:20px;height:20px;background:red;transition:background-color 1s linear}#a.changed{background:blue}",
    );
    paint(&tree);
    let order = tree.paint_order_rebuilds();
    let a = tree.find_by_id("a").unwrap();
    let now = Instant::now();
    tree.set_classes(a, "changed");
    tree.update_styles(now);
    tree.update_styles(now + Duration::from_millis(500));
    paint(&tree);
    assert_eq!(tree.paint_order_rebuilds(), order);
}
#[test]
fn css_only_tooltip_keeps_layout_and_inherited_hover() {
    let mut tree = build(
        div().id("trigger").child(div().id("tip")),
        "#trigger{position:relative;width:80px;height:40px}#tip{position:absolute;top:100%;left:0;width:100px;height:30px;visibility:hidden;pointer-events:none;background:blue;z-index:5}#trigger:hover>#tip{visibility:visible}",
    );
    let tip = tree.find_by_id("tip").unwrap();
    assert_box(&tree, tip, 0.0, 40.0, 100.0, 30.0);
    assert!(colors(&paint(&tree)).is_empty());
    assert!(tree.pointer_moved(Some(Point::new(20.0, 20.0))));
    let change = tree.update_styles(Instant::now());
    assert!(change.paint && !change.layout);
    assert_eq!(colors(&paint(&tree)), vec![color("blue")]);
    assert_eq!(hit(&tree, 10.0, 50.0), None);
}

#[test]
fn z_index_integer_rounding_and_visibility_follow_css_transition_rules() {
    use std::time::Duration;
    use voidui::style::layer::{Visibility, ZIndex};
    let mut tree = build(
        div().id("a"),
        "#a{position:relative;width:20px;height:20px;z-index:-1;visibility:hidden;transition:z-index 1s linear,visibility 1s linear}#a.on{z-index:0;visibility:visible}",
    );
    let a = tree.root().unwrap();
    let now = Instant::now();
    tree.set_classes(a, "on");
    tree.update_styles(now);
    tree.update_styles(now + Duration::from_millis(500));
    assert_eq!(tree.layer_style(a).z_index, ZIndex::Integer(0));
    assert_eq!(tree.layer_style(a).visibility, Visibility::Visible);
    let _ = (ZIndex::Auto, Visibility::Visible);
}

#[test]
fn opening_modal_during_pointer_dispatch_does_not_erase_autofocus() {
    let mut tree = build(
        div()
            .child(div().tag("button").id("launch").width(100).height(40))
            .child(
                div()
                    .tag("dialog")
                    .id("m")
                    .child(div().tag("button").id("focus").attr("autofocus", "")),
            ),
        "dialog{width:120px;height:80px}",
    );
    tree.pointer_moved(Some(Point::new(5.0, 5.0)));
    let m = tree.find_by_id("m").unwrap();
    let focus = tree.find_by_id("focus").unwrap();
    tree.show_modal(m).unwrap();
    tree.pointer_pressed(true);
    assert_eq!(tree.focused(), Some(focus));
    update(&mut tree);
    tree.pointer_moved(Some(Point::new(5.0, 5.0)));
    tree.pointer_pressed(true);
    assert_eq!(tree.focused(), Some(focus));
}

#[test]
fn changing_containing_blocks_rebuilds_layout_links_without_changing_dom_links() {
    let mut tree = build(
        div()
            .id("root")
            .child(div().id("parent").child(div().id("absolute"))),
        "#root{position:relative;width:200px;height:100px}#parent{margin:20px;width:30px;height:30px}#parent.positioned{position:relative}#absolute{position:absolute;right:0;bottom:0;width:10px;height:10px}",
    );
    let root = tree.root().unwrap();
    let parent = tree.find_by_id("parent").unwrap();
    let absolute = tree.find_by_id("absolute").unwrap();
    assert_box(&tree, absolute, 190.0, 90.0, 10.0, 10.0);
    tree.set_classes(parent, "positioned");
    update(&mut tree);
    let b = tree.bounds(parent);
    assert_box(
        &tree,
        absolute,
        b.origin.x + 20.0,
        b.origin.y + 20.0,
        10.0,
        10.0,
    );
    tree.set_classes(parent, "");
    update(&mut tree);
    assert_box(&tree, absolute, 190.0, 90.0, 10.0, 10.0);
    assert_eq!(tree.parent(absolute), Some(parent));
    assert_eq!(tree.children(root), &[parent]);
}
#[test]
fn geometry_updates_refresh_clips_without_sorting_stacking_contexts_again() {
    let mut tree = build(
        div().id("root").child(div().id("child")),
        "#root{width:40px;height:40px;overflow:hidden}#child{width:100px;height:40px;background:red}#root.wide{width:100px}",
    );
    let root = tree.root().unwrap();
    let child = tree.children(root)[0];
    paint(&tree);
    let count = tree.paint_order_rebuilds();
    assert_eq!(hit(&tree, 70.0, 20.0), None);
    tree.set_classes(root, "wide");
    update(&mut tree);
    assert_eq!(hit(&tree, 70.0, 20.0), Some(HitTarget::Element(child)));
    assert_eq!(tree.paint_order_rebuilds(), count);
}
#[test]
fn disabling_backdrop_hit_testing_does_not_remove_modality() {
    let mut tree = build(
        div().id("root").child(div().id("modal").tag("dialog")),
        "#root{width:400px;height:300px}dialog{width:100px;height:80px}dialog::backdrop{pointer-events:none}",
    );
    let modal = tree.find_by_id("modal").unwrap();
    tree.show_modal(modal).unwrap();
    update(&mut tree);
    assert_eq!(hit(&tree, 5.0, 5.0), None);
    assert!(tree.is_inert(tree.root().unwrap()));
}
#[test]
fn modal_escapes_ancestor_inert_but_explicit_self_inert_remains_effective() {
    let mut tree = build(
        div().attr("inert", "").child(div().tag("dialog").id("m")),
        "dialog{width:100px;height:80px}",
    );
    let m = tree.find_by_id("m").unwrap();
    tree.show_modal(m).unwrap();
    update(&mut tree);
    assert!(!tree.is_inert(m));
    tree.set_attribute(m, "inert", Some(""));
    assert!(tree.is_inert(m));
}
#[test]
fn top_layer_order_is_open_order_not_dom_order_or_numeric_z_order() {
    let mut tree = build(
        div()
            .child(div().tag("dialog").id("first"))
            .child(div().tag("dialog").id("second")),
        "dialog{width:100px;height:80px}#first{z-index:-100}#second{z-index:100}",
    );
    let first = tree.find_by_id("first").unwrap();
    let second = tree.find_by_id("second").unwrap();
    tree.show_modal(second).unwrap();
    tree.show_modal(first).unwrap();
    update(&mut tree);
    assert_eq!(hit(&tree, 160.0, 120.0), Some(HitTarget::Element(first)));
    assert!(!tree.show_modal(second).unwrap());
    assert_eq!(tree.top_layer().collect::<Vec<_>>(), vec![second, first]);
}
#[test]
fn focus_navigation_respects_positive_tabindex_and_reverse_order_in_modal() {
    let mut tree = build(
        div().child(div().tag("button").id("outside")).child(
            div()
                .tag("dialog")
                .id("m")
                .child(div().id("zero").attr("tabindex", "0"))
                .child(div().id("one").attr("tabindex", "1"))
                .child(div().id("max").attr("tabindex", "2147483647")),
        ),
        "dialog{width:100px;height:80px}",
    );
    let m = tree.find_by_id("m").unwrap();
    tree.show_modal(m).unwrap();
    update(&mut tree);
    tree.focus_next(false);
    assert_eq!(tree.focused(), tree.find_by_id("one"));
    tree.focus_next(false);
    assert_eq!(tree.focused(), tree.find_by_id("max"));
    tree.focus_next(false);
    assert_eq!(tree.focused(), tree.find_by_id("zero"));
    tree.focus_next(false);
    assert_eq!(tree.focused(), tree.find_by_id("one"));
    tree.focus_next(true);
    assert_eq!(tree.focused(), tree.find_by_id("zero"));
}

#[test]
fn popover_backdrop_is_pointer_transparent_at_ua_important_origin() {
    let mut tree = build(
        div()
            .id("root")
            .child(div().id("p").attr("popover", "manual")),
        "#root{width:400px;height:300px}[popover]{width:100px;height:40px}:popover-open::backdrop{pointer-events:auto!important}",
    );
    let p = tree.find_by_id("p").unwrap();
    let root = tree.root().unwrap();
    tree.show_popover(p).unwrap();
    update(&mut tree);
    assert_eq!(hit(&tree, 5.0, 5.0), Some(HitTarget::Element(root)));
    assert_eq!(tree.active_modal(), None);
}

#[test]
fn explicit_position_unset_overrides_a_low_level_absolute_preset() {
    let preset = layout::LayoutStyle {
        position: layout::Position::Absolute,
        ..Default::default()
    };
    let tree = build(
        div().width(200).height(100).child(
            div()
                .layout_style(preset)
                .position(voidui::style::CssValue::Unset)
                .width(20)
                .height(10)
                .right(10),
        ),
        "div{background:red}",
    );
    let child = tree.children(tree.root().unwrap())[0];
    assert_eq!(
        tree.layer_style(child).position,
        voidui::style::layer::Position::Static
    );
    assert_box(&tree, child, 0.0, 0.0, 20.0, 10.0);
}

#[test]
fn backdrop_variables_and_viewport_units_follow_the_window() {
    let mut tree = build(
        div().child(div().tag("dialog").id("m")),
        ":root{--ink:red;font-size:10px}dialog{width:80px;height:40px}dialog::backdrop{inset:10vw;background:var(--ink)}",
    );
    let m = tree.find_by_id("m").unwrap();
    tree.show_modal(m).unwrap();
    update(&mut tree);
    assert!(colors(&paint(&tree)).contains(&color("red")));
    assert_eq!(hit(&tree, 45.0, 45.0), Some(HitTarget::Backdrop(m)));
    assert_eq!(hit(&tree, 35.0, 35.0), None);
    tree.layout(
        Size {
            width: AvailableSpace::Definite(600.0),
            height: AvailableSpace::Definite(500.0),
        },
        &cache(),
    );
    assert_eq!(hit(&tree, 45.0, 45.0), None);
    assert_eq!(hit(&tree, 65.0, 65.0), Some(HitTarget::Backdrop(m)));
}
