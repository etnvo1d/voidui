//! Line-container typography must survive projection, editing and virtualization.
#![cfg(feature = "editing")]
use std::{borrow::Cow, sync::Arc};
use voidui::{
    InlineStyle, StyleSpan,
    core::geometry::{Point, Rect},
    editing::*,
    render::{self, ParleyTextSystem, TextAlign, TextSystem},
};

fn system() -> Arc<TextSystem> {
    let backend = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    backend
        .add_fonts(vec![
            Cow::Borrowed(include_bytes!(
                "../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
            )),
            Cow::Borrowed(include_bytes!(
                "../crates/voidui_gpui_wgpu/tests/fonts/Ahem.ttf"
            )),
        ])
        .unwrap();
    Arc::new(TextSystem::new(Arc::new(backend)))
}

fn options() -> LayoutOptions {
    LayoutOptions {
        font: render::font("IBM Plex Sans"),
        font_size: 16.0,
        line_height: 26.0,
        width: Some(240.0),
    }
}

fn typography() -> ParagraphStyle {
    ParagraphStyle {
        font: Some(render::font("Ahem")),
        font_size: Some(14.0),
        line_height: Some(30.0),
        ..Default::default()
    }
}

fn prepare(
    layout: &mut EditorLayout,
    state: &EditorState,
    projection: Projection,
    system: &Arc<TextSystem>,
    opts: LayoutOptions,
) {
    layout
        .prepare_snapshot(
            state.snapshot(),
            projection,
            opts,
            system.clone(),
            None,
            Default::default(),
        )
        .unwrap();
}

fn caret(layout: &EditorLayout, byte: usize) -> Rect<f32> {
    layout
        .caret(byte, Bias::After, 240.0, TextAlign::Left)
        .unwrap()
}

fn close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.0001,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn line_typography_matches_editor_defaults_and_preserves_inline_overrides() {
    let state = EditorState::new("abc def ghi jkl mno pqr");
    let system = system();
    for width in [240.0, 75.0] {
        for inline in [
            InlineStyle::default(),
            InlineStyle::new().font_size(22.0).line_height(35.0),
            InlineStyle::new().font(render::font("IBM Plex Sans")),
        ] {
            let mut actual = EditorLayout::default();
            let mut expected = EditorLayout::default();
            let mut p = Projection::new().paragraph(0..state.document().len(), typography());
            p.styles.push(StyleSpan::new(4..7, inline.clone()));
            prepare(
                &mut actual,
                &state,
                p,
                &system,
                LayoutOptions {
                    width: Some(width),
                    ..options()
                },
            );
            // A line decoration must behave like the same typography on the
            // editor itself, including the line's strut and wrapping decisions.
            let mut reference = Projection::new();
            reference.styles.push(StyleSpan::new(4..7, inline));
            prepare(
                &mut expected,
                &state,
                reference,
                &system,
                LayoutOptions {
                    font: render::font("Ahem"),
                    font_size: 14.0,
                    line_height: 30.0,
                    width: Some(width),
                },
            );
            close(actual.size().height, expected.size().height);
            for byte in 0..=state.document().len() {
                assert_eq!(caret(&actual, byte), caret(&expected, byte));
            }
        }
    }
}

#[test]
fn hidden_empty_and_revealed_lines_share_geometry_through_resize() {
    let state = EditorState::new("before\n```\n\n```\nafter\n");
    let system = system();
    let mut layout = EditorLayout::default();
    for height in [18.0, 26.0, 35.5] {
        let style = ParagraphStyle {
            line_height: Some(height),
            ..typography()
        };
        let mut baseline = None;
        for hidden in [false, true, false, true] {
            let mut p = Projection::new().paragraph(7..16, style.clone());
            if hidden {
                p.replacements.extend([
                    Replacement::hide(ViewId(1), 7..10),
                    Replacement::hide(ViewId(2), 12..15),
                ]);
            }
            prepare(&mut layout, &state, p, &system, options());
            for width in [None, Some(180.0), Some(240.0)] {
                layout.reflow(width);
                for byte in [7, 11, 12] {
                    close(caret(&layout, byte).size.height, height);
                }
                let after = caret(&layout, 16).origin.y;
                close(after, *baseline.get_or_insert(after));
                close(after, 26.0 + 3.0 * height);
            }
        }
    }
}

#[test]
fn changing_line_typography_invalidates_only_its_cached_paragraph() {
    let state = EditorState::new("before\ncode\nafter");
    let system = system();
    let mut layout = EditorLayout::default();
    let styles = [
        typography(),
        ParagraphStyle {
            font_size: Some(20.0),
            ..typography()
        },
        ParagraphStyle {
            line_height: Some(40.0),
            ..typography()
        },
        ParagraphStyle::default(),
    ];
    // Font metrics use a shaped probe once per font/size. Warm those probes so
    // the counter below measures document paragraphs, not font initialization.
    let mut warm = EditorLayout::default();
    for style in &styles {
        prepare(
            &mut warm,
            &state,
            Projection::new().paragraph(7..12, style.clone()),
            &system,
            options(),
        );
    }
    prepare(&mut layout, &state, Projection::new(), &system, options());
    let mut shaped = system.stats().paragraphs_shaped;
    for style in styles {
        let height = style.line_height.unwrap_or(26.0);
        prepare(
            &mut layout,
            &state,
            Projection::new().paragraph(7..12, style),
            &system,
            options(),
        );
        assert_eq!(system.stats().paragraphs_shaped, shaped + 1);
        shaped += 1;
        close(caret(&layout, 12).origin.y, 26.0 + height);
    }
}

