//! Retained editor layout over bundled real fonts, without a window or GPU.
#![cfg(feature = "editing")]
use std::{borrow::Cow, sync::Arc};
use voidui::{
    core::{
        geometry::Point,
        rich_text::{InlineStyle, StyleSpan, resolve_runs},
    },
    editing::{Bias, EditorLayout, LayoutOptions, Motion, Selection},
    render::{self, ParleyTextSystem, TextAlign, TextSystem},
};

const FONT: &str = "IBM Plex Sans";
const WIDTH: f32 = 400.0;

fn system() -> Arc<TextSystem> {
    let backend = ParleyTextSystem::new_without_system_fonts(FONT);
    backend
        .add_fonts(vec![Cow::Borrowed(include_bytes!(
            "../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
        ))])
        .unwrap();
    Arc::new(TextSystem::new(Arc::new(backend)))
}
fn options() -> LayoutOptions {
    LayoutOptions {
        font: render::font(FONT),
        font_size: 16.0,
        line_height: 24.0,
        width: Some(WIDTH),
    }
}
fn close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.02,
        "expected {expected}, got {actual}"
    );
}
fn move_row(
    layout: &EditorLayout,
    selection: Selection,
    motion: Motion,
    align: TextAlign,
) -> Selection {
    layout.move_selection(selection, motion, false, WIDTH, align, 200.0)
}

#[test]
fn formatting_only_reshapes_its_paragraph_and_plain_transition_reuses_neighbors() {
    let system = system();
    let text = "first paragraph\nsecond paragraph\nthird paragraph";
    let range = text.find("second").unwrap()..text.find("\nthird").unwrap();
    let mut layout = EditorLayout::default();
    layout.prepare(text, options(), system.clone()).unwrap();
    let mut shaped = system.stats().paragraphs_shaped;
    for style in [
        InlineStyle::new().font_size(30.0),
        InlineStyle::new().font_size(36.0),
        InlineStyle::new().font_size(36.0).color(render::black()),
    ] {
        layout
            .prepare_styled(
                text,
                &[StyleSpan::new(range.clone(), style)],
                options(),
                system.clone(),
            )
            .unwrap();
        assert_eq!(system.stats().paragraphs_shaped, shaped + 1);
        shaped += 1;
    }
    layout.prepare(text, options(), system.clone()).unwrap();
    assert_eq!(system.stats().paragraphs_shaped, shaped + 1);
    layout
        .prepare_styled(text, &[], options(), system.clone())
        .unwrap();
    assert_eq!(system.stats().paragraphs_shaped, shaped + 1);
}

#[test]
fn metadata_and_equivalent_run_boundaries_do_not_reshape() {
    let system = system();
    let text = "first\nsecond\nthird";
    let mut layout = EditorLayout::default();
    layout.prepare(text, options(), system.clone()).unwrap();
    let before = system.stats().paragraphs_shaped;
    layout
        .prepare_styled(
            text,
            &[StyleSpan::new(
                6..12,
                InlineStyle::new().metadata("link", "a"),
            )],
            options(),
            system.clone(),
        )
        .unwrap();
    assert_eq!(system.stats().paragraphs_shaped, before);
    let style = InlineStyle::new().font_size(30.0);
    layout
        .prepare_styled(
            text,
            &[StyleSpan::new(6..12, style.clone())],
            options(),
            system.clone(),
        )
        .unwrap();
    let before = system.stats().paragraphs_shaped;
    layout
        .prepare_styled(
            text,
            &[
                StyleSpan::new(6..9, style.clone().metadata("link", "b")),
                StyleSpan::new(9..12, style.metadata("link", "c")),
            ],
            options(),
            system.clone(),
        )
        .unwrap();
    assert_eq!(system.stats().paragraphs_shaped, before);
    // Explicit black is a visual override even when its fallback RGB matches.
    layout
        .prepare_styled(
            text,
            &[StyleSpan::new(
                6..12,
                InlineStyle::new().color(render::black()),
            )],
            options(),
            system.clone(),
        )
        .unwrap();
    assert_eq!(system.stats().paragraphs_shaped, before + 1);
}

