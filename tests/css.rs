use std::sync::Arc;
use voidui::{
    core::{
        element::IntoElement,
        layout::{Display, LengthPercentageAuto, Size, TaffyMaxContent},
        widget::WidgetStatus,
        widget_tree::WidgetTree,
    },
    div,
    render::{ParleyTextSystem, TextLayoutCache, TextSystem},
    style::{
        AUTO, CssValue,
        color::{Color, Rgba8},
        css::Stylesheet,
    },
};
fn cache() -> TextLayoutCache {
    TextLayoutCache::new(Arc::new(TextSystem::new(Arc::new(
        ParleyTextSystem::new_without_system_fonts("unused"),
    ))))
}
fn build(root: impl IntoElement, css: &str) -> WidgetTree {
    let mut tree = WidgetTree::new();
    tree.build_root(root);
    tree.set_stylesheets(vec![Stylesheet::parse(css).unwrap()]);
    tree.layout(Size::MAX_CONTENT, &cache());
    tree
}
fn red() -> Color {
    Rgba8::from_rgb8(255, 0, 0).into()
}
fn blue() -> Color {
    Rgba8::from_rgb8(0, 0, 255).into()
}

#[test]
fn type_id_class_combinators_and_attributes_match() {
    let tree = build(
        div()
            .id("root")
            .child(
                div()
                    .class("a card")
                    .attr("data-kind", "primary")
                    .child(div().class("label")),
            )
            .child(div().class("sibling")),
        "div { color: blue; } #root > .card[data-kind^='pri'] .label {color:red;} .card + .sibling {padding: 12px;} .a ~ .sibling {margin-left: 3px;}",
    );
    let children = tree.children(tree.root().unwrap());
    assert_eq!(tree.text_style(tree.children(children[0])[0]).color, red());
    assert_eq!(
        tree.layout_style(children[1]).padding.top,
        voidui::core::layout::length(12)
    );
    assert_eq!(
        tree.layout_style(children[1]).margin.left,
        voidui::core::layout::length(3)
    );
}
#[test]
fn specificity_source_order_inline_and_important() {
    let tree = build(
        div().id("x").class("c").color(blue()),
        "#x {color:red;} .c {color:green !important;} div {color:red !important;} .c {color:blue !important;}",
    );
    assert_eq!(tree.text_style(tree.root().unwrap()).color, blue());
    let tree = build(
        div().id("x").class("c"),
        "#x {color:red;} .c {color:blue;} div {color:green;}",
    );
    assert_eq!(tree.text_style(tree.root().unwrap()).color, red());
    let tree = build(div().color(blue()), "div{color:red;}");
    assert_eq!(tree.text_style(tree.root().unwrap()).color, blue());
}
#[test]
fn explicitly_set_default_values_override_stylesheet() {
    let tree = build(
        div().width(AUTO).padding(0).color(CssValue::Unset),
        "div{width:100px;padding:20px;color:red;}",
    );
    let id = tree.root().unwrap();
    assert!(tree.layout_style(id).size.width.is_auto());
    assert_eq!(
        tree.layout_style(id).padding.top,
        voidui::core::layout::length(0)
    );
    assert_ne!(tree.text_style(id).color, red());
}
#[test]
fn structural_logical_and_relational_selectors() {
    let tree = build(
        div()
            .class("parent")
            .child(div().class("first"))
            .child(div().class("middle"))
            .child(div()),
        ":root {padding:1px;} .parent:has(> .middle) {color:red;} :where(.parent) {color:blue;} div > div:nth-child(2n) {margin-top:5px;} :is(.first, .never):not(.middle) {padding:8px;} div:last-child:empty {height:9px;}",
    );
    let root = tree.root().unwrap();
    let c = tree.children(root);
    assert_eq!(tree.text_style(root).color, red());
    assert_eq!(
        tree.layout_style(c[0]).padding.left,
        voidui::core::layout::length(8)
    );
    assert_eq!(
        tree.layout_style(c[1]).margin.top,
        voidui::core::layout::length(5)
    );
    assert_eq!(tree.bounds(c[2]).size.height, 9.0);
}
#[test]
fn selector_lists_choose_max_matching_specificity() {
    let tree = build(
        div().id("x").class("a"),
        ".a, #x {color:red;} .a {color:blue;}",
    );
    assert_eq!(tree.text_style(tree.root().unwrap()).color, red());
}
#[test]
fn escaped_identifiers_and_attribute_case_flags() {
    let tree = build(
        div().class("a:b").attr("data-kind", "PRIMARY"),
        ".a\\:b[data-kind='primary' i] {color:red;}",
    );
    assert_eq!(tree.text_style(tree.root().unwrap()).color, red());
}
#[test]
fn parses_colors_units_comments_and_shorthands() {
    let tree = build(
        div(),
        "div { color: hsl(120 100% 50%); background-color: rgba(255, 0, 0, .5); margin: 1px 2px 3px 4px; padding: 0 /* comment */ 6px; border: 2px solid #abc; display:flex; flex-direction:column; gap: 4px 8px; }",
    );
    let s = tree.layout_style(tree.root().unwrap());
    assert_eq!(s.display, Display::Flex);
    assert_eq!(s.margin.left, voidui::core::layout::length(4));
    assert_eq!(s.padding.top, voidui::core::layout::length(0));
    assert_eq!(
        tree.text_style(tree.root().unwrap()).color,
        Rgba8::from_rgb8(0, 255, 0).into()
    );
    assert_eq!(
        tree.paint_style(tree.root().unwrap()).background,
        voidui::style::color::Color::new(
            voidui::style::color::ColorSpace::Srgb,
            [1.0, 0.0, 0.0, 0.5]
        )
    );
}
#[test]
fn invalid_css_returns_diagnostic_without_changing_tree() {
    let mut tree = build(div(), "div{color:red;}");
    assert!(Stylesheet::parse("div {color:no-such-color;}").is_err());
    tree.layout(Size::MAX_CONTENT, &cache());
    assert_eq!(tree.text_style(tree.root().unwrap()).color, red());
    let error = Stylesheet::parse("\n\ndiv { width: -4px; }").unwrap_err();
    assert!(error.line >= 3);
    assert!(Stylesheet::parse("@import 'https://example.invalid/test.css';").is_err());
}
#[test]
fn inline_important_css_and_inheritance_keep_origins_separate() {
    let tree = build(
        div().color(blue()).child(div()),
        "div{color:red!important;} div > div {color:inherit!important;padding:inherit;}",
    );
    let root = tree.root().unwrap();
    assert_eq!(tree.text_style(root).color, red());
    assert_eq!(tree.text_style(tree.children(root)[0]).color, red());
    assert_eq!(tree.style(root).color, CssValue::Value(blue()));
}
#[test]
fn no_repeated_matching_on_resize_or_exposure() {
    let mut tree = build(div().class("a").child(div()), ".a {padding:10px;}");
    let before = tree.cascade_stats();
    tree.layout(Size::MAX_CONTENT, &cache());
    assert_eq!(tree.cascade_stats(), before);
    assert!(!tree.set_stylesheets(vec![Stylesheet::parse(".a {padding:10px;}").unwrap()]));
}
#[test]
fn id_class_attribute_and_state_changes_invalidate_relational_matches() {
    let mut tree = build(
        div().child(div()),
        "#x > .a[data-on] {color:red;} :root:has(:focus) {padding:11px;} div:focus-within {margin-top:2px;}",
    );
    let root = tree.root().unwrap();
    let child = tree.children(root)[0];
    tree.set_id(root, Some("x"));
    tree.set_classes(child, "a");
    tree.set_attribute(child, "data-on", Some("yes"));
    tree.set_focused(Some(child));
    tree.layout(Size::MAX_CONTENT, &cache());
    assert_eq!(tree.text_style(child).color, red());
    assert_eq!(
        tree.layout_style(root).padding.top,
        voidui::core::layout::length(11)
    );
    tree.set_focused(None);
    tree.set_attribute(child, "data-on", None);
    tree.layout(Size::MAX_CONTENT, &cache());
    assert_ne!(tree.text_style(child).color, red());
}
#[test]
fn indexed_candidates_do_not_scan_unrelated_rules() {
    let css = (0..1000)
        .map(|i| format!(".c{i}{{padding:1px;}}"))
        .collect::<String>();
    let tree = build(div().class("c333").child(div().class("c111")), &css);
    assert_eq!(tree.cascade_stats().candidate_tests, 2);
}
#[test]
fn replacing_sheet_removes_stale_properties_and_hides_nodes() {
    let mut tree = build(div().child(div().height(10)), "div{padding:12px;}");
    let root = tree.root().unwrap();
    tree.set_stylesheets(vec![Stylesheet::parse(":root{display:none;}").unwrap()]);
    tree.layout(Size::MAX_CONTENT, &cache());
    assert_eq!(tree.bounds(root).size.height, 0.0);
    assert_eq!(
        tree.layout_style(root).padding.top,
        voidui::core::layout::length(0)
    );
    tree.set_stylesheets(vec![]);
    tree.layout(Size::MAX_CONTENT, &cache());
    assert_eq!(tree.bounds(root).size.height, 10.0);
}
#[test]
fn stylesheet_order_only_breaks_specificity_ties() {
    let mut tree = build(div().id("x"), "#x{color:red;}");
    tree.set_stylesheets(vec![
        Stylesheet::parse("#x{color:red;}").unwrap(),
        Stylesheet::parse("div{color:blue;}").unwrap(),
    ]);
    tree.layout(Size::MAX_CONTENT, &cache());
    assert_eq!(tree.text_style(tree.root().unwrap()).color, red());
}
#[test]
fn css_grid_flex_and_percentage_layout() {
    let tree = build(
        div().class("grid").child(div()).child(div()),
        ".grid{display:grid;width:310px;grid-template-columns:1fr 2fr;column-gap:10px;grid-auto-rows:30px;} .grid > div {min-width:0;}",
    );
    let c = tree.children(tree.root().unwrap());
    assert_eq!(tree.bounds(c[0]).size.width, 100.0);
    assert_eq!(tree.bounds(c[1]).size.width, 200.0);
    let _ = LengthPercentageAuto::auto();
    let _ = WidgetStatus::default();
}

