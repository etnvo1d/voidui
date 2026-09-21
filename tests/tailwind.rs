//! Utility classes and fluent presets must resolve through the same style engine.
use std::{sync::Arc, time::Instant};
use voidui::{
    IntoElement,
    core::{
        layout::{AvailableSpace, Size},
        widget::WidgetStatus,
        widget_tree::WidgetTree,
    },
    div,
    render::{ParleyTextSystem, TextLayoutCache, TextSystem},
    style::{
        color::{Color, Rgba8},
        css::Stylesheet,
        tailwind,
    },
    text,
};

fn layout(tree: &mut WidgetTree) {
    let cache = TextLayoutCache::new(Arc::new(TextSystem::new(Arc::new(
        ParleyTextSystem::new_without_system_fonts("unused"),
    ))));
    tree.layout(
        Size {
            width: AvailableSpace::Definite(800.0),
            height: AvailableSpace::Definite(600.0),
        },
        &cache,
    );
}

fn build(root: impl IntoElement, sheets: Vec<Stylesheet>) -> WidgetTree {
    let mut tree = WidgetTree::new();
    tree.build_root(root);
    tree.set_stylesheets(sheets);
    layout(&mut tree);
    tree
}

fn css(source: &str) -> Stylesheet {
    Stylesheet::parse(source).unwrap()
}

#[test]
fn requested_chain_sets_independent_borders_and_background() {
    let tree = build(
        div().width(400).child(
            div()
                .box_border()
                .w_full()
                .border_r(3)
                .border_l_2()
                .bg_gray_50(),
        ),
        vec![],
    );
    let child = tree.children(tree.root().unwrap())[0];
    assert_eq!(tree.layout_result(child).size.width, 400.0);
    assert_eq!(
        tree.layout_style(child).border.right,
        taffy::LengthPercentage::length(3.0)
    );
    assert_eq!(
        tree.layout_style(child).border.left,
        taffy::LengthPercentage::length(2.0)
    );
    assert_eq!(
        tree.paint_style(child).background,
        "oklch(98.5% 0.002 247.839)".parse::<Color>().unwrap()
    );
}

#[test]
fn classes_and_chains_have_identical_computed_styles() {
    let classes = "flex flex-col w-full min-w-0 h-20 p-4 px-2 pl-1 gap-3 border-r border-l-2 border-gray-200 bg-gray-50 rounded-lg text-sm font-semibold shadow-md";
    let tree = build(
        div().width(400).child(div().class(classes)).child(
            div()
                .flex_col()
                .w_full()
                .min_w_0()
                .h_20()
                .p_4()
                .px_2()
                .pl_1()
                .gap_3()
                .border_r_1()
                .border_l_2()
                .border_gray_200()
                .bg_gray_50()
                .rounded_lg()
                .text_sm()
                .font_semibold()
                .shadow_md(),
        ),
        vec![tailwind::stylesheet(classes).unwrap()],
    );
    let children = tree.children(tree.root().unwrap());
    assert_eq!(
        tree.layout_style(children[0]),
        tree.layout_style(children[1])
    );
    assert_eq!(tree.paint_style(children[0]), tree.paint_style(children[1]));
    assert_eq!(tree.text_style(children[0]), tree.text_style(children[1]));
    assert_eq!(tree.text_style(children[0]).font_size, 14.0);
    assert!((tree.text_style(children[0]).line_height.resolve(14.0) - 20.0).abs() < 0.001);
    assert_eq!(tree.paint_style(children[0]).border_radius, 8.0);
    assert!(!tree.paint_style(children[0]).box_shadow.is_empty());
}

#[test]
fn later_methods_override_only_their_longhands_in_both_directions() {
    let tree = build(
        div()
            .child(
                div()
                    .p_4()
                    .padding_left(7)
                    .border_4()
                    .border_r(3)
                    .border_l_0(),
            )
            .child(div().padding_left(7).p_4().border_r(3).border_4())
            .child(
                div()
                    .p_0()
                    .m_0()
                    .border_0()
                    .rounded_none()
                    .bg_transparent()
                    .class("defaults"),
            ),
        vec![css(
            ".defaults { padding:20px; margin:10px; border-width:4px; border-radius:8px; background:red; }",
        )],
    );
    let ids = tree.children(tree.root().unwrap());
    let first = tree.layout_style(ids[0]);
    assert_eq!(first.padding.left, taffy::LengthPercentage::length(7.0));
    assert_eq!(first.padding.right, taffy::LengthPercentage::length(16.0));
    assert_eq!(first.border.right, taffy::LengthPercentage::length(3.0));
    assert_eq!(first.border.left, taffy::LengthPercentage::length(0.0));
    let second = tree.layout_style(ids[1]);
    assert_eq!(second.padding.left, taffy::LengthPercentage::length(16.0));
    assert_eq!(second.border.right, taffy::LengthPercentage::length(4.0));
    let third = tree.layout_style(ids[2]);
    assert_eq!(third.padding.left, taffy::LengthPercentage::length(0.0));
    assert_eq!(third.margin.left, taffy::LengthPercentageAuto::length(0.0));
    assert_eq!(third.border.left, taffy::LengthPercentage::length(0.0));
    assert_eq!(tree.paint_style(ids[2]).border_radius, 0.0);
    assert_eq!(
        tree.paint_style(ids[2]).background,
        Rgba8::new(0, 0, 0, 0).into()
    );
}

