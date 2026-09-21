//! Document-selection semantics over real shaped text; no window or OS clipboard.
use std::{borrow::Cow, sync::Arc, time::Instant};
use voidui::{
    core::{
        element::IntoElement,
        geometry::Point,
        layout::{AvailableSpace, Size},
        selection::{SelectionDirection, SelectionPoint as P},
        widget::WidgetId,
        widget_tree::WidgetTree,
    },
    div,
    render::{ParleyTextSystem, TextLayoutCache, TextSystem},
    style::{css::Stylesheet, selection::UserSelect},
    text,
};
fn cache() -> TextLayoutCache {
    let fonts = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    fonts
        .add_fonts(vec![Cow::Borrowed(include_bytes!(
            "../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
        ))])
        .unwrap();
    TextLayoutCache::new(Arc::new(TextSystem::new(Arc::new(fonts))))
}
fn build(root: impl IntoElement, css: &str) -> WidgetTree {
    let mut tree = WidgetTree::new();
    tree.build_root(root);
    tree.set_stylesheets(vec![
        Stylesheet::parse(&format!(
            "*{{font-family:\"IBM Plex Sans\";font-size:20px;line-height:30px}}{css}"
        ))
        .unwrap(),
    ]);
    tree.layout(
        Size {
            width: AvailableSpace::Definite(500.0),
            height: AvailableSpace::Definite(400.0),
        },
        &cache(),
    );
    tree
}
fn pos(tree: &WidgetTree, id: WidgetId, byte: usize) -> Point<f32> {
    let mut p = tree.text_caret_position(id, byte).unwrap();
    p.y += 10.0;
    p
}
fn id(tree: &WidgetTree, name: &str) -> WidgetId {
    tree.find_by_id(name).unwrap()
}

