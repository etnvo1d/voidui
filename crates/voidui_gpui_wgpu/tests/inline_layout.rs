//! CSS inline formatting uses font struts and per-item half-leading, including
//! object-only lines. Ahem and IBM Plex exercise different real font metrics.
use std::{borrow::Cow, sync::Arc};
use voidui_gpui_wgpu::*;

const OBJECT: &str = "\u{fffc}";
fn system() -> TextSystem {
    let backend = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    backend
        .add_fonts(vec![
            Cow::Borrowed(include_bytes!("fonts/IBMPlexSans-Regular.ttf")),
            Cow::Borrowed(include_bytes!("fonts/Ahem.ttf")),
        ])
        .unwrap();
    TextSystem::new(Arc::new(backend))
}
fn run(text: &str, family: &str) -> TextRun {
    TextRun {
        len: text.len(),
        font: font(family),
        ..Default::default()
    }
}
fn object(align: InlineAlignment) -> InlineTextBox {
    InlineTextBox {
        id: 1,
        index: 0,
        width: 18.0,
        height: 14.0,
        baseline: 12.5,
        offset_em: 0.0,
        align,
    }
}
fn close(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.001, "{actual} != {expected}");
}
fn metrics(system: &TextSystem, font: &Font, size: f32) -> parley::RunMetrics {
    let paragraph = system
        .shape_paragraph(
            "a".into(),
            &[TextRun {
                len: 1,
                font: font.clone(),
                ..Default::default()
            }],
            size,
            size,
            None,
            None,
        )
        .unwrap();
    *paragraph
        .layout()
        .lines()
        .next()
        .unwrap()
        .runs()
        .next()
        .unwrap()
        .metrics()
}

#[test]
fn adding_and_removing_text_preserves_object_geometry() {
    let system = system();
    for family in ["IBM Plex Sans", "Ahem"] {
        let font = font(family);
        for size in [12.0, 16.0, 27.5] {
            for height in [size * 0.75, size * 1.625, size * 2.5] {
                for align in [
                    InlineAlignment::Baseline,
                    InlineAlignment::Middle,
                    InlineAlignment::TextTop,
                    InlineAlignment::TextBottom,
                ] {
                    let mut previous: Option<(f32, f32, f32)> = None;
                    for suffix in ["", "a", "a ", "", " ", "a", ""] {
                        let text = format!("{OBJECT}{suffix}");
                        let p = system
                            .shape_inline_paragraph(
                                text.clone().into(),
                                &[run(&text, family)],
                                InlineTextStyle {
                                    font: &font,
                                    font_size: size,
                                    line_height: height,
                                },
                                None,
                                None,
                                &[object(align)],
                                0.0,
                            )
                            .unwrap();
                        let b = p.inline_boxes(300.0, TextAlign::Left)[0].2;
                        let actual = (p.height(), p.first_baseline().unwrap(), b.origin.y);
                        if let Some(expected) = previous {
                            close(actual.0, expected.0);
                            close(actual.1, expected.1);
                            close(actual.2, expected.2);
                        }
                        previous = Some(actual);
                        assert_eq!(p.source(), text);
                        assert_eq!(p.layout_text(), suffix);
                    }
                }
            }
        }
    }
}