#[test]
fn insertion_retains_styled_suffix_at_shifted_byte_offsets() {
    let system = system();
    let mut layout = EditorLayout::default();
    let style = InlineStyle::new().font_size(32.0).line_height(48.0);
    layout
        .prepare_styled(
            "first\nsecond\nthird",
            &[StyleSpan::new(13..18, style.clone())],
            options(),
            system.clone(),
        )
        .unwrap();
    let before = system.stats().paragraphs_shaped;
    layout
        .prepare_styled(
            "first\ninserted\nsecond\nthird",
            &[StyleSpan::new(22..27, style)],
            options(),
            system.clone(),
        )
        .unwrap();
    assert_eq!(system.stats().paragraphs_shaped, before + 1);
    assert_eq!(layout.paragraph_count(), 4);
    let caret = layout
        .caret(22, Bias::After, WIDTH, TextAlign::Left)
        .unwrap();
    close(caret.origin.y, 72.0);
    close(caret.size.height, 48.0);
    assert_eq!(
        layout
            .hit_test(
                Point::new(0.0, caret.origin.y + 1.0),
                WIDTH,
                TextAlign::Left
            )
            .head,
        22
    );
}

#[test]
fn width_changes_rebreak_styled_glyphs_without_shaping() {
    let system = system();
    let text = "small words and LARGE words repeat across several wrapped lines";
    let spans = [StyleSpan::new(
        16..21,
        InlineStyle::new().font_size(32.0).line_height(50.0),
    )];
    let mut layout = EditorLayout::default();
    layout
        .prepare_styled(text, &spans, options(), system.clone())
        .unwrap();
    let before = system.stats().paragraphs_shaped;
    let initial_height = layout.size().height;
    layout.reflow(Some(90.0));
    assert!(layout.size().height > initial_height);
    let mut narrow = options();
    narrow.width = Some(120.0);
    layout
        .prepare_styled(text, &spans, narrow, system.clone())
        .unwrap();
    assert_eq!(system.stats().paragraphs_shaped, before);
}

#[test]
fn invalid_dimensions_and_spans_leave_cached_layout_and_key_unchanged() {
    let system = system();
    let text = "é\nsecond\nthird";
    let mut layout = EditorLayout::default();
    layout.prepare(text, options(), system.clone()).unwrap();
    let before = system.stats().paragraphs_shaped;
    let size = layout.size();
    let caret = layout.caret(text.len(), Bias::Before, WIDTH, TextAlign::Left);
    for value in [f32::NAN, f32::INFINITY, -1.0] {
        let mut invalid = options();
        invalid.width = Some(value);
        assert!(layout.prepare(text, invalid, system.clone()).is_err());
    }
    for (font_size, line_height) in [
        (0.0, 24.0),
        (16.0, 0.0),
        (f32::NAN, 24.0),
        (16.0, f32::INFINITY),
    ] {
        let mut invalid = options();
        invalid.font_size = font_size;
        invalid.line_height = line_height;
        assert!(layout.prepare(text, invalid, system.clone()).is_err());
    }
    let bold = InlineStyle::new().bold();
    let invalid_style = InlineStyle {
        font_size: Some(f32::NAN),
        ..Default::default()
    };
    for spans in [
        vec![StyleSpan::new(1..2, bold.clone())],
        vec![StyleSpan::new(0..text.len() + 1, bold.clone())],
        vec![
            StyleSpan::new(0..4, bold.clone()),
            StyleSpan::new(3..5, bold.clone()),
        ],
        vec![
            StyleSpan::new(3..5, bold.clone()),
            StyleSpan::new(0..2, bold.clone()),
        ],
        // Even a style covering only a hard separator must be validated.
        vec![StyleSpan::new(2..3, invalid_style)],
    ] {
        assert!(
            layout
                .prepare_styled(text, &spans, options(), system.clone())
                .is_err()
        );
        assert_eq!(layout.size(), size);
        assert_eq!(
            layout.caret(text.len(), Bias::Before, WIDTH, TextAlign::Left),
            caret
        );
    }
    layout.prepare(text, options(), system.clone()).unwrap();
    assert_eq!(system.stats().paragraphs_shaped, before);
}

#[test]
fn shaping_failure_with_another_font_system_preserves_the_old_cache() {
    let system = system();
    let empty_fonts = Arc::new(TextSystem::new(Arc::new(
        ParleyTextSystem::new_without_system_fonts(FONT),
    )));
    let text = "first\nsecond\nthird";
    let mut layout = EditorLayout::default();
    layout.prepare(text, options(), system.clone()).unwrap();
    let before = system.stats().paragraphs_shaped;
    let size = layout.size();
    assert!(
        layout
            .prepare_styled("\nchanged\nthird", &[], options(), empty_fonts)
            .is_err()
    );
    assert_eq!(layout.paragraph_count(), 3);
    assert_eq!(layout.size(), size);
    layout.prepare(text, options(), system.clone()).unwrap();
    assert_eq!(system.stats().paragraphs_shaped, before);
}