#[test]
fn theme_and_root_font_changes_update_both_apis() {
    let classes = "p-4 bg-gray-50 rounded-lg";
    let utilities = tailwind::stylesheet(classes).unwrap();
    let mut tree = build(
        div()
            .child(div().p_4().bg_gray_50().rounded_lg())
            .child(div().class(classes)),
        vec![
            utilities.clone(),
            css(
                ":root {font-size:20px; --spacing:0.5rem; --color-gray-50:#123456; --radius-lg:1rem;}",
            ),
        ],
    );
    for &id in tree.children(tree.root().unwrap()) {
        assert_eq!(
            tree.layout_style(id).padding.left,
            taffy::LengthPercentage::length(40.0)
        );
        assert_eq!(
            tree.paint_style(id).background,
            Rgba8::from_hex_rgb(0x123456).into()
        );
        assert_eq!(tree.paint_style(id).border_radius, 20.0);
    }
    tree.set_stylesheets(vec![
        utilities,
        css(":root {font-size:10px; --color-gray-50:#654321;}"),
    ]);
    layout(&mut tree);
    for &id in tree.children(tree.root().unwrap()) {
        assert_eq!(
            tree.layout_style(id).padding.left,
            taffy::LengthPercentage::length(10.0)
        );
        assert_eq!(
            tree.paint_style(id).background,
            Rgba8::from_hex_rgb(0x654321).into()
        );
        assert_eq!(tree.paint_style(id).border_radius, 5.0);
    }
}

#[test]
fn class_order_is_stable_and_inline_and_important_keep_precedence() {
    let a = "p-8 px-4 pl-2";
    let b = "pl-2 px-4 p-8";
    for compilation in [a, b] {
        let tree = build(
            div()
                .child(div().class(a))
                .child(div().class(b))
                .child(div().class(a).pl(7)),
            vec![tailwind::stylesheet(compilation).unwrap()],
        );
        let ids = tree.children(tree.root().unwrap());
        assert_eq!(tree.layout_style(ids[0]), tree.layout_style(ids[1]));
        assert_eq!(
            tree.layout_style(ids[0]).padding.left,
            taffy::LengthPercentage::length(8.0)
        );
        assert_eq!(
            tree.layout_style(ids[2]).padding.left,
            taffy::LengthPercentage::length(7.0)
        );
    }
    let tree = build(
        div().class("p-4!").p_0(),
        vec![tailwind::stylesheet("p-4!").unwrap()],
    );
    assert_eq!(
        tree.layout_style(tree.root().unwrap()).padding.left,
        taffy::LengthPercentage::length(16.0)
    );
}

#[test]
fn states_activate_reset_and_leave_idle_trees_clean() {
    let classes = "bg-white hover:bg-red-500 hover:focus:bg-blue-500";
    let mut tree = build(
        div().class(classes),
        vec![tailwind::stylesheet(classes).unwrap()],
    );
    let root = tree.root().unwrap();
    let background = tree.paint_style(root).background;
    tree.set_status(root, WidgetStatus::Hover);
    tree.update_styles(Instant::now());
    let hover = tree.paint_style(root).background;
    assert_ne!(hover, background);
    tree.set_status(root, WidgetStatus::Hover | WidgetStatus::Focused);
    tree.update_styles(Instant::now());
    assert_ne!(tree.paint_style(root).background, hover);
    tree.set_status(root, WidgetStatus::empty());
    tree.update_styles(Instant::now());
    assert_eq!(tree.paint_style(root).background, background);
    let passes = tree.cascade_stats().passes;
    tree.update_styles(Instant::now());
    assert_eq!(tree.cascade_stats().passes, passes);
}

