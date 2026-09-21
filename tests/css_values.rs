//! Contextual CSS values must preserve cascade semantics across updates.
use std::{sync::Arc, time::Instant};
use voidui::{
    core::{
        element::IntoElement,
        layout::{AvailableSpace, Dimension, Size},
        widget_tree::WidgetTree,
    },
    div,
    render::{ParleyTextSystem, TextLayoutCache, TextSystem},
    style::{color::Rgba8, css::Stylesheet, pct},
};
fn cache() -> TextLayoutCache {
    TextLayoutCache::new(Arc::new(TextSystem::new(Arc::new(
        ParleyTextSystem::new_without_system_fonts("unused"),
    ))))
}
fn space(w: f32, h: f32) -> Size<AvailableSpace> {
    Size {
        width: AvailableSpace::Definite(w),
        height: AvailableSpace::Definite(h),
    }
}
fn build(root: impl IntoElement, css: &str) -> WidgetTree {
    let mut tree = WidgetTree::new();
    tree.build_root(root);
    tree.set_stylesheets(vec![Stylesheet::parse(css).unwrap()]);
    tree.layout(space(800.0, 600.0), &cache());
    tree
}
fn width(tree: &WidgetTree, id: voidui::core::widget::WidgetId) -> f32 {
    tree.layout_result(id).size.width
}
#[test]
fn percent_api_uses_css_numbers_without_changing_css_percentages() {
    let tree = build(
        div()
            .width(400)
            .child(div().width(Dimension::percent(50.0)))
            .child(div().width(pct(25.0)))
            .child(div().class("css")),
        ".css { width: 50%; }",
    );
    let children = tree.children(tree.root().unwrap());
    assert_eq!(width(&tree, children[0]), 200.0);
    assert_eq!(width(&tree, children[1]), 100.0);
    assert_eq!(width(&tree, children[2]), 200.0);
    assert_eq!(
        Dimension::percent(125.0).into_taffy(),
        taffy::Dimension::percent(1.25)
    );
}
#[test]
fn variables_inherit_resolved_tokens_and_preserve_case() {
    let tree = build(
        div().child(div().class("child")),
        ":root { --A: 10px; --a: 20px; --alias: var(--A); }\n.child { --A: 30px; width: var(--alias); height: var(--a); }",
    );
    let id = tree.children(tree.root().unwrap())[0];
    assert_eq!(width(&tree, id), 10.0);
    assert_eq!(tree.layout_result(id).size.height, 20.0);
}
#[test]
fn variable_fallbacks_cycles_and_invalid_values_reset_winners() {
    let tree = build(
        div().child(div()),
        ":root { color: red; --a:var(--b); --b:var(--a, 5px); --valid:30px; --cycle:var(--valid, var(--cycle)); }\n div > div { width:77px; width:var(--a, var(--missing, 23px)); height:var(--cycle, 19px); color: blue; color:var(--missing); padding: 12px; padding: var(--missing); }",
    );
    let id = tree.children(tree.root().unwrap())[0];
    assert_eq!(width(&tree, id), 23.0);
    assert_eq!(tree.layout_result(id).size.height, 19.0);
    assert_eq!(
        tree.text_style(id).color,
        Rgba8::from_rgb8(255, 0, 0).into()
    );
    assert_eq!(
        tree.layout_style(id).padding.left,
        taffy::LengthPercentage::length(0.0)
    );
}
#[test]
fn substitution_does_not_join_tokens_or_use_fallback_for_wrong_types() {
    let tree = build(
        div(),
        "div { --n: 12; --color:red; width: 10px; width:var(--n)px; height:20px; height:var(--color,30px); }",
    );
    let id = tree.root().unwrap();
    assert!(tree.layout_style(id).size.width.is_auto());
    assert!(tree.layout_style(id).size.height.is_auto());
}
#[test]
fn shorthand_longhands_inline_and_important_keep_their_precedence() {
    let tree = build(
        div().padding_left(7).child(div().class("child")),
        "div { --space: 2em; font-size:10px; padding:var(--space); padding-top:3px; } .child { padding-left:1rem !important; }",
    );
    let root = tree.root().unwrap();
    let child = tree.children(root)[0];
    assert_eq!(
        tree.layout_style(root).padding.left,
        taffy::LengthPercentage::length(7.0)
    );
    assert_eq!(
        tree.layout_style(root).padding.top,
        taffy::LengthPercentage::length(3.0)
    );
    assert_eq!(
        tree.layout_style(root).padding.right,
        taffy::LengthPercentage::length(20.0)
    );
    assert_eq!(
        tree.layout_style(child).padding.left,
        taffy::LengthPercentage::length(10.0)
    );
}
#[test]
fn font_size_precedes_em_and_root_rem_uses_initial_font() {
    let tree = build(
        div().child(div()),
        ":root { font-size:2rem; width:3rem; } div > div { width:2em; font-size:0.5em; height:1rem; line-height:2em; }",
    );
    let root = tree.root().unwrap();
    let child = tree.children(root)[0];
    assert_eq!(tree.text_style(root).font_size, 32.0);
    assert_eq!(width(&tree, root), 96.0);
    assert_eq!(tree.text_style(child).font_size, 16.0);
    assert_eq!(width(&tree, child), 32.0);
    assert_eq!(tree.layout_result(child).size.height, 32.0);
    assert_eq!(tree.text_style(child).line_height.resolve(16.0), 32.0);
}
#[test]
fn viewport_units_and_mixed_math_recompute_without_rematching() {
    let mut tree = build(
        div().class("root").child(div()),
        ".root { --gap: 2rem; font-size:10px; width:50vw; } .root > div { width:calc(100% - var(--gap)); height:10vh; }",
    );
    let root = tree.root().unwrap();
    let child = tree.children(root)[0];
    assert_eq!(width(&tree, root), 400.0);
    assert_eq!(width(&tree, child), 380.0);
    assert_eq!(tree.layout_result(child).size.height, 60.0);
    let stats = tree.cascade_stats();
    tree.layout(space(1000.0, 800.0), &cache());
    assert_eq!(width(&tree, root), 500.0);
    assert_eq!(width(&tree, child), 480.0);
    assert_eq!(tree.layout_result(child).size.height, 80.0);
    assert_eq!(tree.cascade_stats(), stats);
    tree.layout(space(1000.0, 800.0), &cache());
    assert_eq!(tree.cascade_stats(), stats);
}
#[test]
fn classes_and_stylesheet_replacement_recompute_inherited_values() {
    let mut tree = build(
        div().child(div()),
        ":root { --size:2em; font-size:10px; } .large { font-size:20px; } div > div { width:var(--size); }",
    );
    let root = tree.root().unwrap();
    let child = tree.children(root)[0];
    assert_eq!(width(&tree, child), 20.0);
    tree.set_classes(root, "large");
    tree.layout(space(800.0, 600.0), &cache());
    assert_eq!(width(&tree, child), 40.0);
    tree.set_stylesheets(vec![
        Stylesheet::parse("div > div { width:var(--size, 7px); }").unwrap(),
    ]);
    tree.layout(space(800.0, 600.0), &cache());
    assert_eq!(width(&tree, child), 7.0);
}
#[test]
fn custom_property_wide_keywords_and_empty_fallbacks() {
    let tree = build(
        div().child(div()),
        ":root { --x:11px; --y:13px; --z:17px; } div > div { --x:initial; --y:inherit; --z:unset; width:var(--x, 19px); height:var(--y); margin:var(--z); padding:var(--empty,) 4px; }",
    );
    let id = tree.children(tree.root().unwrap())[0];
    assert_eq!(width(&tree, id), 27.0); // Content width plus horizontal padding.
    assert_eq!(
        tree.layout_style(id).margin.left,
        taffy::LengthPercentageAuto::length(17.0)
    );
    assert_eq!(
        tree.layout_style(id).padding.left,
        taffy::LengthPercentage::length(4.0)
    );
}
#[test]
fn relative_units_work_in_grid_paint_and_transforms() {
    let tree = build(
        div().child(div()).child(div()),
        "div { font-size:10px; } :root { display:grid; grid-template-columns:2em 3rem; column-gap:1vw; border-radius:1em; box-shadow:1em 2em 3px red; transform:translateX(1rem); }",
    );
    let root = tree.root().unwrap();
    let children = tree.children(root);
    assert_eq!(width(&tree, children[0]), 20.0);
    assert_eq!(width(&tree, children[1]), 30.0);
    assert_eq!(tree.paint_style(root).border_radius, 10.0);
}
#[test]
fn invalid_syntax_is_rejected_but_missing_variables_are_computed_time_errors() {
    for css in [
        "div{width:var(x);}",
        "div{width:var(--x 2px);}",
        "div{unknown:var(--x);}",
        "div{width:-2em;}",
        "div{width:calc(1em + 2);}",
    ] {
        assert!(Stylesheet::parse(css).is_err(), "accepted {css}");
    }
    assert!(Stylesheet::parse("div{width:var(--missing);}").is_ok());
}
#[test]
fn explicit_viewport_is_available_before_layout() {
    let mut tree = WidgetTree::new();
    tree.build_root(div());
    tree.set_stylesheets(vec![
        Stylesheet::parse("div{font-size:2vw;width:10vh;}").unwrap(),
    ]);
    tree.set_viewport_size(Size {
        width: 1000.0,
        height: 500.0,
    });
    tree.update_styles(Instant::now());
    assert_eq!(tree.text_style(tree.root().unwrap()).font_size, 20.0);
    tree.layout_computed(space(1000.0, 500.0), &cache());
    assert_eq!(width(&tree, tree.root().unwrap()), 50.0);
}