#[test]
fn hard_separators_keep_utf8_offsets_empty_rows_and_cross_paragraph_motions() {
    let system = system();
    for separator in [
        "\n", "\r", "\r\n", "\u{b}", "\u{c}", "\u{85}", "\u{2028}", "\u{2029}",
    ] {
        let text = format!("a{separator}{separator}é{separator}");
        let second = 1 + separator.len();
        let third = second + separator.len();
        let mut layout = EditorLayout::default();
        layout
            .prepare_styled(
                &text,
                &[StyleSpan::new(
                    third..third + 2,
                    InlineStyle::new().line_height(48.0),
                )],
                options(),
                system.clone(),
            )
            .unwrap();
        assert_eq!(layout.paragraph_count(), 4, "separator {separator:?}");
        close(layout.size().height, 120.0);
        let first = layout.hard_line_at(Point::new(0.0, 1.0), WIDTH, TextAlign::Left);
        assert_eq!(first, 0..second);
        let down = move_row(&layout, Selection::caret(0), Motion::Down, TextAlign::Left);
        assert_eq!(down.head, second);
        let down = move_row(&layout, down, Motion::Down, TextAlign::Left);
        assert_eq!(down.head, third);
        let down = move_row(&layout, down, Motion::Down, TextAlign::Left);
        assert_eq!(down.head, text.len());
        assert_eq!(
            move_row(&layout, down, Motion::Up, TextAlign::Left).head,
            third
        );
        assert_eq!(
            move_row(&layout, Selection::caret(1), Motion::Right, TextAlign::Left).head,
            second
        );
        assert_eq!(
            move_row(
                &layout,
                Selection::caret(third),
                Motion::Left,
                TextAlign::Left
            )
            .head,
            second
        );
    }
}

#[test]
fn mixed_height_paragraphs_use_actual_caret_and_row_advances() {
    let system = system();
    let text = "large\ntiny\nmedium";
    let spans = [
        StyleSpan::new(0..5, InlineStyle::new().font_size(40.0).line_height(80.0)),
        StyleSpan::new(6..10, InlineStyle::new().font_size(8.0).line_height(12.0)),
        StyleSpan::new(11..17, InlineStyle::new().font_size(24.0).line_height(40.0)),
    ];
    let mut layout = EditorLayout::default();
    layout
        .prepare_styled(text, &spans, options(), system.clone())
        .unwrap();
    // The container's 24px strut remains present on the line with a 12px span,
    // matching CSS line-height's minimum for a block's inline formatting context.
    close(layout.size().height, 144.0);
    for (byte, top, height) in [(0, 0.0, 80.0), (6, 80.0, 24.0), (11, 104.0, 40.0)] {
        let caret = layout
            .caret(byte, Bias::After, WIDTH, TextAlign::Left)
            .unwrap();
        close(caret.origin.y, top);
        close(caret.size.height, height);
    }
    let mut selection = Selection::caret(0);
    for expected in [6, 11] {
        selection = move_row(&layout, selection, Motion::Down, TextAlign::Left);
        assert_eq!(selection.head, expected);
        assert_eq!(selection.preferred_x, Some(0.0));
    }
    for expected in [6, 0] {
        selection = move_row(&layout, selection, Motion::Up, TextAlign::Left);
        assert_eq!(selection.head, expected);
    }
    let extended = layout.move_selection(
        Selection::caret(0),
        Motion::Down,
        true,
        WIDTH,
        TextAlign::Left,
        200.0,
    );
    assert_eq!((extended.anchor, extended.head), (0, 6));
}

#[test]
fn wrapped_mixed_size_navigation_visits_every_native_row_in_both_directions() {
    let system = system();
    let text = "small small TALL small small small small small small small small small small small small small small";
    let spans = [StyleSpan::new(
        12..16,
        InlineStyle::new().font_size(50.0).line_height(90.0),
    )];
    let mut options = options();
    options.width = Some(150.0);
    let native = system
        .shape_paragraph(
            text.into(),
            &resolve_runs(text, &spans, 0..text.len(), &options.font),
            options.font_size,
            options.line_height,
            options.width,
            None,
        )
        .unwrap();
    assert!(native.line_count() > 3);
    // Display row bounds remain the geometry contract when the backend adapts
    // native line metrics; raw Parley metrics need not be display coordinates.
    let heights: Vec<_> = (0..native.line_count())
        .map(|row| {
            native
                .row_bounds(row, WIDTH, TextAlign::Left)
                .unwrap()
                .size
                .height
        })
        .collect();
    assert!(heights.contains(&90.0), "missing tall row: {heights:?}");
    assert!(
        heights.contains(&24.0),
        "small rows inherited the tall height: {heights:?}"
    );
    let mut layout = EditorLayout::default();
    layout
        .prepare_styled(text, &spans, options, system.clone())
        .unwrap();
    close(layout.size().height, native.height());
    for align in [TextAlign::Left, TextAlign::Center, TextAlign::Right] {
        let mut selection = Selection::caret(0);
        let first = layout.caret(0, Bias::After, WIDTH, align).unwrap();
        let x = first.origin.x;
        for row in 1..native.line_count() {
            selection = move_row(&layout, selection, Motion::Down, align);
            assert_eq!(selection.preferred_x, Some(x));
            let expected = native.row_bounds(row, WIDTH, align).unwrap();
            let actual = layout
                .caret(selection.head, selection.affinity, WIDTH, align)
                .unwrap();
            close(actual.origin.y, expected.origin.y);
            close(actual.size.height, expected.size.height);
        }
        for row in (0..native.line_count() - 1).rev() {
            selection = move_row(&layout, selection, Motion::Up, align);
            let expected = native.row_bounds(row, WIDTH, align).unwrap();
            let actual = layout
                .caret(selection.head, selection.affinity, WIDTH, align)
                .unwrap();
            close(actual.origin.y, expected.origin.y);
        }
    }
}

