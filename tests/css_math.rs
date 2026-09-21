//! CSS math is tested through parsing, cascade, and real headless layout.
use std::sync::Arc;
use voidui::{
    core::{
        element::IntoElement,
        layout::{self, AvailableSpace, Size},
        widget_tree::WidgetTree,
    },
    div,
    render::{ParleyTextSystem, TextLayoutCache, TextSystem},
    style::css::Stylesheet,
};

fn cache() -> TextLayoutCache {
    let fonts = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    fonts
        .add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!(
            "../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
        ))])
        .unwrap();
    TextLayoutCache::new(Arc::new(TextSystem::new(Arc::new(fonts))))
}

fn resize(tree: &mut WidgetTree, width: f32, height: f32) {
    tree.layout(
        Size {
            width: AvailableSpace::Definite(width),
            height: AvailableSpace::Definite(height),
        },
        &cache(),
    );
}

fn build(view: impl IntoElement, css: &str) -> WidgetTree {
    let mut tree = WidgetTree::new();
    tree.build_root(view);
    tree.set_stylesheets(vec![Stylesheet::parse(css).unwrap()]);
    resize(&mut tree, 1000.0, 600.0);
    tree
}

fn close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.01,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn arithmetic_precedence_nesting_and_comparison_functions() {
    for (value, expected) in [
        ("calc(10px + 3 * 4px)", 22.0),
        ("calc((10px + 2px) / (1 + 2))", 4.0),
        ("calc(100% - 40px)", 960.0),
        ("calc(100% / 2 - 10px * 3)", 470.0),
        ("min(90%, 600px, 800px)", 600.0),
        ("max(10px, 20%, 15px)", 200.0),
        ("clamp(100px, 50%, 400px)", 400.0),
        ("clamp(500px, 50%, 100px)", 500.0),
        ("max(48px, (100% - 720px) / 2)", 140.0),
        ("calc(min(80%, 900px) - max(10px, 2%))", 780.0),
        ("CALC(20PX + 5px)", 25.0),
        ("calc(10px /* comment */ + 5px)", 15.0),
    ] {
        let tree = build(div(), &format!("div {{ width: {value}; }}"));
        close(tree.bounds(tree.root().unwrap()).size.width, expected);
    }
}

#[test]
fn invalid_math_is_rejected_without_relaxing_literal_validation() {
    for value in [
        "calc()",
        "min()",
        "max()",
        "clamp(1px, 2px)",
        "clamp(1px, 2px, 3px, 4px)",
        "min(1px,)",
        "calc(1px +)",
        "calc(1px + 2)",
        "calc(0)",
        "min(0, 1px)",
        "calc(1px+ 2px)",
        "calc(1px +2px)",
        "calc(1px- 2px)",
        "calc(1px * 2px)",
        "calc(1px / 0)",
        "calc(1px / (2 - 2))",
        "calc(1px / 2px)",
        "calc(1px, 2px)",
        "calc(1px) garbage",
        "calc(1e100px)",
        "-1px",
    ] {
        assert!(
            Stylesheet::parse(&format!("div {{ width: {value}; }}")).is_err(),
            "accepted {value}"
        );
    }
    let nested = format!("{}1px{}", "calc(".repeat(100), ")".repeat(100));
    assert!(Stylesheet::parse(&format!("div {{ width: {nested}; }}")).is_err());
}

#[test]
fn centered_content_reflows_without_recascading() {
    let mut tree = build(
        div().child(div().id("content")),
        "
        * { box-sizing: border-box; }
        :root { width: 100%; padding: 32px max(48px, (100% - 720px) / 2); }
        #content { height: 30px; }
    ",
    );
    let root = tree.root().unwrap();
    let content = tree.find_by_id("content").unwrap();
    let passes = tree.cascade_stats().passes;
    for (width, padding, content_width) in [
        (1000.0, 140.0, 720.0),
        (600.0, 48.0, 504.0),
        (1400.0, 340.0, 720.0),
    ] {
        resize(&mut tree, width, 600.0);
        close(tree.bounds(root).size.width, width);
        close(tree.layout_result(root).padding.left, padding);
        close(tree.bounds(content).origin.x, padding);
        close(tree.bounds(content).size.width, content_width);
    }
    assert_eq!(tree.cascade_stats().passes, passes);
}

