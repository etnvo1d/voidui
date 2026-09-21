//! Rich runs retain native typography, source offsets and editor geometry.
use parley::{Affinity, Cursor};
use std::{borrow::Cow, sync::Arc};
use voidui_gpui_wgpu::*;

fn system() -> TextSystem {
    let backend = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    backend
        .add_fonts(vec![Cow::Borrowed(include_bytes!(
            "fonts/IBMPlexSans-Regular.ttf"
        ))])
        .unwrap();
    TextSystem::new(Arc::new(backend))
}

fn run(len: usize, font_size: Option<f32>, line_height: Option<f32>) -> TextRun {
    TextRun {
        len,
        font: font("IBM Plex Sans"),
        font_size,
        line_height,
        ..Default::default()
    }
}

fn close(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.001, "{actual} != {expected}");
}

/// Native layout supplies x and cluster identity; Paragraph owns display y and height.
fn check_geometry(p: &Paragraph, width: f32) {
    for (row, line) in p.layout().lines().take(p.line_count()).enumerate() {
        let byte = p.source_index(line.text_range().start);
        for align in [TextAlign::Left, TextAlign::Center, TextAlign::Right] {
            let bounds = p.row_bounds(row, width, align).unwrap();
            let caret = p
                .caret_bounds(byte, Affinity::Downstream, width, align)
                .unwrap();
            let cursor =
                Cursor::from_byte_index(p.layout(), line.text_range().start, Affinity::Downstream);
            assert_eq!(p.cursor_row(cursor), row);
            let native = cursor.geometry(p.layout(), 1.0);
            close(caret.origin.x, native.x0 as f32 + bounds.origin.x);
            close(caret.origin.y, bounds.origin.y);
            close(caret.size.height, bounds.size.height);
            close(caret.size.width, 1.0);
            assert_eq!(
                p.caret_position(byte, Affinity::Downstream, width, align),
                Some(caret.origin)
            );
            let hit = p
                .cursor_at(
                    point(caret.origin.x, caret.origin.y + caret.size.height * 0.5),
                    width,
                    align,
                )
                .unwrap();
            assert_eq!(p.source_index(hit.index()), byte, "row {row}, {align:?}");
            if !line.text_range().is_empty() {
                assert_eq!(
                    p.character_at(
                        point(caret.origin.x, caret.origin.y + caret.size.height * 0.5),
                        width,
                        align
                    ),
                    Some(byte)
                );
            }
        }
    }
    assert!(
        p.row_bounds(p.line_count(), width, TextAlign::Left)
            .is_none()
    );
}

#[test]
fn variable_heights_drive_carets_hits_selections_and_clamps() {
    let s = system();
    let text = "small\nLARGE\ntail\n";
    let runs = [
        run(6, None, None),
        run(6, Some(32.0), Some(48.0)),
        run(5, Some(12.0), Some(20.0)),
    ];
    for clamp in [None, Some(2), Some(0)] {
        let p = s
            .shape_paragraph(text.into(), &runs, 16.0, 24.0, None, clamp)
            .unwrap();
        check_geometry(&p, 200.0);
        if clamp == Some(0) {
            assert_eq!(p.height(), 0.0);
            assert!(
                p.caret_bounds(0, Affinity::Downstream, 200.0, TextAlign::Left)
                    .is_none()
            );
            continue;
        }
        let heights: Vec<_> = (0..p.line_count())
            .map(|row| {
                p.row_bounds(row, 200.0, TextAlign::Left)
                    .unwrap()
                    .size
                    .height
            })
            .collect();
        for (actual, expected) in heights.iter().zip([24.0, 48.0, 20.0, 20.0]) {
            close(*actual, expected);
        }
        close(p.height(), if clamp.is_some() { 72.0 } else { 112.0 });
        for (row, rect) in p.selection_rectangles(0..text.len(), 200.0, TextAlign::Left) {
            let line = p.row_bounds(row, 200.0, TextAlign::Left).unwrap();
            close(rect.origin.y, line.origin.y);
            close(rect.size.height, line.size.height);
        }
        let last = p
            .cursor_at(point(0.0, 10000.0), 200.0, TextAlign::Left)
            .unwrap();
        let caret = p
            .caret_bounds(
                p.source_index(last.index()),
                last.affinity(),
                200.0,
                TextAlign::Left,
            )
            .unwrap();
        close(
            caret.origin.y,
            p.row_bounds(p.line_count() - 1, 200.0, TextAlign::Left)
                .unwrap()
                .origin
                .y,
        );
        if clamp.is_some() {
            assert!(
                p.caret_bounds(12, Affinity::Downstream, 200.0, TextAlign::Left)
                    .is_none()
            );
        }
    }
}