#[test]
fn distant_format_changes_retain_the_middle_of_a_thousand_rich_paragraphs() {
    let system = system();
    let mut text = String::new();
    let mut spans = Vec::new();
    for index in 0..1000 {
        if index > 0 {
            text.push('\n');
        }
        let start = text.len();
        text.push_str(&format!("paragraph {index} has styled text"));
        spans.push(StyleSpan::new(
            start..text.len(),
            InlineStyle::new().font_size(18.0),
        ));
    }
    let mut layout = EditorLayout::default();
    layout
        .prepare_styled(&text, &spans, options(), system.clone())
        .unwrap();
    let before = system.stats().paragraphs_shaped;
    for index in [10, 990] {
        spans[index].style = InlineStyle::new().font_size(26.0).line_height(40.0);
    }
    layout
        .prepare_styled(&text, &spans, options(), system.clone())
        .unwrap();
    assert_eq!(system.stats().paragraphs_shaped, before + 2);
    let before = system.stats().paragraphs_shaped;
    layout
        .prepare_styled(&text, &spans, options(), system.clone())
        .unwrap();
    assert_eq!(system.stats().paragraphs_shaped, before);
    // Equal-count text edits also leave the unchanged interior glyphs intact.
    let text = text
        .replacen("paragraph 10 has", "paragraph 10 HAS", 1)
        .replacen("paragraph 990 has", "paragraph 990 HAS", 1);
    layout
        .prepare_styled(&text, &spans, options(), system.clone())
        .unwrap();
    assert_eq!(system.stats().paragraphs_shaped, before + 2);
}

#[test]
fn invalid_font_features_fail_before_any_paragraph_is_shaped() {
    let system = system();
    let text = "first\nsecond\nthird";
    let mut layout = EditorLayout::default();
    layout.prepare(text, options(), system.clone()).unwrap();
    let original_size = layout.size();
    let original_caret = layout.caret(13, Bias::After, WIDTH, TextAlign::Left);
    // Validate the entire request before shaping even its first valid paragraph.
    let mut invalid_font = render::font(FONT);
    invalid_font.features = render::FontFeatures(Arc::new(vec![("bad".into(), 1)]));
    let spans = [
        StyleSpan::new(0..5, InlineStyle::new().font_size(32.0).line_height(48.0)),
        StyleSpan::new(13..18, InlineStyle::new().font(invalid_font)),
    ];
    let before = system.stats().paragraphs_shaped;
    assert!(
        layout
            .prepare_styled(text, &spans, options(), system.clone())
            .is_err()
    );
    assert_eq!(system.stats().paragraphs_shaped, before);
    assert_eq!(layout.size(), original_size);
    assert_eq!(
        layout.caret(13, Bias::After, WIDTH, TextAlign::Left),
        original_caret
    );
    let after_failure = system.stats().paragraphs_shaped;
    layout.prepare(text, options(), system.clone()).unwrap();
    assert_eq!(system.stats().paragraphs_shaped, after_failure);
}