#[test]
fn alignments_and_tall_boxes_follow_independent_css_extents() {
    let system = system();
    let font = font("IBM Plex Sans");
    let m = metrics(&system, &font, 16.0);
    for height in [12.0, 26.0, 48.0] {
        let leading = (height - m.ascent - m.descent) / 2.0;
        for box_height in [2.0, 14.0, 60.0] {
            for align in [
                InlineAlignment::Baseline,
                InlineAlignment::Middle,
                InlineAlignment::TextTop,
                InlineAlignment::TextBottom,
            ] {
                let mut b = object(align);
                b.height = box_height;
                b.baseline = box_height / 3.0;
                let ascent = match align {
                    InlineAlignment::Baseline => b.baseline,
                    InlineAlignment::Middle => (box_height + m.x_height.unwrap()) / 2.0,
                    InlineAlignment::TextTop => m.ascent,
                    InlineAlignment::TextBottom => box_height - m.descent,
                };
                let over = (m.ascent + leading).max(ascent);
                let under = (m.descent + leading).max(box_height - ascent);
                let p = system
                    .shape_inline_paragraph(
                        OBJECT.into(),
                        &[run(OBJECT, "IBM Plex Sans")],
                        InlineTextStyle {
                            font: &font,
                            font_size: 16.0,
                            line_height: height,
                        },
                        None,
                        None,
                        &[b],
                        0.0,
                    )
                    .unwrap();
                close(p.height(), over + under);
                close(p.first_baseline().unwrap(), over);
                close(
                    p.inline_boxes(300.0, TextAlign::Left)[0].2.origin.y,
                    over - ascent,
                );
                let row = p.row_bounds(0, 300.0, TextAlign::Left).unwrap();
                close(row.origin.y, 0.0_f32.min(over - m.ascent));
                close(
                    row.origin.y + row.size.height,
                    (over + under).max(over + m.descent),
                );
            }
        }
    }
}

#[test]
fn object_only_spans_keep_their_inherited_font_and_line_height() {
    let system = system();
    let parent = font("IBM Plex Sans");
    let mut expected = None;
    for suffix in ["", "a", ""] {
        let text = format!("{OBJECT}{suffix}");
        let mut styled = run(&text, "Ahem");
        styled.font_size = Some(32.0);
        styled.line_height = Some(45.0);
        let p = system
            .shape_inline_paragraph(
                text.into(),
                &[styled],
                InlineTextStyle {
                    font: &parent,
                    font_size: 16.0,
                    line_height: 26.0,
                },
                None,
                None,
                &[object(InlineAlignment::Middle)],
                0.0,
            )
            .unwrap();
        let top = p.inline_boxes(300.0, TextAlign::Left)[0].2.origin.y;
        close(p.height(), 45.0);
        if let Some(y) = expected {
            close(top, y);
        }
        expected = Some(top);
    }
}

#[test]
fn mixed_fonts_combine_each_runs_half_leading_before_line_sizing() {
    let system = system();
    let parent = font("IBM Plex Sans");
    let ahem = font("Ahem");
    let parent_metrics = metrics(&system, &parent, 16.0);
    let child_metrics = metrics(&system, &ahem, 32.0);
    let mut child = run("b", "Ahem");
    child.font_size = Some(32.0);
    child.line_height = Some(12.0);
    let p = system
        .shape_inline_paragraph(
            "ab".into(),
            &[run("a", "IBM Plex Sans"), child],
            InlineTextStyle {
                font: &parent,
                font_size: 16.0,
                line_height: 26.0,
            },
            None,
            None,
            &[],
            0.0,
        )
        .unwrap();
    let parent_over =
        parent_metrics.ascent + (26.0 - parent_metrics.ascent - parent_metrics.descent) / 2.0;
    let child_over =
        child_metrics.ascent + (12.0 - child_metrics.ascent - child_metrics.descent) / 2.0;
    close(p.first_baseline().unwrap(), parent_over.max(child_over));
    close(
        p.height(),
        parent_over.max(child_over) + (26.0 - parent_over).max(12.0 - child_over),
    );
}