#[test]
fn user_select_used_values_are_not_normal_property_inheritance() {
    let tree = build(
        div()
            .id("root")
            .child(
                div()
                    .id("none")
                    .child(text("x").id("auto"))
                    .child(text("y").id("override")),
            )
            .child(div().id("contain").child(text("z").id("inside"))),
        "#none{user-select:none}#override{user-select:text}#contain{user-select:contain}",
    );
    assert_eq!(
        tree.selection_style(id(&tree, "auto")).user_select,
        UserSelect::Auto
    );
    assert_eq!(
        tree.selection_style(id(&tree, "auto")).used_user_select,
        UserSelect::None
    );
    assert_eq!(
        tree.selection_style(id(&tree, "override")).used_user_select,
        UserSelect::Text
    );
    assert_eq!(
        tree.selection_style(id(&tree, "inside")).used_user_select,
        UserSelect::Text
    );
}
#[test]
fn forward_backward_drag_and_shift_click_preserve_anchor_and_dom_order() {
    let mut tree = build(
        div()
            .child(text("Hello world").id("a"))
            .child(text("Second line").id("b")),
        "",
    );
    let (a, b) = (id(&tree, "a"), id(&tree, "b"));
    tree.selection_pointer_down(pos(&tree, a, 1), false, 1);
    tree.selection_pointer_move(pos(&tree, b, 6));
    tree.end_selection_drag();
    assert_eq!(tree.selected_text(), "ello world\nSecond");
    assert_eq!(tree.selection_direction(), SelectionDirection::Forward);
    tree.selection_pointer_down(pos(&tree, b, 6), false, 1);
    tree.selection_pointer_move(pos(&tree, a, 1));
    tree.end_selection_drag();
    assert_eq!(tree.selected_text(), "ello world\nSecond");
    assert_eq!(tree.selection_direction(), SelectionDirection::Backward);
    let anchor = tree.selection().unwrap().anchor;
    tree.selection_pointer_down(pos(&tree, a, 0), true, 1);
    assert_eq!(tree.selection().unwrap().anchor, anchor);
}
#[test]
fn none_cannot_start_or_clear_an_existing_selection_and_crossing_excludes_it() {
    let mut tree = build(
        div()
            .child(text("Alpha").id("a"))
            .child(
                text("Hidden from selection")
                    .id("n")
                    .user_select(UserSelect::None),
            )
            .child(text("Omega").id("b")),
        "",
    );
    let (a, n, b) = (id(&tree, "a"), id(&tree, "n"), id(&tree, "b"));
    tree.selection_pointer_down(pos(&tree, a, 0), false, 1);
    tree.selection_pointer_move(pos(&tree, a, 5));
    tree.end_selection_drag();
    let before = tree.selection();
    assert!(!tree.selection_pointer_down(pos(&tree, n, 2), false, 1));
    assert_eq!(tree.selection(), before);
    assert!(!tree.selection_is_dragging());
    tree.selection_pointer_down(pos(&tree, a, 0), false, 1);
    tree.selection_pointer_move(pos(&tree, n, 4));
    assert_eq!(tree.selected_text(), "Alpha");
    tree.selection_pointer_move(pos(&tree, b, 5));
    assert_eq!(tree.selected_text(), "Alpha\nOmega");
    assert_eq!(tree.selected_range(n), None);
}
#[test]
fn all_selects_ancestor_atomically_but_explicit_text_descendants_can_opt_out() {
    let mut tree = build(
        div().child(
            div()
                .id("atomic")
                .user_select(UserSelect::All)
                .child(text("one").id("a"))
                .child(text("two").id("b"))
                .child(
                    text("editable choice")
                        .id("free")
                        .user_select(UserSelect::Text),
                ),
        ),
        "",
    );
    let (a, free) = (id(&tree, "a"), id(&tree, "free"));
    tree.selection_pointer_down(pos(&tree, a, 1), false, 1);
    assert_eq!(tree.selected_text(), "one\ntwo\neditable choice");
    tree.end_selection_drag();
    tree.selection_pointer_down(pos(&tree, free, 1), false, 1);
    tree.selection_pointer_move(pos(&tree, free, 4));
    assert_eq!(tree.selected_text(), "dit");
}
#[test]
fn containment_constrains_inside_and_outside_endpoints_but_can_be_crossed() {
    let mut tree = build(
        div()
            .child(text("before").id("a"))
            .child(
                div()
                    .user_select(UserSelect::Contain)
                    .id("scope")
                    .child(text("contained").id("c")),
            )
            .child(text("after").id("b")),
        "",
    );
    let (a, c, b) = (id(&tree, "a"), id(&tree, "c"), id(&tree, "b"));
    tree.selection_pointer_down(pos(&tree, c, 1), false, 1);
    tree.selection_pointer_move(pos(&tree, b, 3));
    assert_eq!(tree.selected_text(), "ontained");
    tree.end_selection_drag();
    tree.selection_pointer_down(pos(&tree, a, 0), false, 1);
    tree.selection_pointer_move(pos(&tree, c, 3));
    assert_eq!(tree.selected_text(), "before");
    tree.selection_pointer_move(pos(&tree, b, 5));
    assert_eq!(tree.selected_text(), "before\ncontained\nafter");
}
#[test]
fn unicode_words_graphemes_and_crlf_preserve_valid_boundaries() {
    let mut tree = build(text("Hello world\r\nCafe\u{301} 😀👨‍👩‍👧‍👦 中文").id("a"), "");
    let a = id(&tree, "a");
    let mut p = pos(&tree, a, 7);
    p.x += 2.0;
    tree.selection_pointer_down(p, false, 2);
    assert_eq!(tree.selected_text(), "world");
    tree.end_selection_drag();
    tree.selection_pointer_down(pos(&tree, a, 1), false, 3);
    assert_eq!(tree.selected_text(), "Hello world\n");
    assert!(tree.set_selection(P::text(a, 1), P::text(a, 2)).is_ok());
    assert!(tree.set_selection(P::text(a, 21), P::text(a, 22)).is_err());
    tree.select_all();
    assert_eq!(tree.selected_text(), "Hello world\nCafe\u{301} 😀👨‍👩‍👧‍👦 中文");
}
#[test]
fn soft_wraps_are_not_copied_as_newlines_and_alignment_matches_hit_positions() {
    let mut tree = build(
        text("one two three four five six")
            .id("a")
            .width(90)
            .text_align(voidui::style::TextAlignment::Right),
        "",
    );
    let a = id(&tree, "a");
    tree.select_all();
    assert_eq!(tree.selected_text(), "one two three four five six");
    let p = pos(&tree, a, 8);
    tree.selection_pointer_down(p, false, 1);
    assert_eq!(tree.selection().unwrap().anchor, P::text(a, 8));
}
#[test]
fn selected_fragment_updates_live_on_replacement_and_removal() {
    let mut tree = build(
        div()
            .child(text("abcdef").id("a"))
            .child(text("tail").id("b")),
        "",
    );
    let (a, b) = (id(&tree, "a"), id(&tree, "b"));
    tree.set_selection(P::text(a, 2), P::text(a, 5)).unwrap();
    tree.replace_text(a, 0..1, "XYZ").unwrap();
    assert_eq!(tree.selected_text(), "cde");
    tree.replace_text(a, 3..6, "").unwrap();
    assert_eq!(tree.selected_text(), "e");
    tree.set_selection(P::text(a, 0), P::text(b, 2)).unwrap();
    tree.remove_subtree(a);
    assert_eq!(tree.selected_text(), "ta");
}
#[test]
fn select_all_obeys_modality_visibility_and_ua_controls() {
    let mut tree = build(
        div()
            .child(text("page"))
            .child(div().tag("button").child("button"))
            .child(
                div()
                    .tag("dialog")
                    .id("m")
                    .width(200)
                    .height(100)
                    .child(text("modal").id("inside")),
            ),
        "",
    );
    tree.select_all();
    assert_eq!(tree.selected_text(), "page");
    let m = id(&tree, "m");
    tree.show_modal(m).unwrap();
    tree.update_styles(Instant::now());
    tree.layout_computed(
        Size {
            width: AvailableSpace::Definite(500.0),
            height: AvailableSpace::Definite(400.0),
        },
        &cache(),
    );
    tree.select_all();
    assert_eq!(tree.selected_text(), "modal");
}
#[test]
fn programmatic_ranges_can_select_none_but_user_actions_cannot() {
    let mut tree = build(
        text("public UI text").id("a").user_select(UserSelect::None),
        "",
    );
    let a = id(&tree, "a");
    tree.set_selection(P::text(a, 0), P::text(a, 6)).unwrap();
    assert_eq!(tree.selected_text(), "public");
    assert!(!tree.selection_pointer_down(pos(&tree, a, 2), false, 1));
    assert_eq!(tree.selected_text(), "public");
}
#[test]
fn pointer_events_none_is_not_a_selection_prohibition() {
    let mut tree = build(
        text("selectable")
            .id("a")
            .pointer_events(voidui::style::layer::PointerEvents::None),
        "",
    );
    let a = id(&tree, "a");
    tree.selection_pointer_down(pos(&tree, a, 0), false, 1);
    tree.selection_pointer_move(pos(&tree, a, 10));
    assert_eq!(tree.selected_text(), "selectable");
}