#[test]
fn line_height_only_overrides_drive_wrapped_caret_navigation() {
    let system = system();
    let text = "one two three four five six seven eight nine ten eleven twelve";
    let spans = [StyleSpan::new(8..18, InlineStyle::new().line_height(60.0))];
    let mut options = options();
    options.width = Some(70.0);
    let paragraph = system
        .shape_paragraph(
            text.into(),
            &resolve_runs(text, &spans, 0..text.len(), &options.font),
            options.font_size,
            options.line_height,
            options.width,
            None,
        )
        .unwrap();
    let bounds: Vec<_> = (0..paragraph.line_count())
        .map(|row| paragraph.row_bounds(row, WIDTH, TextAlign::Left).unwrap())
        .collect();
    assert!(
        bounds
            .iter()
            .any(|row| (row.size.height - 60.0).abs() < 0.02)
    );
    assert!(
        bounds
            .iter()
            .any(|row| (row.size.height - 24.0).abs() < 0.02)
    );
    let mut layout = EditorLayout::default();
    layout
        .prepare_styled(text, &spans, options, system.clone())
        .unwrap();
    let before = system.stats().paragraphs_shaped;
    let mut selection = Selection::caret(0);
    for row in &bounds[1..] {
        selection = move_row(&layout, selection, Motion::Down, TextAlign::Left);
        let caret = layout
            .caret(selection.head, selection.affinity, WIDTH, TextAlign::Left)
            .unwrap();
        close(caret.origin.y, row.origin.y);
        close(caret.size.height, row.size.height);
    }
    for row in bounds[..bounds.len() - 1].iter().rev() {
        selection = move_row(&layout, selection, Motion::Up, TextAlign::Left);
        let caret = layout
            .caret(selection.head, selection.affinity, WIDTH, TextAlign::Left)
            .unwrap();
        close(caret.origin.y, row.origin.y);
    }
    assert_eq!(system.stats().paragraphs_shaped, before);
}

#[test]
fn styled_empty_paragraphs_inherit_terminators_without_shaping_glyphs() {
    let system = system();
    let mut layout = EditorLayout::default();
    let mut spans = [
        StyleSpan::new(0..1, InlineStyle::new().font_size(40.0).line_height(64.0)),
        StyleSpan::new(1..2, InlineStyle::new().line_height(36.0)),
    ];
    let before = system.stats().paragraphs_shaped;
    layout
        .prepare_styled("\n\n", &spans, options(), system.clone())
        .unwrap();
    assert_eq!(layout.paragraph_count(), 3);
    close(layout.size().height, 136.0);
    for (byte, top, height) in [(0, 0.0, 64.0), (1, 64.0, 36.0), (2, 100.0, 36.0)] {
        let caret = layout
            .caret(byte, Bias::After, WIDTH, TextAlign::Left)
            .unwrap();
        close(caret.origin.y, top);
        close(caret.size.height, height);
    }
    let down = move_row(&layout, Selection::caret(0), Motion::Down, TextAlign::Left);
    assert_eq!(down.head, 1);
    assert_eq!(
        move_row(&layout, down, Motion::Down, TextAlign::Left).head,
        2
    );
    assert_eq!(
        move_row(&layout, Selection::caret(2), Motion::Up, TextAlign::Left).head,
        1
    );
    spans[1].style = InlineStyle::new().font_size(80.0);
    layout
        .prepare_styled("\n\n", &spans, options(), system.clone())
        .unwrap();
    // A size override inherits the default absolute line height independently.
    close(layout.size().height, 112.0);
    close(
        layout
            .caret(2, Bias::After, WIDTH, TextAlign::Left)
            .unwrap()
            .size
            .height,
        24.0,
    );
    assert_eq!(system.stats().paragraphs_shaped, before);
}

#[test]
fn trailing_empty_paragraph_uses_the_final_utf8_separator_style() {
    let system = system();
    for separator in ["\n", "\r\n", "\u{85}", "\u{2028}", "\u{2029}"] {
        let text = format!("heading{separator}");
        let mut layout = EditorLayout::default();
        layout
            .prepare_styled(
                &text,
                &[
                    StyleSpan::new(0..7, InlineStyle::new().font_size(32.0).line_height(48.0)),
                    StyleSpan::new(7..text.len(), InlineStyle::new().line_height(60.0)),
                ],
                options(),
                system.clone(),
            )
            .unwrap();
        let caret = layout
            .caret(text.len(), Bias::After, WIDTH, TextAlign::Left)
            .unwrap();
        close(caret.origin.y, 48.0);
        close(caret.size.height, 60.0);
        close(layout.size().height, 108.0);
        let before = system.stats().paragraphs_shaped;
        layout
            .prepare_styled(
                &text,
                &[
                    StyleSpan::new(0..7, InlineStyle::new().font_size(32.0).line_height(48.0)),
                    StyleSpan::new(7..text.len(), InlineStyle::new().line_height(72.0)),
                ],
                options(),
                system.clone(),
            )
            .unwrap();
        close(layout.size().height, 120.0);
        assert_eq!(system.stats().paragraphs_shaped, before);
    }
}