#[test]
fn wrapping_reuses_shaping_and_all_display_queries_share_the_rows() {
    let system = system();
    let parent = font("IBM Plex Sans");
    let text = format!("one {OBJECT} two three\r\nfour five {OBJECT} six seven\n");
    let boxes: Vec<_> = text
        .match_indices(OBJECT)
        .enumerate()
        .map(|(id, (index, _))| InlineTextBox {
            id: id as u64,
            index,
            height: 48.0,
            baseline: 10.0,
            ..object(InlineAlignment::Baseline)
        })
        .collect();
    let mut p = system
        .shape_inline_paragraph(
            text.clone().into(),
            &[run(&text, "IBM Plex Sans")],
            InlineTextStyle {
                font: &parent,
                font_size: 16.0,
                line_height: 26.0,
            },
            Some(90.0),
            None,
            &boxes,
            0.0,
        )
        .unwrap();
    let stats = system.stats();
    for width in [400.0, 90.0, 55.0, 90.0] {
        p.reflow(Some(width));
        assert_eq!(
            system.stats(),
            stats,
            "reflow must not resolve fonts or reshape"
        );
        for (row, line) in p.layout().lines().enumerate() {
            let byte = p.source_index(line.text_range().start);
            let bounds = p.row_bounds(row, width, TextAlign::Left).unwrap();
            let caret = p
                .caret_bounds(byte, parley::Affinity::Downstream, width, TextAlign::Left)
                .unwrap();
            close(caret.origin.y, bounds.origin.y);
            close(caret.size.height, bounds.size.height);
            let hit = p
                .cursor_at(
                    point(bounds.origin.x, bounds.origin.y + bounds.size.height / 2.0),
                    width,
                    TextAlign::Left,
                )
                .unwrap();
            assert_eq!(p.cursor_row(hit), row);
        }
        for (row, rect) in p.selection_rectangles(0..text.len(), width, TextAlign::Left) {
            let bounds = p.row_bounds(row, width, TextAlign::Left).unwrap();
            close(rect.origin.y, bounds.origin.y);
            close(rect.size.height, bounds.size.height);
        }
        for (_, _, rect) in p.inline_boxes(width, TextAlign::Left) {
            assert!(rect.origin.y >= 0.0);
            assert!(rect.origin.y + rect.size.height <= p.height() + 0.001);
        }
    }
}

#[test]
fn registering_fonts_invalidates_cached_strut_metrics() {
    let backend = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    backend
        .add_fonts(vec![Cow::Borrowed(include_bytes!(
            "fonts/IBMPlexSans-Regular.ttf"
        ))])
        .unwrap();
    let system = TextSystem::new(Arc::new(backend));
    let parent = font("Ahem");
    let shape = || {
        system
            .shape_inline_paragraph(
                OBJECT.into(),
                &[run(OBJECT, "Ahem")],
                InlineTextStyle {
                    font: &parent,
                    font_size: 16.0,
                    line_height: 26.0,
                },
                None,
                None,
                &[object(InlineAlignment::Middle)],
                0.0,
            )
            .unwrap()
    };
    let before = shape().first_baseline().unwrap();
    system
        .add_fonts(vec![Cow::Borrowed(include_bytes!("fonts/Ahem.ttf"))])
        .unwrap();
    let after = shape().first_baseline().unwrap();
    assert!((before - after).abs() > 0.01);
    close(after, shape().first_baseline().unwrap());
}

#[test]
fn inline_geometry_matches_browser_reference() {
    let system = system();
    let reference: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/inline-layout-browser.json")).unwrap();
    let mut max_difference = 0.0_f32;
    for case in reference["cases"].as_array().unwrap() {
        let family = match case["family"].as_str().unwrap() {
            "PlexTest" => "IBM Plex Sans",
            "AhemTest" => "Ahem",
            _ => unreachable!(),
        };
        let font = font(family);
        let align = match case["align"].as_str().unwrap() {
            "baseline" => InlineAlignment::Baseline,
            "middle" => InlineAlignment::Middle,
            "text-top" => InlineAlignment::TextTop,
            "text-bottom" => InlineAlignment::TextBottom,
            _ => unreachable!(),
        };
        let mut b = object(align);
        // An empty CSS inline-block exports its bottom edge as its baseline.
        b.baseline = b.height;
        let p = system
            .shape_inline_paragraph(
                OBJECT.into(),
                &[run(OBJECT, family)],
                InlineTextStyle {
                    font: &font,
                    font_size: case["size"].as_f64().unwrap() as f32,
                    line_height: case["lineHeight"].as_f64().unwrap() as f32,
                },
                None,
                None,
                &[b],
                0.0,
            )
            .unwrap();
        let top = p.inline_boxes(600.0, TextAlign::Left)[0].2.origin.y;
        for (key, actual) in [
            ("height", p.height()),
            ("baseline", p.first_baseline().unwrap()),
            ("boxTop", top),
        ] {
            let difference = (actual - case[key].as_f64().unwrap() as f32).abs();
            max_difference = max_difference.max(difference);
            // Browser font metrics and layout coordinates are pixel-quantized;
            // Parley preserves fractional metrics. This comparison allows that
            // rounding only. The typing/deletion test above allows just 0.001px.
            assert!(
                difference <= 0.5,
                "{case}: {key}={actual}, difference={difference}"
            );
        }
    }
    println!("48 browser reference cases: maximum coordinate difference {max_difference:.6}px");
}