#[test]
fn changing_inline_overrides_reflows_a_cached_styled_paragraph() {
    let state = EditorState::new("before\nabc def\nafter");
    let system = system();
    let mut layout = EditorLayout::default();
    for size in [14.0, 40.0, 14.0] {
        let mut projection = Projection::new().paragraph(7..15, typography());
        projection.styles.push(StyleSpan::new(
            11..14,
            InlineStyle::new().font_size(size).line_height(size * 2.0),
        ));
        prepare(&mut layout, &state, projection.clone(), &system, options());
        let mut fresh = EditorLayout::default();
        prepare(&mut fresh, &state, projection, &system, options());
        assert_eq!(caret(&layout, 15), caret(&fresh, 15));
        assert_eq!(caret(&layout, 14), caret(&fresh, 14));
    }
}

#[test]
fn empty_final_line_inherits_typography_after_an_edit_and_option_changes() {
    let mut state = EditorState::new("before\ncode");
    let system = system();
    let mut layout = EditorLayout::default();
    prepare(&mut layout, &state, Projection::new(), &system, options());
    state
        .transact(Transaction::new(
            state.revision(),
            [Edit::new(11..11, "\n")],
        ))
        .unwrap();
    let style = ParagraphStyle {
        line_height: None,
        ..typography()
    };
    let projection = Projection::new()
        .paragraph(0..7, typography())
        .paragraph(12..12, style);
    for height in [26.0, 38.0] {
        prepare(
            &mut layout,
            &state,
            projection.clone(),
            &system,
            LayoutOptions {
                line_height: height,
                ..options()
            },
        );
        close(caret(&layout, 12).size.height, height);
        close(caret(&layout, 7).origin.y, 30.0);
        let rect = caret(&layout, 12);
        assert_eq!(
            layout
                .hit_test(
                    Point::new(0.0, rect.origin.y + height * 0.5),
                    240.0,
                    TextAlign::Left
                )
                .head,
            12
        );
    }
}

#[test]
fn offscreen_height_estimates_use_paragraph_line_height() {
    let state = EditorState::new("x\n".repeat(100));
    let system = system();
    let mut layout = EditorLayout::default();
    let projection = Projection::new()
        .paragraph(0..200, typography())
        .paragraph(200..200, typography());
    layout
        .prepare_snapshot(
            state.snapshot(),
            projection,
            LayoutOptions {
                width: None,
                ..options()
            },
            system,
            Some(Rect::from_xywh(0.0, 0.0, 240.0, 60.0)),
            ViewportOptions {
                overscan: 0.0,
                max_cached_blocks: 4,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(layout.stats().cached_blocks <= 4);
    close(layout.size().height, 101.0 * 30.0);
}

#[test]
fn source_backed_cells_inherit_typography_and_invalidate_on_local_changes() {
    let state = EditorState::new("abc\ndef\nnext");
    let system = system();
    let mut layout = EditorLayout::default();
    layout.configure_views(
        EditorViews::new().block(
            ViewId(1),
            GridBlock {
                rows: vec![vec![0..3], vec![4..7]],
                column_weights: vec![1.0],
                padding: 0.0,
                gap: 0.0,
                rule: None,
            },
        ),
        None,
    );
    for height in [42.0, 46.0, 42.0] {
        let projection = Projection::new()
            .block(ViewId(1), 0..8)
            .paragraph(0..8, typography())
            .paragraph(
                4..8,
                ParagraphStyle {
                    font_size: Some(21.0),
                    line_height: Some(height),
                    ..Default::default()
                },
            );
        prepare(&mut layout, &state, projection, &system, options());
        // Ahem's advance equals its size. Both cells inherit the outer font,
        // while only the second overrides its size and row height.
        close(caret(&layout, 1).origin.x, 14.0);
        close(caret(&layout, 5).origin.x, 21.0);
        close(caret(&layout, 4).size.height, height);
        close(caret(&layout, 8).origin.y, 30.0 + height);
    }
}

#[test]
fn invalid_paragraph_typography_is_rejected_at_projection_boundary() {
    let state = EditorState::new("text");
    for value in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        for style in [
            ParagraphStyle {
                font_size: Some(value),
                ..Default::default()
            },
            ParagraphStyle {
                line_height: Some(value),
                ..Default::default()
            },
        ] {
            assert!(
                Projection::new()
                    .paragraph(0..4, style)
                    .validate(state.document())
                    .is_err()
            );
        }
    }
    let mut font = render::font("Ahem");
    font.weight.0 = f32::NAN;
    assert!(
        Projection::new()
            .paragraph(
                0..4,
                ParagraphStyle {
                    font: Some(font),
                    ..Default::default()
                }
            )
            .validate(state.document())
            .is_err()
    );
}