#[test]
fn mixed_font_sizes_and_heights_survive_reflow_without_reshaping() {
    let s = system();
    let runs = [
        run(4, None, None),
        run(8, Some(32.0), Some(48.0)),
        run(9, Some(12.0), Some(20.0)),
    ];
    let mut p = s
        .shape_paragraph(
            "one BIG WORD two more".into(),
            &runs,
            16.0,
            24.0,
            None,
            None,
        )
        .unwrap();
    let sizes: Vec<_> = p
        .layout()
        .lines()
        .flat_map(|line| line.runs())
        .map(|run| run.font_size())
        .collect();
    assert!(sizes.contains(&16.0) && sizes.contains(&32.0) && sizes.contains(&12.0));
    close(p.height(), 48.0);
    let stats = s.stats();
    for width in [80.0, 150.0, 500.0, 80.0] {
        p.reflow(Some(width));
        check_geometry(&p, width);
        assert_eq!(s.stats(), stats);
        if width == 80.0 {
            let heights: Vec<_> = (0..p.line_count())
                .map(|row| {
                    p.row_bounds(row, width, TextAlign::Left)
                        .unwrap()
                        .size
                        .height
                })
                .collect();
            assert!(heights.contains(&48.0) && heights.iter().any(|h| *h < 48.0));
        }
    }
}

#[test]
fn font_size_inherits_independently_from_absolute_line_height() {
    let s = system();
    let p = s
        .shape_paragraph(
            "a\nb".into(),
            &[run(2, Some(32.0), None), run(1, None, Some(48.0))],
            16.0,
            24.0,
            None,
            None,
        )
        .unwrap();
    close(
        p.row_bounds(1, 200.0, TextAlign::Left).unwrap().origin.y,
        24.0,
    );
    close(
        p.row_bounds(1, 200.0, TextAlign::Left).unwrap().size.height,
        48.0,
    );
    close(
        p.layout()
            .get(1)
            .unwrap()
            .runs()
            .next()
            .unwrap()
            .font_size(),
        16.0,
    );
    close(p.height(), 72.0);
    // Tight leading expands caret/selection geometry, not the line advance.
    let caret = p
        .caret_bounds(0, Affinity::Downstream, 200.0, TextAlign::Left)
        .unwrap();
    assert!(caret.origin.y < 0.0 && caret.size.height > 24.0);
    let native = *p.layout().get(0).unwrap().metrics();
    let ink_height = native.ascent + native.descent;
    close(caret.origin.y, (24.0 - ink_height) * 0.5);
    close(caret.size.height, ink_height);
    close(
        p.first_baseline().unwrap(),
        native.ascent + (24.0 - ink_height) * 0.5,
    );
}

#[test]
fn soft_wrap_affinity_uses_the_correct_row_geometry_and_alignment() {
    let s = system();
    let p = s
        .shape_paragraph(
            "one BIG WORD two more".into(),
            &[run(4, None, None), run(17, Some(30.0), Some(44.0))],
            16.0,
            24.0,
            Some(80.0),
            None,
        )
        .unwrap();
    let byte = p.layout().get(1).unwrap().text_range().start;
    for (affinity, row) in [(Affinity::Upstream, 0), (Affinity::Downstream, 1)] {
        let native = Cursor::from_byte_index(p.layout(), byte, affinity).geometry(p.layout(), 1.0);
        let caret = p
            .caret_bounds(byte, affinity, 200.0, TextAlign::Right)
            .unwrap();
        let bounds = p.row_bounds(row, 200.0, TextAlign::Right).unwrap();
        close(caret.origin.x, native.x0 as f32 + bounds.origin.x);
        close(caret.origin.y, bounds.origin.y);
        close(caret.size.height, bounds.size.height);
    }
}