#[test]
fn fractions_half_steps_and_viewport_sizes_remain_contextual() {
    let mut tree = build(
        div()
            .width(400)
            .child(div().box_border().w_1_2().h_1p5().p_0p5())
            .child(div().box_border().class("w-1/2 h-1.5 p-0.5"))
            .child(div().w_screen().h_screen()),
        vec![tailwind::stylesheet("w-1/2 h-1.5 p-0.5").unwrap()],
    );
    let ids = tree.children(tree.root().unwrap()).to_vec();
    assert_eq!(tree.layout_style(ids[0]), tree.layout_style(ids[1]));
    assert_eq!(tree.layout_result(ids[0]).size.width, 200.0);
    assert_eq!(
        tree.layout_style(ids[0]).size.height,
        taffy::Dimension::length(6.0)
    );
    assert_eq!(
        tree.layout_style(ids[0]).padding.left,
        taffy::LengthPercentage::length(2.0)
    );
    assert_eq!(tree.layout_result(ids[2]).size.width, 800.0);
    tree.set_viewport_size(Size {
        width: 1024.0,
        height: 768.0,
    });
    layout(&mut tree);
    assert_eq!(tree.layout_result(ids[2]).size.width, 1024.0);
}

#[test]
fn every_base_class_compiles_and_unsupported_inputs_fail() {
    for name in tailwind::classes() {
        tailwind::stylesheet(name).unwrap_or_else(|error| panic!("{name}: {error}"));
    }
    assert_eq!(
        tailwind::all().unwrap().rule_count(),
        tailwind::classes().len()
    );
    assert_eq!(tailwind::stylesheet("").unwrap().rule_count(), 0);
    assert_eq!(tailwind::stylesheet("p-4 p-4").unwrap().rule_count(), 1);
    for invalid in [
        "unknown",
        "md:p-4",
        "dark:bg-black",
        "w-[23px]",
        "p-4;bad",
        "bg-gray-51",
        "hover:",
    ] {
        let error = tailwind::stylesheet(invalid).unwrap_err().to_string();
        assert!(error.contains("unsupported"), "{invalid}: {error}");
    }
}

#[test]
fn presets_are_available_on_text_and_grid_places_children() {
    let _ = text("Title")
        .w_full()
        .p_2()
        .bg_gray_50()
        .text_xl()
        .font_bold();
    let tree = build(
        div()
            .grid()
            .grid_cols_2()
            .gap_2()
            .w(200)
            .child(div().h_4())
            .child(div().h_4()),
        vec![],
    );
    let ids = tree.children(tree.root().unwrap());
    assert_eq!(tree.layout_result(ids[0]).size.width, 96.0);
    assert_eq!(tree.layout_result(ids[1]).location.x, 104.0);
}

#[test]
#[should_panic(expected = "nonnegative")]
fn short_setters_preserve_length_validation() {
    let _ = div().border_r(-1);
}

#[test]
fn self_auto_clears_the_item_override_without_accepting_auto_for_items() {
    let tree = build(
        div().flex_row().items_center().child(div().self_auto()),
        vec![],
    );
    let child = tree.children(tree.root().unwrap())[0];
    assert!(tree.layout_style(child).align_self.is_none());
    assert!(Stylesheet::parse("div {align-self:auto;justify-self:auto}").is_ok());
    assert!(Stylesheet::parse("div {align-items:auto}").is_err());
}