#[test]
fn contextual_font_math_is_not_rejected_using_a_sample_font_size() {
    let tree = build(
        div().child(div()),
        ":root{font-size:40px} div > div{font-size:calc(1em - 20px);width:1em;line-height:calc(2em - 30px)}",
    );
    let child = tree.children(tree.root().unwrap())[0];
    assert_eq!(tree.text_style(child).font_size, 20.0);
    assert_eq!(width(&tree, child), 20.0);
    assert_eq!(tree.text_style(child).line_height.resolve(20.0), 10.0);
}
#[test]
fn selection_resolves_originating_variables_and_updates_on_class_changes() {
    let mut tree = build(
        div().child(div()),
        ":root{--ink:red} .changed{--ink:blue} div > div::selection{color:var(--ink);background:var(--missing,green)}",
    );
    let root = tree.root().unwrap();
    let child = tree.children(root)[0];
    assert_eq!(
        tree.highlight_style(child).color,
        Some(Rgba8::from_rgb8(255, 0, 0).into())
    );
    tree.set_classes(root, "changed");
    tree.layout(space(800.0, 600.0), &cache());
    assert_eq!(
        tree.highlight_style(child).color,
        Some(Rgba8::from_rgb8(0, 0, 255).into())
    );
}
#[test]
fn strings_escaped_names_and_important_custom_properties() {
    let tree = build(
        div().child(div()),
        r#":root{--x:11px!important;--x:12px;--\73 ize:17px;--face:"var(--x) 1em"} div > div{width:var(--x);height:var(--size);font-family:var(--face)}"#,
    );
    let child = tree.children(tree.root().unwrap())[0];
    assert_eq!(width(&tree, child), 11.0);
    assert_eq!(tree.layout_result(child).size.height, 17.0);
    assert_eq!(tree.text_style(child).font.family.as_str(), "var(--x) 1em");
}
#[test]
fn late_declarations_overwrite_deferred_values_and_vars_can_select_display() {
    let tree = build(
        div(),
        "div{--display:flex;display:var(--display);width:var(--missing);width:21px;padding:var(--missing);padding:4px}",
    );
    let root = tree.root().unwrap();
    assert_eq!(tree.layout_style(root).display, taffy::Display::Flex);
    assert_eq!(width(&tree, root), 29.0);
}
#[test]
fn long_variable_expansion_is_bounded() {
    let mut css = String::from("div{--x0:1px;");
    for index in 1..24 {
        css.push_str(&format!(
            "--x{index}:var(--x{}) var(--x{});",
            index - 1,
            index - 1
        ));
    }
    css.push_str("width:var(--x23, 9px)}");
    let tree = build(div(), &css);
    assert_eq!(width(&tree, tree.root().unwrap()), 9.0);
}
#[test]
fn percentage_padding_helper_uses_css_numbers() {
    let tree = build(
        div().width(200).child(div().width(50).padding(pct(10.0))),
        "",
    );
    let child = tree.children(tree.root().unwrap())[0];
    assert_eq!(tree.layout_result(child).padding.left, 20.0);
    assert_eq!(width(&tree, child), 90.0);
}