#[test]
fn clamping_applies_to_final_values_and_margins_remain_signed() {
    let tree = build(
        div().child(div().id("child")),
        "
        :root { width: 200px; }
        #child { width: calc(20% - 100px); height: calc(10px - 50px);
            padding: calc(5% - 30px); margin-left: calc(10% - 50px); }
    ",
    );
    let child = tree.find_by_id("child").unwrap();
    close(tree.bounds(child).size.width, 0.0);
    close(tree.bounds(child).size.height, 0.0);
    close(tree.layout_result(child).padding.left, 0.0);
    close(tree.layout_result(child).margin.left, -30.0);
}

#[test]
fn inheritance_overrides_and_stylesheet_replacement_preserve_ownership() {
    let source = ":root { width: calc(100% - 100px); } #child { width: inherit; }";
    let mut tree = build(div().child(div().id("child")), source);
    let child = tree.find_by_id("child").unwrap();
    close(tree.bounds(child).size.width, 800.0);
    let root = tree.root().unwrap();
    tree.set_stylesheets(vec![Stylesheet::parse("div { width: 80px; }").unwrap()]);
    resize(&mut tree, 1000.0, 600.0);
    close(tree.bounds(child).size.width, 80.0);
    tree.set_stylesheets(vec![Stylesheet::parse(source).unwrap()]);
    resize(&mut tree, 600.0, 600.0);
    close(tree.bounds(root).size.width, 500.0);
    close(tree.bounds(child).size.width, 400.0);

    let tree = build(div().width(90), "div { width: calc(100% - 100px); }");
    close(tree.bounds(tree.root().unwrap()).size.width, 90.0);
    let tree = build(
        div().width(90),
        "div { width: calc(100% - 100px) !important; }",
    );
    close(tree.bounds(tree.root().unwrap()).size.width, 900.0);
    let tree = build(div(), "div { width: calc(100% - 100px); width: initial; }");
    close(tree.bounds(tree.root().unwrap()).size.width, 1000.0);
}

#[test]
fn expressions_work_in_flex_grid_limits_gaps_and_positioned_boxes() {
    for display in ["block", "flex", "grid"] {
        let tree = build(
            div().child(div().id("child")),
            &format!(
                "
            :root {{ display: {display}; width: 800px; height: 400px; }}
            #child {{ width: calc(100% - 40px); max-width: min(600px, 90%);
                height: clamp(20px, 50%, 300px); }}
        "
            ),
        );
        let child = tree.find_by_id("child").unwrap();
        close(tree.bounds(child).size.width, 600.0);
        close(tree.bounds(child).size.height, 200.0);
    }
    let tree = build(
        div().child(div()).child(div().id("second")),
        "
        :root { display: flex; width: 800px; gap: max(10px, 5%); }
        :root > div { flex: 0 0 calc(25% - 10px); }
    ",
    );
    close(
        tree.bounds(tree.find_by_id("second").unwrap()).origin.x,
        230.0,
    );

    let tree = build(
        div(),
        "div { width: min(200px, 50%); height: calc(50% - 10px); left: calc(10% + 5px); position: absolute; }",
    );
    let computed = tree.layout_style(tree.root().unwrap());
    let result = layout::layout_positioned_box(
        computed,
        Size {
            width: 300.0,
            height: 200.0,
        },
    );
    close(result.size.width, 150.0);
    close(result.size.height, 90.0);
    close(result.location.x, 35.0);
}

