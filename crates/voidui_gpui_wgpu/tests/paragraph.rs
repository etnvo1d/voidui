//! Parley adapter contracts: shared fonts, retained layout and source boundaries.
use std::{borrow::Cow, sync::Arc};
use voidui_gpui_wgpu::*;
fn system() -> Arc<TextSystem> {
    let backend = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    backend
        .add_fonts(vec![Cow::Borrowed(include_bytes!(
            "fonts/IBMPlexSans-Regular.ttf"
        ))])
        .unwrap();
    Arc::new(TextSystem::new(Arc::new(backend)))
}
fn glyphs(p: &Paragraph) -> usize {
    p.layout()
        .lines()
        .flat_map(|l| l.runs())
        .map(|r| {
            r.visual_clusters()
                .map(|c| c.glyphs().count())
                .sum::<usize>()
        })
        .sum()
}
#[test]
fn width_changes_do_not_reshape_or_duplicate_font_resources() {
    let s = system();
    let mut p = s
        .shape_paragraph(
            "One paragraph, many widths.".into(),
            &[TextRun {
                len: 27,
                font: font("IBM Plex Sans"),
                ..Default::default()
            }],
            18.0,
            26.0,
            Some(200.0),
            None,
        )
        .unwrap();
    let stats = s.stats();
    let data = p
        .layout()
        .lines()
        .next()
        .unwrap()
        .runs()
        .next()
        .unwrap()
        .font()
        .data
        .id();
    for width in [100.0, 150.0, 400.0, 80.0] {
        p.reflow(Some(width));
        assert_eq!(s.stats(), stats);
        assert_eq!(
            p.layout()
                .lines()
                .next()
                .unwrap()
                .runs()
                .next()
                .unwrap()
                .font()
                .data
                .id(),
            data
        );
    }
}
#[test]
fn registered_fonts_supply_system_alias_and_explicit_fallbacks() {
    let s = system();
    let normal = s.resolve_font(&font("IBM Plex Sans"));
    assert_eq!(normal, s.resolve_font(&Font::default()));
    let mut missing = font("Missing family");
    missing.fallbacks = Some(FontFallbacks::from_fonts(vec!["IBM Plex Sans".into()]));
    assert_eq!(normal, s.resolve_font(&missing));
    assert!(s.ascent(normal, px(18.0)) > px(0.0));
    assert!(s.ch_advance(normal, px(18.0)).unwrap() > px(0.0));
}
#[test]
fn feature_settings_reach_the_native_shaper() {
    let s = system();
    let enabled = font("IBM Plex Sans");
    let mut disabled = enabled.clone();
    disabled.features = FontFeatures(Arc::new(vec![("liga".into(), 0)]));
    let shape = |f| {
        s.shape_paragraph(
            "ffi".into(),
            &[TextRun {
                len: 3,
                font: f,
                ..Default::default()
            }],
            20.0,
            30.0,
            None,
            None,
        )
        .unwrap()
    };
    assert!(glyphs(&shape(enabled)) < glyphs(&shape(disabled)));
}
#[test]
fn synthetic_faces_have_distinct_raster_identities() {
    let s = system();
    assert_ne!(
        s.resolve_font(&font("IBM Plex Sans")),
        s.resolve_font(&font("IBM Plex Sans").bold())
    );
    assert_ne!(
        s.resolve_font(&font("IBM Plex Sans")),
        s.resolve_font(&font("IBM Plex Sans").italic())
    );
}
#[test]
fn blank_paragraph_and_zero_clamp_need_no_font() {
    let s = TextSystem::new(Arc::new(ParleyTextSystem::new_without_system_fonts(
        "unused",
    )));
    let p = s
        .shape_paragraph("".into(), &[], 16.0, 19.2, None, None)
        .unwrap();
    assert_eq!(p.line_count(), 0);
    assert_eq!(p.height(), 0.0);
    let p = s
        .shape_paragraph(
            "hidden".into(),
            &[TextRun {
                len: 6,
                ..Default::default()
            }],
            16.0,
            19.2,
            None,
            Some(0),
        )
        .unwrap();
    assert_eq!(p.height(), 0.0);
    assert_eq!(s.stats().paragraphs_shaped, 0);
}
#[test]
fn crlf_and_lone_cr_have_one_line_break_and_source_offsets() {
    let s = system();
    let source = "a\r\nb\rc\n";
    let p = s
        .shape_paragraph(
            source.into(),
            &[TextRun {
                len: source.len(),
                font: font("IBM Plex Sans"),
                ..Default::default()
            }],
            16.0,
            24.0,
            None,
            None,
        )
        .unwrap();
    assert_eq!(p.layout_text(), "a\nb\nc\n");
    assert_eq!(p.line_count(), 4);
    assert_eq!(p.height(), 96.0);
    assert_eq!(p.source_index(p.layout_index(3)), 3);
    assert_eq!(p.source(), source);
}
#[test]
fn invalid_utf8_runs_and_feature_tags_are_rejected() {
    let s = system();
    assert!(
        s.shape_paragraph(
            "é".into(),
            &[TextRun {
                len: 1,
                font: font("IBM Plex Sans"),
                ..Default::default()
            }],
            16.0,
            24.0,
            None,
            None
        )
        .is_err()
    );
    let mut f = font("IBM Plex Sans");
    f.features = FontFeatures(Arc::new(vec![("bad".into(), 1)]));
    assert!(
        s.shape_paragraph(
            "x".into(),
            &[TextRun {
                len: 1,
                font: f,
                ..Default::default()
            }],
            16.0,
            24.0,
            None,
            None
        )
        .is_err()
    );
}
#[test]
fn weak_cache_does_not_pin_old_paragraphs() {
    let cache = TextLayoutCache::new(system());
    let a = cache
        .prepare(
            "shared".into(),
            &font("IBM Plex Sans"),
            16.0,
            24.0,
            Some(100.0),
            None,
        )
        .unwrap();
    let weak = Arc::downgrade(&a);
    drop(a);
    cache.finish_frame();
    assert!(weak.upgrade().is_none());
}
#[test]
fn selection_geometry_reuses_layout_without_shaping() {
    // Font/color separation also applies when a caller uses core PreparedText;
    // the renderer exposes native brushes without baking glyph images per color.
    let s = system();
    let p = s
        .shape_paragraph(
            "color".into(),
            &[TextRun {
                len: 5,
                font: font("IBM Plex Sans"),
                color: white(),
                ..Default::default()
            }],
            16.0,
            24.0,
            None,
            None,
        )
        .unwrap();
    let stats = s.stats();
    let range = p.selection_rectangles(0..5, 100.0, TextAlign::Left);
    assert!(!range.is_empty());
    assert_eq!(s.stats(), stats);
}