#[test]
fn highlight_inheritance_and_paired_defaults_follow_css_not_normal_element_inheritance() {
    use voidui::style::{color::Color, selection::SelectionColors};
    let tree = build(
        div()
            .id("parent")
            .color("red".parse::<Color>().unwrap())
            .child(text("child").id("child")),
        ":root::selection{color:yellow;background:green}#child::selection{color:blue}",
    );
    let child = id(&tree, "child");
    let colors = tree
        .highlight_style(child)
        .colors(tree.text_style(child).color, SelectionColors::default());
    assert_eq!(colors.color, "blue".parse::<Color>().unwrap());
    assert_eq!(colors.background, "green".parse::<Color>().unwrap());
    let tree = build(
        div().child(
            text("child")
                .id("child")
                .color("red".parse::<Color>().unwrap()),
        ),
        ":root::selection{color:initial}",
    );
    let child = id(&tree, "child");
    let colors = tree
        .highlight_style(child)
        .colors(tree.text_style(child).color, SelectionColors::default());
    assert_eq!(colors.color, "black".parse::<Color>().unwrap());
    assert_eq!(colors.background, "transparent".parse::<Color>().unwrap());
}
#[test]
fn universal_selection_rules_override_inherited_highlights_and_currentcolor_stays_local() {
    use voidui::style::{color::Color, selection::SelectionColors};
    let tree = build(
        div().id("parent").child(
            text("child")
                .id("child")
                .color("red".parse::<Color>().unwrap()),
        ),
        "#parent::selection{background:green;color:currentColor}::selection{background:blue}",
    );
    let child = id(&tree, "child");
    let c = tree
        .highlight_style(child)
        .colors(tree.text_style(child).color, SelectionColors::default());
    assert_eq!(c.background, "blue".parse::<Color>().unwrap());
    assert_eq!(c.color, "red".parse::<Color>().unwrap());
}
#[test]
fn selecting_does_not_invalidate_layout_or_rematch_selectors() {
    let mut tree = build(
        text("stable layout and stable selectors").id("a"),
        "::selection{background:yellow;color:black}",
    );
    let a = id(&tree, "a");
    let before = tree.cascade_stats();
    let bounds = tree.bounds(a);
    tree.selection_pointer_down(pos(&tree, a, 0), false, 1);
    for end in 1..=10 {
        tree.selection_pointer_move(pos(&tree, a, end));
    }
    assert_eq!(tree.update_styles(Instant::now()), Default::default());
    assert_eq!(tree.cascade_stats(), before);
    assert_eq!(tree.bounds(a), bounds);
}
#[test]
fn keyboard_extension_uses_graphemes_and_selects_the_block_separator() {
    use voidui::core::selection::SelectionMove as M;
    let mut tree = build(
        div()
            .child(text("a\u{301}b").id("a"))
            .child(text("next").id("b")),
        "",
    );
    let (a, b) = (id(&tree, "a"), id(&tree, "b"));
    tree.set_selection(P::text(a, 0), P::text(a, 0)).unwrap();
    tree.extend_selection(M::Forward);
    assert_eq!(tree.selected_text(), "a\u{301}");
    tree.set_selection(P::text(a, 4), P::text(a, 4)).unwrap();
    tree.extend_selection(M::Forward);
    assert_eq!(tree.selection().unwrap().focus, P::text(b, 0));
    assert_eq!(tree.selected_text(), "\n");
}
#[test]
fn css_webkit_alias_and_invalid_values_are_handled_as_standard_compatibility() {
    let tree = build(
        text("text").id("a"),
        "#a{-webkit-user-select:none;user-select:text;cursor:text}",
    );
    assert_eq!(
        tree.selection_style(id(&tree, "a")).used_user_select,
        UserSelect::Text
    );
    for value in ["user-select:element", "user-select:copy", "cursor:ibeam"] {
        assert!(Stylesheet::parse(&format!("text{{{value}}}")).is_err());
    }
}