#[test]
fn optical_offsets_use_surrounding_font_size_without_changing_line_geometry() {
    let system = system();
    let parent = font("IBM Plex Sans");
    for family in ["IBM Plex Sans", "Ahem"] {
        for size in [12.0, 16.0, 28.0] {
            for text in [OBJECT.to_owned(), format!("{OBJECT}a")] {
                let mut span = run(&text, family);
                span.font_size = Some(size);
                let shape = |offset| {
                    let mut b = object(InlineAlignment::Middle);
                    b.offset_em = offset;
                    system.shape_inline_paragraph(
                        text.clone().into(),
                        &[span.clone()],
                        InlineTextStyle {
                            font: &parent,
                            font_size: 16.0,
                            line_height: 26.0,
                        },
                        None,
                        None,
                        &[b],
                        0.0,
                    )
                };
                let before = shape(0.0).unwrap();
                for offset in [-0.1, 0.1] {
                    let after = shape(offset).unwrap();
                    close(after.height(), before.height());
                    close(
                        after.first_baseline().unwrap(),
                        before.first_baseline().unwrap(),
                    );
                    let original = before.inline_boxes(300.0, TextAlign::Left)[0].2;
                    let moved = after.inline_boxes(300.0, TextAlign::Left)[0].2;
                    close(moved.origin.y - original.origin.y, offset * size);
                    assert_eq!(moved.size, original.size);
                    assert_eq!(
                        before.row_bounds(0, 300.0, TextAlign::Left),
                        after.row_bounds(0, 300.0, TextAlign::Left)
                    );
                }
                assert!(shape(f32::NAN).is_err());
            }
        }
    }
}

#[test]
fn source_hits_include_both_sides_of_inline_objects_and_row_margins() {
    let system = system();
    let parent = font("IBM Plex Sans");
    for text in [
        OBJECT.to_owned(),
        format!("{OBJECT}text"),
        format!("{OBJECT}{OBJECT}text"),
    ] {
        let boxes: Vec<_> = text
            .match_indices(OBJECT)
            .enumerate()
            .map(|(i, (index, _))| InlineTextBox {
                id: i as u64,
                index,
                offset_em: -0.1,
                ..object(InlineAlignment::Middle)
            })
            .collect();
        let mut p = system
            .shape_inline_paragraph(
                text.clone().into(),
                &[run(&text, "IBM Plex Sans")],
                InlineTextStyle {
                    font: &parent,
                    font_size: 16.0,
                    line_height: 26.0,
                },
                Some(300.0),
                None,
                &boxes,
                0.0,
            )
            .unwrap();
        for width in [300.0, 20.0] {
            p.reflow(Some(width));
            assert_eq!(p.source_row_edge(0, false).unwrap().0, 0);
            assert_eq!(
                p.source_row_edge(p.line_count() - 1, true).unwrap().0,
                text.len()
            );
            for align in [TextAlign::Left, TextAlign::Center, TextAlign::Right] {
                let placed = p.inline_boxes(width, align);
                for (_, index, bounds) in &placed {
                    let y = bounds.origin.y + bounds.size.height / 2.0;
                    for (x, expected) in [
                        (bounds.origin.x + 0.25, *index),
                        (
                            bounds.origin.x + bounds.size.width - 0.25,
                            index + OBJECT.len(),
                        ),
                    ] {
                        assert_eq!(
                            p.source_cursor_at(point(x, y), width, align).unwrap().0,
                            expected,
                            "{text:?} width={width}, align={align:?}"
                        );
                    }
                }
                let first = placed[0].2;
                let y = first.origin.y + first.size.height / 2.0;
                assert_eq!(
                    p.source_cursor_at(point(first.origin.x - 10.0, y), width, align)
                        .unwrap()
                        .0,
                    0
                );
            }
        }
    }
}