#[test]
fn a_failed_font_batch_does_not_change_font_revision_or_cached_layout() {
    let s = system();
    let cache = TextLayoutCache::new(s.clone());
    let before = cache.font_revision();
    let a = cache
        .prepare(
            "stable".into(),
            &font("IBM Plex Sans"),
            16.0,
            24.0,
            None,
            None,
        )
        .unwrap();
    assert!(
        s.add_fonts(vec![
            Cow::Borrowed(include_bytes!("fonts/IBMPlexSans-Regular.ttf")),
            Cow::Borrowed(b"not a font")
        ])
        .is_err()
    );
    assert_eq!(before, cache.font_revision());
    let b = cache
        .prepare(
            "stable".into(),
            &font("IBM Plex Sans"),
            16.0,
            24.0,
            None,
            None,
        )
        .unwrap();
    assert!(Arc::ptr_eq(&a, &b));
}

#[test]
fn programmatic_partial_graphemes_paint_the_whole_grapheme() {
    let s = system();
    let p = s
        .shape_paragraph(
            "e\u{301}x".into(),
            &[TextRun {
                len: 4,
                font: font("IBM Plex Sans"),
                ..Default::default()
            }],
            20.0,
            30.0,
            None,
            None,
        )
        .unwrap();
    assert_eq!(
        p.selection_rectangles(1..3, 100.0, TextAlign::Left),
        p.selection_rectangles(0..3, 100.0, TextAlign::Left)
    );
    assert!(
        p.selection_rectangles(1..1, 100.0, TextAlign::Left)
            .is_empty()
    );
}

#[test]
fn shared_text_resources_are_send_and_sync() {
    fn assert_thread_safe<T: Send + Sync>() {}
    assert_thread_safe::<ParleyTextSystem>();
    assert_thread_safe::<TextSystem>();
    assert_thread_safe::<Paragraph>();
    assert_thread_safe::<TextLayoutCache>();
}