#[test]
fn bidi_selection_has_disjoint_visual_rectangles_but_logical_copy_order() {
    let mut tree = build(text("abc אבג xyz").id("a").wrap(false), "");
    let a = id(&tree, "a");
    tree.set_selection(P::text(a, 1), P::text(a, 6)).unwrap();
    assert_eq!(tree.selected_text(), "bc א");
    let rectangles = tree.selection_rectangles(a);
    assert_eq!(rectangles.len(), 2, "{rectangles:?}");
    assert!(rectangles[0].origin.x + rectangles[0].size.width < rectangles[1].origin.x);
    tree.set_selection(P::text(a, 4), P::text(a, 10)).unwrap();
    let rect = tree.selection_rectangles(a)[0];
    // At a bidi boundary the upstream and downstream logical carets can have
    // different visual positions. Enter each edge from inside the Hebrew run.
    let begin = Point::new(rect.origin.x + rect.size.width - 0.1, rect.origin.y + 5.0);
    let end = Point::new(rect.origin.x + 0.1, rect.origin.y + 5.0);
    tree.selection_pointer_down(begin, false, 1);
    tree.selection_pointer_move(end);
    assert_eq!(tree.selected_text(), "אבג");
    tree.selection_pointer_down(end, false, 1);
    tree.selection_pointer_move(begin);
    assert_eq!(tree.selected_text(), "אבג");
    assert_eq!(tree.selection_direction(), SelectionDirection::Backward);
}
#[test]
fn highlight_layout_properties_do_not_change_text_metrics() {
    let tree = build(
        text("same metrics").id("a"),
        "::selection{font-size:99px;padding:40px;width:1px;color:red}",
    );
    let a = id(&tree, "a");
    assert_eq!(tree.text_style(a).font_size, 20.0);
    assert_eq!(tree.layout_result(a).padding.left, 0.0);
}
#[test]
fn removing_highlight_css_restores_paired_defaults_without_reflow() {
    let mut tree = build(text("sample").id("a"), "::selection{color:red}");
    let a = id(&tree, "a");
    tree.set_selection(P::text(a, 0), P::text(a, 6)).unwrap();
    tree.set_stylesheets(vec![
        Stylesheet::parse("*{font-family:'IBM Plex Sans';font-size:20px;line-height:30px}")
            .unwrap(),
    ]);
    let change = tree.update_styles(Instant::now());
    assert!(change.paint && !change.layout);
    assert_eq!(tree.highlight_style(a), Default::default());
    assert_eq!(tree.selected_text(), "sample");
}