#[test]
fn focus_only_styles_do_not_recascade_on_pointer_moves() {
    let mut tree = build(
        div().size(200, 100).child(div().size(50, 30)),
        "div:focus{color:red;}",
    );
    let before = tree.cascade_stats();
    assert!(!tree.pointer_moved(Some(voidui::core::geometry::Point::new(10.0, 10.0))));
    tree.layout(Size::MAX_CONTENT, &cache());
    assert_eq!(tree.cascade_stats(), before);
}
#[test]
fn native_pointer_state_matches_ancestors_and_stops_when_unchanged() {
    let mut tree = build(
        div()
            .size(200, 100)
            .child(div().tag("button").size(50, 30).child(div().size(20, 10))),
        "button:hover {color:red;} button:active {padding:3px;} button:focus {margin-top:2px;}",
    );
    let root = tree.root().unwrap();
    let button = tree.children(root)[0];
    assert!(tree.pointer_moved(Some(voidui::core::geometry::Point::new(5.0, 5.0))));
    tree.layout(Size::MAX_CONTENT, &cache());
    assert_eq!(tree.text_style(button).color, red());
    let before = tree.cascade_stats();
    assert!(!tree.pointer_moved(Some(voidui::core::geometry::Point::new(6.0, 5.0))));
    tree.layout(Size::MAX_CONTENT, &cache());
    assert_eq!(before, tree.cascade_stats());
    assert!(tree.pointer_pressed(true));
    tree.layout(Size::MAX_CONTENT, &cache());
    assert_eq!(
        tree.layout_style(button).padding.top,
        voidui::core::layout::length(3)
    );
    assert!(tree.pointer_pressed(false));
    tree.layout(Size::MAX_CONTENT, &cache());
    assert_eq!(
        tree.layout_style(button).padding.top,
        voidui::core::layout::length(0)
    );
    assert!(tree.pointer_moved(None));
}
#[test]
fn duplicate_classes_do_not_duplicate_index_candidates() {
    let tree = build(div().class("a a").class("a"), ".a{color:red;}");
    assert_eq!(tree.cascade_stats().candidate_tests, 1);
}
#[test]
fn nth_child_of_selector_and_general_sibling_backtracking() {
    let tree = build(
        div()
            .child(div().class("a"))
            .child(div())
            .child(div().class("a")),
        ".a:nth-child(2 of .a) {color:red;} .a ~ div:last-child {padding:2px;}",
    );
    let c = tree.children(tree.root().unwrap());
    assert_eq!(tree.text_style(c[2]).color, red());
    assert_ne!(tree.text_style(c[0]).color, red());
}
#[test]
fn removing_class_and_replacing_root_do_not_leave_cached_matches() {
    let mut tree = build(div().class("a"), ".a{color:red;}");
    let root = tree.root().unwrap();
    tree.set_classes(root, "");
    tree.layout(Size::MAX_CONTENT, &cache());
    assert_ne!(tree.text_style(root).color, red());
    let root = tree.build_root(div().class("a"));
    tree.layout(Size::MAX_CONTENT, &cache());
    assert_eq!(tree.text_style(root).color, red());
}
#[test]
fn important_shorthand_preserves_longhand_cascade_order() {
    let tree = build(
        div().padding_top(99),
        "div{padding-top:8px!important;padding:1px 2px!important;padding-left:3px!important;}",
    );
    let s = tree.layout_style(tree.root().unwrap());
    assert_eq!(s.padding.top, voidui::core::layout::length(1));
    assert_eq!(s.padding.left, voidui::core::layout::length(3));
}
#[test]
fn strings_functions_and_comments_are_not_split_as_declarations() {
    assert!(
        Stylesheet::parse(
            "div { font-family:'a;!b', 'IBM Plex Sans'; background:rgb(0 /*x*/ 0 255 / 50%); } "
        )
        .is_ok()
    );
    assert!(Stylesheet::parse("div{color:red;unknown-property:1;}").is_err());
    assert!(Stylesheet::parse(".a::before{color:red;}").is_err());
    assert!(Stylesheet::parse("div{width:1px 2px;}").is_err());
}
#[test]
fn compiled_rule_index_handles_unindexed_pseudo_subjects() {
    let tree = build(
        div().class("a"),
        ":is(.a,#b){color:red;} :where(.a){color:blue;}",
    );
    assert_eq!(tree.text_style(tree.root().unwrap()).color, red());
}

#[test]
fn file_loading_does_not_imply_watching() {
    let dir = std::env::temp_dir().join(format!(
        "voidui-static-css-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("ui.css");
    std::fs::write(&file, "div{color:red;}").unwrap();
    let mut tree = build(div(), "");
    tree.set_stylesheets(vec![Stylesheet::from_file(&file).unwrap()]);
    tree.layout(Size::MAX_CONTENT, &cache());
    let before = tree.cascade_stats();
    std::fs::write(&file, "div{color:blue;}").unwrap();
    tree.layout(Size::MAX_CONTENT, &cache());
    assert_eq!(tree.text_style(tree.root().unwrap()).color, red());
    assert_eq!(tree.cascade_stats(), before);
    tree.set_stylesheets(vec![Stylesheet::from_file(&file).unwrap()]);
    tree.layout(Size::MAX_CONTENT, &cache());
    assert_eq!(tree.text_style(tree.root().unwrap()).color, blue());
    std::fs::remove_dir_all(dir).unwrap();
}