#[test]
fn crlf_split_runs_preserve_surviving_styles_and_source_positions() {
    let s = system();
    // The removed CR and an explicitly empty run must not style the next byte.
    let mut lf = run(1, Some(24.0), Some(40.0));
    lf.color = white();
    lf.color_is_explicit = true;
    let runs = [
        run(1, None, None),
        run(1, Some(200.0), Some(300.0)),
        run(0, Some(300.0), Some(400.0)),
        lf.clone(),
        run(2, Some(12.0), Some(20.0)),
        run(1, None, None),
    ];
    let normalized = [
        run(1, None, None),
        lf,
        run(2, Some(12.0), Some(20.0)),
        run(1, None, None),
    ];
    let p = s
        .shape_paragraph("a\r\nb\rc".into(), &runs, 16.0, 24.0, None, None)
        .unwrap();
    let expected = s
        .shape_paragraph("a\nb\nc".into(), &normalized, 16.0, 24.0, None, None)
        .unwrap();
    assert_eq!(p.layout_text(), expected.layout_text());
    assert_eq!(p.layout().styles(), expected.layout().styles());
    assert_eq!(p.height(), expected.height());
    for (a, b) in p.layout().lines().zip(expected.layout().lines()) {
        assert_eq!(a.metrics(), b.metrics());
    }
    check_geometry(&p, 200.0);
    for source in [0, 1, 3, 4, 5, 6] {
        assert_eq!(p.source_index(p.layout_index(source)), source);
    }
    assert_eq!(
        p.caret_bounds(1, Affinity::Downstream, 200.0, TextAlign::Left),
        p.caret_bounds(2, Affinity::Downstream, 200.0, TextAlign::Left)
    );
}

#[test]
fn invalid_run_dimensions_are_rejected_before_shaping_even_when_hidden() {
    let s = system();
    for value in [0.0, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for invalid in [run(1, Some(value), None), run(1, None, Some(value))] {
            for clamp in [None, Some(0)] {
                let error = s
                    .shape_paragraph("x".into(), &[invalid.clone()], 16.0, 24.0, None, clamp)
                    .err()
                    .unwrap();
                assert_eq!(error.to_string(), "invalid text run dimensions");
            }
        }
    }
    assert!(
        s.shape_paragraph(
            "".into(),
            &[run(0, Some(f32::NAN), None)],
            16.0,
            24.0,
            None,
            None
        )
        .is_err()
    );
    assert_eq!(s.stats().paragraphs_shaped, 0);
    let p = s
        .shape_paragraph("é".into(), &[run(2, None, None)], 16.0, 24.0, None, None)
        .unwrap();
    for byte in [1, 3, usize::MAX] {
        assert!(
            p.caret_bounds(byte, Affinity::Downstream, 200.0, TextAlign::Left)
                .is_none()
        );
    }
}

#[test]
fn explicit_foregrounds_opt_out_of_paragraph_overrides() {
    let inherited = TextBrush::from(&TextRun {
        color: black(),
        ..Default::default()
    });
    let explicit = TextBrush::from(&TextRun {
        color: black(),
        color_is_explicit: true,
        ..Default::default()
    });
    assert!(!TextBrush::default().color_is_explicit);
    assert_eq!(inherited.foreground(Some(white())), white());
    assert_eq!(explicit.foreground(Some(white())), black());
    assert_eq!(inherited.foreground(None), black());
    assert_eq!(explicit.foreground(None), black());
}