#[test]
fn constant_functions_work_in_pixel_and_number_properties() {
    let tree = build(
        div(),
        "div { width: calc(10px * max(2, 3)); border-radius: min(8px, 10px); flex-grow: calc(1 + 2); font-size: clamp(12px, 20px, 30px); }",
    );
    let root = tree.root().unwrap();
    close(tree.bounds(root).size.width, 30.0);
    close(tree.layout_style(root).flex_grow, 3.0);
    close(tree.text_style(root).font_size, 20.0);
}

#[test]
fn identical_math_stylesheets_compare_equal() {
    let a = Stylesheet::parse("div { width: calc(100% - 10px); }").unwrap();
    let b = Stylesheet::parse("div { width: calc(100% - 10px); }").unwrap();
    let mut tree = WidgetTree::new();
    assert!(tree.set_stylesheets(vec![a]));
    assert!(!tree.set_stylesheets(vec![b]));
}

#[test]
fn media_leaf_sizes_resolve_math_before_applying_intrinsic_ratio() {
    let picture = voidui::svg_from_str(
        "<svg xmlns='http://www.w3.org/2000/svg' width='200' height='100'></svg>",
    )
    .unwrap();
    let tree = build(
        div().child(picture.id("picture")),
        "
        :root { width: 800px; }
        /* Override the SVG height attribute so intrinsic-ratio sizing applies. */
        #picture { width: calc(50% - 100px); max-width: min(250px, 100%); height: auto; }
    ",
    );
    let picture = tree.find_by_id("picture").unwrap();
    close(tree.bounds(picture).size.width, 250.0);
    close(tree.bounds(picture).size.height, 125.0);
}

#[test]
fn mirrored_scrollbar_gutters_treat_math_and_literal_widths_equally() {
    use voidui::ScrollbarMode;
    let make = |width: &str| {
        build(
            div().child(
                div()
                    .id("pane")
                    .scrollbar_mode(ScrollbarMode::Classic)
                    .child(div().height(900)),
            ),
            &format!(
                ":root {{ width: 1000px; }} #pane {{ box-sizing: border-box; width: {width}; height: 300px; overflow-y: scroll; scrollbar-gutter: stable both-edges; }}"
            ),
        )
    };
    let math = make("calc(100% - 40px)");
    let literal = make("960px");
    let m = math.find_by_id("pane").unwrap();
    let l = literal.find_by_id("pane").unwrap();
    assert_eq!(math.layout_result(m), literal.layout_result(l));
    close(math.bounds(m).size.width, 960.0);
}

#[cfg(feature = "editing")]
#[test]
fn editor_content_is_centered_while_scrollbar_stays_at_the_right_edge() {
    use voidui::{Editor, ScrollbarMode, rich_editor};
    let editor = Editor::new("A line of text\n".repeat(100));
    let mut tree = build(
        div().child(
            rich_editor(&editor)
                .id("editor")
                .scrollbar_mode(ScrollbarMode::Classic),
        ),
        "
        * { box-sizing: border-box; font-family: 'IBM Plex Sans'; }
        :root { width: 100%; height: 100%; }
        #editor { width: 100%; height: 100%; overflow-y: scroll;
            padding: 32px max(48px, (100% - 720px) / 2); }
    ",
    );
    let id = tree.find_by_id("editor").unwrap();
    for width in [1000.0, 600.0, 1400.0] {
        resize(&mut tree, width, 600.0);
        tree.refresh_scroll_content(&cache());
        close(tree.bounds(id).size.width, width);
        close(
            tree.layout_result(id).padding.left,
            ((width - 720.0) / 2.0).max(48.0),
        );
        let metrics = tree.scroll_metrics(id).unwrap();
        assert!(metrics.max.y > 0.0);
        let scrollbar = tree
            .scrollbar_geometry(id, voidui::ScrollAxis::Vertical)
            .unwrap();
        close(scrollbar.track.origin.x + scrollbar.track.size.width, width);
    }
}