#[test]
fn explicit_viewport_is_independent_of_layout_constraints() {
    let mut tree = WidgetTree::new();
    tree.build_root(div());
    tree.set_stylesheets(vec![
        Stylesheet::parse("div{width:50vw;height:10vh}").unwrap(),
    ]);
    tree.set_viewport_size(Size {
        width: 1000.0,
        height: 600.0,
    });
    tree.layout(space(200.0, 100.0), &cache());
    assert_eq!(width(&tree, tree.root().unwrap()), 500.0);
    assert_eq!(tree.layout_result(tree.root().unwrap()).size.height, 60.0);
}

#[test]
fn shared_stylesheets_keep_window_contexts_separate() {
    let sheet = Stylesheet::parse(":root{font-size:2vw}div{width:3rem}").unwrap();
    for (viewport, expected) in [(500.0, 30.0), (1000.0, 60.0)] {
        let mut tree = WidgetTree::new();
        tree.build_root(div());
        tree.set_stylesheets(vec![sheet.clone()]);
        tree.layout(space(viewport, 600.0), &cache());
        assert_eq!(width(&tree, tree.root().unwrap()), expected);
    }
}

#[test]
fn color_variables_preserve_functions_and_ignore_surrounding_css_comments() {
    let expected = "oklch(63.7% 0.237 25.331)"
        .parse::<voidui::style::color::Color>()
        .unwrap();
    for value in [
        "var(--missing, oklch(63.7% 0.237 25.331))",
        "/* leading */ oklch(63.7% 0.237 25.331) /* trailing */",
        "var(--accent)",
    ] {
        let tree = build(
            div(),
            &format!(
                ":root {{ --accent: oklch(63.7% 0.237 25.331); background-color:{value}; color:{value}; border-color:{value}; }}"
            ),
        );
        let root = tree.root().unwrap();
        assert_eq!(tree.paint_style(root).background, expected, "{value}");
        assert_eq!(tree.paint_style(root).border_color, expected, "{value}");
        assert_eq!(tree.text_style(root).color, expected, "{value}");
    }
    // Trivia may separate tokens, but must never splice an identifier together.
    assert!(Stylesheet::parse("div {color:r/**/ed}").is_err());
    let tree = build(div(), ":root {--fragment:r; color:var(--fragment)ed;}");
    assert_eq!(
        tree.text_style(tree.root().unwrap()).color,
        Rgba8::from_rgb8(0, 0, 0).into()
    );
}