#[test]
fn cursor_rows_follow_native_geometry_at_bidi_boundaries_and_empty_rows() {
    let s = system();
    // Missing Hebrew glyphs still retain Unicode bidi analysis with the bundled font.
    let text = "abc אבג def\nאבג abc\n";
    let p = s
        .shape_paragraph(
            text.into(),
            &[run(text.len(), None, None)],
            16.0,
            24.0,
            Some(90.0),
            None,
        )
        .unwrap();
    for index in text
        .char_indices()
        .map(|(index, _)| index)
        .chain([text.len()])
    {
        for affinity in [Affinity::Upstream, Affinity::Downstream] {
            let cursor = Cursor::from_byte_index(p.layout(), index, affinity);
            let row = p.cursor_row(cursor);
            let native = cursor.geometry(p.layout(), 1.0);
            let bounds = p.row_bounds(row, 200.0, TextAlign::Right).unwrap();
            let caret = p
                .caret_bounds(index, affinity, 200.0, TextAlign::Right)
                .unwrap();
            close(bounds.origin.y, native.y0 as f32);
            close(bounds.size.height, (native.y1 - native.y0) as f32);
            close(caret.origin.x, native.x0 as f32 + bounds.origin.x);
        }
    }
    let last = Cursor::from_byte_index(p.layout(), text.len(), Affinity::Upstream);
    assert_eq!(p.cursor_row(last), p.line_count() - 1);
    let empty = s
        .shape_paragraph("".into(), &[], 16.0, 24.0, None, None)
        .unwrap();
    assert_eq!(
        empty.cursor_row(Cursor::from_byte_index(
            empty.layout(),
            0,
            Affinity::Downstream
        )),
        0
    );
}

#[test]
fn line_height_only_styles_have_distinct_display_heights() {
    let s = system();
    let p = s
        .shape_paragraph(
            "a\nb\nc".into(),
            &[
                run(2, None, Some(24.0)),
                run(2, None, Some(48.0)),
                run(1, None, Some(20.0)),
            ],
            16.0,
            24.0,
            None,
            None,
        )
        .unwrap();
    for (row, (top, height)) in [(0.0, 24.0), (24.0, 48.0), (72.0, 20.0)]
        .into_iter()
        .enumerate()
    {
        let bounds = p.row_bounds(row, 200.0, TextAlign::Left).unwrap();
        let metrics = *p.layout().get(row).unwrap().metrics();
        let overflow = ((metrics.ascent + metrics.descent - height) * 0.5).max(0.0);
        close(bounds.origin.y, top - overflow);
        close(bounds.size.height, height + overflow * 2.0);
    }
    close(p.height(), 92.0);
    check_geometry(&p, 200.0);
}

#[test]
fn tight_mixed_heights_keep_hits_and_selections_in_the_intended_rows() {
    let s = system();
    let text = "a\nb\nc\n";
    let runs = [
        run(2, Some(48.0), Some(2.0)),
        run(2, Some(64.0), Some(3.0)),
        run(2, Some(32.0), Some(4.0)),
    ];
    for clamp in [None, Some(2)] {
        let mut p = s
            .shape_paragraph(text.into(), &runs, 16.0, 24.0, None, clamp)
            .unwrap();
        let stats = s.stats();
        for width in [None, Some(100.0), Some(20.0)] {
            p.reflow(width);
            assert_eq!(s.stats(), stats);
            let mut native_bottom = f32::NEG_INFINITY;
            for line in p.layout().lines() {
                assert!(
                    line.metrics().block_min_coord >= native_bottom,
                    "native overlap"
                );
                native_bottom = line.metrics().block_max_coord;
            }
            let mut top = 0.0;
            let mut count = 0;
            for (row, height) in [2.0, 3.0, 4.0, 4.0]
                .into_iter()
                .take(p.line_count())
                .enumerate()
            {
                let byte = row * 2;
                let line = p.layout().get(row).unwrap();
                let native = line.metrics();
                let expected_baseline =
                    top + native.ascent + (height - native.ascent - native.descent) * 0.5;
                let caret = p
                    .caret_bounds(byte, Affinity::Downstream, 200.0, TextAlign::Left)
                    .unwrap();
                close(caret.origin.y, expected_baseline - native.ascent);
                close(caret.size.height, native.ascent + native.descent);
                if row == 0 {
                    close(p.first_baseline().unwrap(), expected_baseline);
                }
                if row + 1 == p.line_count() {
                    close(p.last_baseline().unwrap(), expected_baseline);
                }
                for align in [TextAlign::Left, TextAlign::Center, TextAlign::Right] {
                    let bounds = p.row_bounds(row, 200.0, align).unwrap();
                    let point = point(bounds.origin.x, top + height * 0.5);
                    let cursor = p.cursor_at(point, 200.0, align).unwrap();
                    assert_eq!(p.cursor_row(cursor), row);
                    assert_eq!(p.source_index(cursor.index()), byte);
                    if byte < text.len() {
                        assert_eq!(p.character_at(point, 200.0, align), Some(byte));
                        assert_eq!(
                            p.selection_unit(point, 200.0, align, false),
                            Some(byte..byte + 2)
                        );
                        assert_eq!(
                            p.selection_unit(point, 200.0, align, true),
                            Some(byte..byte + 1)
                        );
                    }
                }
                if byte < text.len() {
                    let rects = p.selection_rectangles(byte..byte + 1, 200.0, TextAlign::Left);
                    assert_eq!(rects.len(), 1);
                    assert_eq!(rects[0].0, row);
                    close(rects[0].1.origin.y, caret.origin.y);
                    close(rects[0].1.size.height, caret.size.height);
                    count += 1;
                }
                top += height;
            }
            close(p.height(), top);
            assert_eq!(
                p.selection_rectangles(0..text.len(), 200.0, TextAlign::Left)
                    .len(),
                count
            );
            let cursor = p
                .cursor_at(point(0.0, f32::MAX), 200.0, TextAlign::Left)
                .unwrap();
            assert_eq!(p.cursor_row(cursor), p.line_count() - 1);
        }
    }
}