#[test]
fn user_selection_start_is_cancelable_and_change_notifications_coalesce() {
    let mut tree = build(text("selection").id("a"), "");
    let a = id(&tree, "a");
    tree.on_select_start(|_| false);
    tree.selection_pointer_down(pos(&tree, a, 0), false, 1);
    let caret = tree.selection();
    assert!(!tree.selection_pointer_move(pos(&tree, a, 5)));
    assert_eq!(tree.selection(), caret);
    tree.set_selection(P::text(a, 0), P::text(a, 2)).unwrap();
    tree.set_selection(P::text(a, 0), P::text(a, 4)).unwrap();
    assert_eq!(tree.take_selection_change(), Some(tree.selection()));
    assert_eq!(tree.take_selection_change(), None);
    tree.clear_selection();
    assert_eq!(tree.take_selection_change(), Some(None));
}

#[test]
fn none_ancestor_padding_keeps_a_drag_inside_its_selectable_descendant() {
    let mut tree = build(
        div()
            .id("none")
            .height(180)
            .padding(20)
            .user_select(UserSelect::None)
            .child(
                text("selectable child")
                    .id("a")
                    .user_select(UserSelect::Text),
            ),
        "",
    );
    let a = id(&tree, "a");
    tree.selection_pointer_down(pos(&tree, a, 2), false, 1);
    tree.selection_pointer_move(Point::new(400.0, 160.0));
    assert_eq!(tree.selected_text(), "lectable child");
    let old = tree.selection();
    assert!(!tree.selection_pointer_down(Point::new(400.0, 160.0), false, 1));
    assert_eq!(tree.selection(), old);
    assert!(!tree.selection_is_dragging());
}
#[test]
fn pointer_carets_never_split_combining_marks_or_emoji_sequences() {
    use unicode_segmentation::UnicodeSegmentation;
    let value = "e\u{301} 👩‍💻 🇨🇳 ffi";
    let mut tree = build(text(value).id("a").wrap(false), "");
    let a = id(&tree, "a");
    let boundaries: Vec<_> = value
        .grapheme_indices(true)
        .map(|(i, _)| i)
        .chain([value.len()])
        .collect();
    let end = pos(&tree, a, value.len());
    for x in 0..=(end.x.ceil() as usize + 2) {
        tree.selection_pointer_down(Point::new(x as f32, 10.0), false, 1);
        let P::Text { byte, .. } = tree.selection().unwrap().focus else {
            panic!("expected a text caret")
        };
        assert!(
            boundaries.contains(&byte),
            "split a grapheme at byte {byte}"
        );
    }
}
#[test]
fn live_child_boundaries_and_highlight_cascade_preserve_document_order() {
    let mut tree = build(
        div().id("root").child(text("first").id("a")),
        "text::selection{color:red!important}.special::selection{color:blue}#a::selection{background:yellow}",
    );
    let root = id(&tree, "root");
    let a = id(&tree, "a");
    tree.set_selection(P::children(root, 0), P::children(root, 1))
        .unwrap();
    tree.append_child(root, text("second")).unwrap();
    tree.layout(
        Size {
            width: AvailableSpace::Definite(500.0),
            height: AvailableSpace::Definite(400.0),
        },
        &cache(),
    );
    assert_eq!(tree.selected_text(), "first");
    assert_eq!(tree.highlight_style(a).color, Some("red".parse().unwrap()));
    tree.remove_subtree(a);
    assert!(tree.selection().unwrap().is_collapsed());
    assert_eq!(tree.selected_text(), "");
}