#[test]
fn every_standard_cursor_has_matching_class_method_and_native_icon() {
    use std::collections::BTreeSet;
    use voidui::{Element, core::geometry::Point, style::selection::Cursor};
    use winit::window::CursorIcon as I;

    // This reference table is independent of the catalog: a missing keyword or
    // incorrect native mapping must fail even if all generated methods compile.
    macro_rules! cursors {
        ($( $method:ident, $keyword:literal, $expected:expr; )*) => {
            [$( (concat!("cursor-", $keyword), $expected,
                (|| div().$method().size(100, 100)
                    .child(div().size(100, 100)).into_element()) as fn() -> Element), )*]
        };
    }
    let cases = cursors! {
        cursor_auto, "auto", Cursor::Auto;
        cursor_default, "default", Cursor::Icon(I::Default);
        cursor_pointer, "pointer", Cursor::Icon(I::Pointer);
        cursor_wait, "wait", Cursor::Icon(I::Wait);
        cursor_text, "text", Cursor::Icon(I::Text);
        cursor_move, "move", Cursor::Icon(I::Move);
        cursor_help, "help", Cursor::Icon(I::Help);
        cursor_not_allowed, "not-allowed", Cursor::Icon(I::NotAllowed);
        cursor_none, "none", Cursor::None;
        cursor_context_menu, "context-menu", Cursor::Icon(I::ContextMenu);
        cursor_progress, "progress", Cursor::Icon(I::Progress);
        cursor_cell, "cell", Cursor::Icon(I::Cell);
        cursor_crosshair, "crosshair", Cursor::Icon(I::Crosshair);
        cursor_vertical_text, "vertical-text", Cursor::Icon(I::VerticalText);
        cursor_alias, "alias", Cursor::Icon(I::Alias);
        cursor_copy, "copy", Cursor::Icon(I::Copy);
        cursor_no_drop, "no-drop", Cursor::Icon(I::NoDrop);
        cursor_grab, "grab", Cursor::Icon(I::Grab);
        cursor_grabbing, "grabbing", Cursor::Icon(I::Grabbing);
        cursor_all_scroll, "all-scroll", Cursor::Icon(I::AllScroll);
        cursor_col_resize, "col-resize", Cursor::Icon(I::ColResize);
        cursor_row_resize, "row-resize", Cursor::Icon(I::RowResize);
        cursor_n_resize, "n-resize", Cursor::Icon(I::NResize);
        cursor_e_resize, "e-resize", Cursor::Icon(I::EResize);
        cursor_s_resize, "s-resize", Cursor::Icon(I::SResize);
        cursor_w_resize, "w-resize", Cursor::Icon(I::WResize);
        cursor_ne_resize, "ne-resize", Cursor::Icon(I::NeResize);
        cursor_nw_resize, "nw-resize", Cursor::Icon(I::NwResize);
        cursor_se_resize, "se-resize", Cursor::Icon(I::SeResize);
        cursor_sw_resize, "sw-resize", Cursor::Icon(I::SwResize);
        cursor_ew_resize, "ew-resize", Cursor::Icon(I::EwResize);
        cursor_ns_resize, "ns-resize", Cursor::Icon(I::NsResize);
        cursor_nesw_resize, "nesw-resize", Cursor::Icon(I::NeswResize);
        cursor_nwse_resize, "nwse-resize", Cursor::Icon(I::NwseResize);
        cursor_zoom_in, "zoom-in", Cursor::Icon(I::ZoomIn);
        cursor_zoom_out, "zoom-out", Cursor::Icon(I::ZoomOut);
    };
    assert_eq!(cases.len(), 36);
    let expected_classes: BTreeSet<_> = cases.iter().map(|(class, _, _)| *class).collect();
    let actual_classes: BTreeSet<_> = tailwind::classes()
        .filter(|name| name.starts_with("cursor-"))
        .collect();
    assert_eq!(actual_classes, expected_classes);
    for (class, expected, fluent) in cases {
        for tree in [
            build(fluent(), vec![]),
            build(
                div()
                    .class(class)
                    .size(100, 100)
                    .child(div().size(100, 100)),
                vec![tailwind::stylesheet(class).unwrap()],
            ),
        ] {
            let root = tree.root().unwrap();
            assert_eq!(tree.selection_style(root).cursor, expected, "{class}");
            let child = tree.children(root)[0];
            assert_eq!(
                tree.selection_style(child).cursor,
                expected,
                "inherited {class}"
            );
            assert_eq!(
                tree.selection_cursor(Point::new(10.0, 10.0)),
                expected,
                "hit-tested {class}"
            );
        }
    }
}

#[test]
fn cursor_state_changes_and_inline_overrides_preserve_cascade() {
    use voidui::style::selection::Cursor;
    use winit::window::CursorIcon as I;

    let classes = "cursor-default hover:cursor-col-resize";
    let mut tree = build(
        div().class(classes),
        vec![tailwind::stylesheet(classes).unwrap()],
    );
    let root = tree.root().unwrap();
    assert_eq!(tree.selection_style(root).cursor, Cursor::Icon(I::Default));
    tree.set_status(root, WidgetStatus::Hover);
    tree.update_styles(Instant::now());
    assert_eq!(
        tree.selection_style(root).cursor,
        Cursor::Icon(I::ColResize)
    );
    tree.set_status(root, WidgetStatus::empty());
    tree.update_styles(Instant::now());
    assert_eq!(tree.selection_style(root).cursor, Cursor::Icon(I::Default));

    let tree = build(
        div()
            .class("cursor-col-resize")
            .cursor_col_resize()
            .cursor_row_resize()
            .child(div().cursor_none()),
        vec![tailwind::stylesheet("cursor-col-resize").unwrap()],
    );
    let root = tree.root().unwrap();
    assert_eq!(
        tree.selection_style(root).cursor,
        Cursor::Icon(I::RowResize)
    );
    assert_eq!(
        tree.selection_style(tree.children(root)[0]).cursor,
        Cursor::None
    );

    let tree = build(
        div().cursor_col_resize().class("cursor-none!"),
        vec![tailwind::stylesheet("cursor-none!").unwrap()],
    );
    assert_eq!(
        tree.selection_style(tree.root().unwrap()).cursor,
        Cursor::None
    );
    let _ = text("Divider").cursor_col_resize();
}