#[test]
fn glyphless_separators_and_trailing_rows_use_their_source_styles() {
    let s = system();
    for separator in ["\n", "\r\n", "\r", "\u{2028}", "\u{2029}"] {
        let text = format!("a{separator}{separator}");
        let p = s
            .shape_paragraph(
                text.into(),
                &[
                    run(1, None, Some(24.0)),
                    run(separator.len(), None, Some(48.0)),
                    run(separator.len(), None, Some(32.0)),
                ],
                16.0,
                24.0,
                None,
                None,
            )
            .unwrap();
        assert_eq!(p.line_count(), 3, "{separator:?}");
        let mut top = 0.0;
        for (row, height) in [48.0, 32.0, 32.0].into_iter().enumerate() {
            let bounds = p.row_bounds(row, 200.0, TextAlign::Left).unwrap();
            close(bounds.origin.y, top);
            close(bounds.size.height, height);
            top += height;
        }
        close(p.height(), top);
        check_geometry(&p, 200.0);
    }
}

#[test]
fn mixed_height_geometry_matches_source_intersections_after_each_wrap() {
    let s = system();
    let text = "small BIG WORD and tail\n";
    let runs = [
        run(6, Some(12.0), Some(18.0)),
        run(8, Some(32.0), Some(50.0)),
        run(10, Some(16.0), Some(24.0)),
    ];
    let mut p = s
        .shape_paragraph(text.into(), &runs, 16.0, 24.0, None, None)
        .unwrap();
    for width in [80.0, 130.0, 500.0] {
        p.reflow(Some(width));
        let mut top = 0.0;
        for (row, line) in p.layout().lines().enumerate() {
            let range = line.text_range();
            let height = [(0..6, 18.0_f32), (6..14, 50.0), (14..24, 24.0)]
                .into_iter()
                .filter(|(span, _)| span.start < range.end && range.start < span.end)
                .map(|(_, height)| height)
                .reduce(f32::max)
                .unwrap_or(24.0);
            let native = line.metrics();
            let overflow = ((native.ascent + native.descent - height) * 0.5).max(0.0);
            let bounds = p.row_bounds(row, width, TextAlign::Left).unwrap();
            close(bounds.origin.y, top - overflow);
            close(bounds.size.height, height + overflow * 2.0);
            top += height;
        }
        close(p.height(), top);
        check_geometry(&p, width);
    }
}