#[test]
fn ligature_carets_select_a_grapheme_without_reshaping_the_cluster() {
    use voidui::render::{TextRun, font};
    let line = cache()
        .shape_paragraph(
            "ffi".into(),
            &[TextRun {
                font: font("IBM Plex Sans"),
                len: 3,
                ..Default::default()
            }],
            20.0,
            30.0,
            None,
            None,
        )
        .unwrap();
    let glyph_count: usize = line
        .layout()
        .lines()
        .flat_map(|l| l.runs())
        .map(|r| {
            r.visual_clusters()
                .map(|c| c.glyphs().count())
                .sum::<usize>()
        })
        .sum();
    assert!(glyph_count < 3, "fixture must actually shape a ligature");
    let mut tree = build(text("ffi").id("a"), "");
    let a = id(&tree, "a");
    tree.selection_pointer_down(pos(&tree, a, 1), false, 1);
    tree.selection_pointer_move(pos(&tree, a, 2));
    assert_eq!(tree.selected_text(), "f");
    assert_eq!(tree.selection_rectangles(a).len(), 1);
}
#[test]
fn selection_hit_order_follows_stacking_but_copy_order_does_not() {
    let mut tree = build(
        div()
            .id("root")
            .child(text("first").id("a"))
            .child(text("second").id("b")),
        "#root{position:relative;width:300px;height:100px}text{position:absolute;inset:0}#a{z-index:2}#b{z-index:1}",
    );
    let (a, b) = (id(&tree, "a"), id(&tree, "b"));
    tree.selection_pointer_down(pos(&tree, a, 2), false, 1);
    assert_eq!(tree.selection().unwrap().focus, P::text(a, 2));
    tree.set_selection(P::text(a, 0), P::text(b, 6)).unwrap();
    assert_eq!(tree.selected_text(), "first\nsecond");
}
#[test]
fn toggling_selection_css_updates_fragments_without_layout() {
    let mut tree = build(text("content").id("a"), "");
    let a = id(&tree, "a");
    tree.select_all();
    assert_eq!(tree.selected_text(), "content");
    tree.set_stylesheets(vec![
        Stylesheet::parse(
            "*{font-family:'IBM Plex Sans';font-size:20px;line-height:30px;user-select:none}",
        )
        .unwrap(),
    ]);
    let change = tree.update_styles(Instant::now());
    assert!(change.paint && !change.layout);
    assert_eq!(tree.selected_text(), "");
    assert!(tree.selection_rectangles(a).is_empty());
}

#[test]
fn pressing_empty_areas_keeps_the_auto_cursor() {
    use voidui::style::selection::Cursor;

    for root in [
        div().height(180),
        div()
            .height(180)
            .child(text("Nearby text"))
            .child(div().height(120)),
    ] {
        let mut tree = build(root, "");
        let blank = Point::new(10.0, 100.0);
        for clicks in 1..=3 {
            assert_eq!(tree.selection_cursor(blank), Cursor::Auto);
            tree.selection_pointer_down(blank, false, clicks);
            // Empty child boundaries still track selection gestures, but must
            // not turn a press on a non-text area into an I-beam.
            assert!(tree.selection_is_dragging());
            assert_eq!(tree.selection_cursor(blank), Cursor::Auto);
            tree.selection_pointer_move(blank);
            assert_eq!(tree.selection_cursor(blank), Cursor::Auto);
            tree.end_selection_drag();
            assert_eq!(tree.selection_cursor(blank), Cursor::Auto);
        }
    }
}

#[test]
fn pressing_padding_keeps_the_auto_cursor_and_can_still_select_text() {
    use voidui::style::selection::Cursor;

    let mut tree = build(
        div()
            .height(180)
            .padding(20)
            .child(text("Selectable text").id("a")),
        "",
    );
    let a = id(&tree, "a");
    let blank = Point::new(400.0, 160.0);
    assert_eq!(tree.selection_cursor(blank), Cursor::Auto);
    tree.selection_pointer_down(blank, false, 1);
    // Padding maps to the closest caret for selection, not for cursor styling.
    assert_eq!(tree.selection().unwrap().focus, P::text(a, 15));
    assert_eq!(tree.selection_cursor(blank), Cursor::Auto);
    let start = pos(&tree, a, 0);
    tree.selection_pointer_move(start);
    assert_eq!(tree.selected_text(), "Selectable text");
    assert_eq!(tree.selection_cursor(start), Cursor::Auto);
    tree.end_selection_drag();
    assert_eq!(tree.selection_cursor(blank), Cursor::Auto);
}

#[test]
fn text_drag_keeps_its_cursor_until_release_without_affecting_the_next_press() {
    use voidui::style::selection::Cursor;
    use winit::window::CursorIcon;

    let mut tree = build(div().height(180).child(text("Select me").id("a")), "");
    let start = pos(&tree, id(&tree, "a"), 0);
    let blank = Point::new(400.0, 160.0);
    let text_cursor = Cursor::Icon(CursorIcon::Text);
    assert_eq!(tree.selection_cursor(start), text_cursor);
    assert_eq!(tree.selection_cursor(blank), Cursor::Auto);
    tree.selection_pointer_down(start, false, 1);
    assert_eq!(tree.selection_cursor(start), text_cursor);
    tree.selection_pointer_move(blank);
    assert_eq!(tree.selected_text(), "Select me");
    assert_eq!(tree.selection_cursor(blank), text_cursor);
    tree.end_selection_drag();
    assert_eq!(tree.selection_cursor(blank), Cursor::Auto);
    assert_eq!(tree.selection_cursor(start), text_cursor);
    tree.selection_pointer_down(blank, false, 1);
    assert_eq!(tree.selection_cursor(blank), Cursor::Auto);
}

#[test]
fn selection_drag_respects_explicit_cursors_at_the_pointer() {
    use voidui::style::selection::Cursor;
    use winit::window::CursorIcon;

    for cursor in [
        Cursor::None,
        Cursor::Icon(CursorIcon::Default),
        Cursor::Icon(CursorIcon::Pointer),
    ] {
        let mut tree = build(
            div()
                .height(180)
                .child(text("Select me").id("a").cursor(cursor))
                .child(div().id("override").height(40)),
            "#override{cursor:not-allowed}",
        );
        let start = pos(&tree, id(&tree, "a"), 0);
        let blank = Point::new(400.0, 160.0);
        let bounds = tree.bounds(id(&tree, "override"));
        let explicit = Point::new(bounds.origin.x + 1.0, bounds.origin.y + 1.0);
        assert_eq!(tree.selection_cursor(start), cursor);
        tree.selection_pointer_down(start, false, 1);
        tree.selection_pointer_move(blank);
        assert_eq!(tree.selection_cursor(blank), cursor);
        tree.selection_pointer_move(explicit);
        assert_eq!(
            tree.selection_cursor(explicit),
            Cursor::Icon(CursorIcon::NotAllowed)
        );
        tree.end_selection_drag();
        assert_eq!(tree.selection_cursor(blank), Cursor::Auto);
    }
}

#[test]
fn cursor_inherits_and_backdrop_cursor_uses_its_own_css() {
    use voidui::style::selection::Cursor;
    use winit::window::CursorIcon;
    let mut tree = build(
        div()
            .cursor(Cursor::Icon(CursorIcon::Crosshair))
            .child(text("hover").id("a"))
            .child(
                div()
                    .tag("dialog")
                    .id("modal")
                    .width(200)
                    .height(100)
                    .child("modal"),
            ),
        "dialog::backdrop{cursor:not-allowed}",
    );
    let a = id(&tree, "a");
    assert_eq!(
        tree.selection_cursor(pos(&tree, a, 1)),
        Cursor::Icon(CursorIcon::Crosshair)
    );
    tree.show_modal(id(&tree, "modal")).unwrap();
    tree.layout(
        Size {
            width: AvailableSpace::Definite(500.0),
            height: AvailableSpace::Definite(400.0),
        },
        &cache(),
    );
    assert_eq!(
        tree.selection_cursor(Point::new(1.0, 1.0)),
        Cursor::Icon(CursorIcon::NotAllowed)
    );
    assert!(!tree.selection_pointer_down(Point::new(1.0, 1.0), false, 1));
}

#[test]
fn visual_bidi_navigation_keeps_affinity_and_does_not_oscillate() {
    use voidui::core::selection::SelectionMove as M;
    let mut tree = build(text("abc אבג xyz").id("a"), "");
    let a = id(&tree, "a");
    tree.set_selection(P::text(a, 0), P::text(a, 0)).unwrap();
    let mut seen = std::collections::HashSet::new();
    for _ in 0..30 {
        let before = (
            tree.selection().unwrap().focus,
            tree.selection_focus_affinity(),
        );
        tree.extend_selection(M::Right);
        let now = (
            tree.selection().unwrap().focus,
            tree.selection_focus_affinity(),
        );
        if now == before {
            break;
        }
        let P::Text { byte, .. } = now.0 else {
            panic!("expected text focus")
        };
        assert!(
            seen.insert((byte, format!("{:?}", now.1))),
            "cursor oscillated"
        );
    }
    assert_eq!(
        tree.selection().unwrap().focus,
        P::text(a, "abc אבג xyz".len())
    );
    assert_eq!(tree.selected_text(), "abc אבג xyz");
}
#[test]
fn vertical_navigation_preserves_column_across_a_short_line() {
    use voidui::core::selection::SelectionMove as M;
    let mut tree = build(text("abcdefghij\nx\nabcdefghij").id("a"), "");
    let a = id(&tree, "a");
    tree.set_selection(P::text(a, 5), P::text(a, 5)).unwrap();
    tree.extend_selection(M::Down);
    tree.extend_selection(M::Down);
    assert_eq!(tree.selection().unwrap().focus, P::text(a, 18));
    tree.extend_selection(M::Up);
    tree.extend_selection(M::Up);
    assert!(tree.selection().unwrap().is_collapsed());
}
#[test]
fn crlf_visual_navigation_never_exposes_the_middle_of_a_line_break() {
    use voidui::core::selection::SelectionMove as M;
    let mut tree = build(text("a\r\nb").id("a"), "");
    let a = id(&tree, "a");
    tree.set_selection(P::text(a, 0), P::text(a, 0)).unwrap();
    for _ in 0..8 {
        tree.extend_selection(M::Right);
        assert_ne!(tree.selection().unwrap().focus, P::text(a, 2));
    }
    assert_eq!(tree.selected_text(), "a\nb");
}

#[test]
fn end_navigation_excludes_the_crlf_terminator() {
    use voidui::core::selection::SelectionMove as M;
    let mut tree = build(text("first\r\nsecond").id("a"), "");
    let a = id(&tree, "a");
    tree.set_selection(P::text(a, 0), P::text(a, 0)).unwrap();
    tree.extend_selection(M::LineEnd);
    assert_eq!(tree.selected_text(), "first");
    assert_eq!(tree.selection().unwrap().focus, P::text(a, 5));
}

#[test]
fn rich_label_pointer_copy_and_vertical_navigation_share_display_rows() {
    use voidui::core::selection::SelectionMove as M;
    use voidui::{InlineStyle, rich_text, span};
    let content = span("abcdefghij\n")
        .child(span("x\n").style(InlineStyle::new().line_height(90.0)))
        .child(span("abcdefghij").bold());
    let mut tree = build(rich_text(content).id("a"), "");
    let a = id(&tree, "a");
    tree.set_selection(P::text(a, 5), P::text(a, 5)).unwrap();
    tree.extend_selection(M::Down);
    tree.extend_selection(M::Down);
    assert_eq!(tree.selection().unwrap().focus, P::text(a, 18));
    tree.extend_selection(M::Up);
    tree.extend_selection(M::Up);
    assert!(tree.selection().unwrap().is_collapsed());
    let start = pos(&tree, a, 0);
    let end = pos(&tree, a, 23);
    tree.selection_pointer_down(start, false, 1);
    tree.selection_pointer_move(end);
    tree.end_selection_drag();
    assert_eq!(tree.selected_text(), "abcdefghij\nx\nabcdefghij");
    assert!(
        (tree.text_caret_position(a, 13).unwrap().y
            - tree.text_caret_position(a, 0).unwrap().y
            - 120.0)
            .abs()
            < 0.1
    );
}

#[test]
fn transformed_selection_carets_rectangles_and_drag_match_painted_text() {
    let mut tree = build(
        div().size(500, 400).child(text("abcdef").id("text")),
        "#text {transform:translate(100px,60px) scale(1.5);transform-origin:0 0}",
    );
    let text = id(&tree, "text");
    let a = pos(&tree, text, 1);
    let b = pos(&tree, text, 4);
    assert!(a.x > 100. && a.y >= 60.);
    tree.selection_pointer_down(a, false, 1);
    tree.selection_pointer_move(b);
    tree.end_selection_drag();
    assert_eq!(tree.selected_text(), "bcd");
    let rects = tree.selection_rectangles(text);
    assert!(!rects.is_empty());
    assert!(rects[0].origin.x >= 100.);
}
