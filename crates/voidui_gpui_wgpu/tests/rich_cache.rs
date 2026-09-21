//! Rich paragraph sharing preserves styles and reuses shaping across width probes.
use std::{borrow::Cow, sync::Arc};
use voidui_gpui_wgpu::*;

const TEXT: &str = "one BIG WORD two more";

fn cache() -> TextLayoutCache {
    let backend = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    backend
        .add_fonts(vec![Cow::Borrowed(include_bytes!(
            "fonts/IBMPlexSans-Regular.ttf"
        ))])
        .unwrap();
    TextLayoutCache::new(Arc::new(TextSystem::new(Arc::new(backend))))
}

fn run(len: usize) -> TextRun {
    TextRun {
        len,
        font: font("IBM Plex Sans"),
        ..Default::default()
    }
}

fn runs() -> Arc<[TextRun]> {
    Arc::from([
        run(4),
        TextRun {
            font_size: Some(32.0),
            line_height: Some(48.0),
            color: white(),
            color_is_explicit: true,
            ..run(8)
        },
        run(TEXT.len() - 12),
    ])
}

fn prepare(cache: &TextLayoutCache, runs: Arc<[TextRun]>, width: Option<f32>) -> Arc<Paragraph> {
    cache
        .prepare_runs(TEXT.into(), runs, 16.0, 24.0, width, None)
        .unwrap()
}

#[test]
fn identical_runs_share_even_with_independent_allocations() {
    let cache = cache();
    let retained = runs();
    let a = prepare(&cache, retained.clone(), Some(100.0));
    let stats = cache.stats();
    let b = prepare(&cache, retained, Some(100.0));
    let c = prepare(&cache, runs(), Some(100.0));
    assert!(Arc::ptr_eq(&a, &b));
    assert!(Arc::ptr_eq(&a, &c));
    assert_eq!(cache.stats(), stats);
    let sizes: Vec<_> = a
        .layout()
        .lines()
        .flat_map(|line| line.runs())
        .map(|run| run.font_size())
        .collect();
    assert!(sizes.contains(&16.0) && sizes.contains(&32.0));
}

#[test]
fn each_run_property_has_a_distinct_identity() {
    let cache = cache();
    let base = runs();
    let original = prepare(&cache, base.clone(), None);
    // Change one field at a time, including paint-only fields that do not affect
    // glyph geometry but must survive paragraph sharing.
    let changes: &[fn(&mut TextRun)] = &[
        |r| r.font.family = "Other family".into(),
        |r| r.font.features = FontFeatures(Arc::new(vec![("liga".into(), 0)])),
        |r| r.font.fallbacks = Some(FontFallbacks::from_fonts(vec!["IBM Plex Sans".into()])),
        |r| r.font.weight = FontWeight::BOLD,
        |r| r.font.style = FontStyle::Italic,
        |r| r.font_size = Some(33.0),
        |r| r.font_size = None,
        |r| r.line_height = Some(49.0),
        |r| r.line_height = None,
        |r| r.color = black(),
        |r| r.color_is_explicit = false,
        |r| r.background_color = Some(white()),
        |r| r.underline = Some(UnderlineStyle::default()),
        |r| r.strikethrough = Some(StrikethroughStyle::default()),
    ];
    for (index, change) in changes.iter().enumerate() {
        let mut changed = base.to_vec();
        change(&mut changed[1]);
        let other = prepare(&cache, changed.into(), None);
        assert!(!Arc::ptr_eq(&original, &other), "property {index}");
    }
    let mut changed = base.to_vec();
    changed[0].len += 1;
    changed[1].len -= 1;
    assert!(!Arc::ptr_eq(
        &original,
        &prepare(&cache, changed.into(), None)
    ));
}

#[test]
fn uniform_and_rich_keys_never_share() {
    let cache = cache();
    let uniform = cache
        .prepare(TEXT.into(), &run(0).font, 16.0, 24.0, None, None)
        .unwrap();
    let rich = prepare(&cache, Arc::from([run(TEXT.len())]), None);
    assert!(!Arc::ptr_eq(&uniform, &rich));
    let uniform_again = cache
        .prepare(TEXT.into(), &run(0).font, 16.0, 24.0, None, None)
        .unwrap();
    assert!(Arc::ptr_eq(&uniform, &uniform_again));
    assert!(Arc::ptr_eq(
        &rich,
        &prepare(&cache, Arc::from([run(TEXT.len())]), None)
    ));
}

#[test]
fn widths_share_shaping_and_intrinsic_probes_rejoin_the_pool() {
    let cache = cache();
    let retained = runs();
    let unwrapped = prepare(&cache, retained.clone(), None);
    let height = unwrapped.height();
    let stats = cache.stats();
    let narrow = prepare(&cache, retained.clone(), Some(80.0));
    let wide = prepare(&cache, retained.clone(), Some(500.0));
    assert!(!Arc::ptr_eq(&narrow, &wide));
    assert!(narrow.line_count() > wide.line_count());
    assert_eq!(unwrapped.wrap_width(), None);
    assert_eq!(unwrapped.height(), height);
    for (width, expected) in [
        (None, &unwrapped),
        (Some(80.0), &narrow),
        (Some(500.0), &wide),
    ] {
        assert!(Arc::ptr_eq(
            expected,
            &prepare(&cache, retained.clone(), width)
        ));
    }
    let mut measured = narrow.clone();
    for width in [None, Some(0.0), Some(150.0), Some(80.0)] {
        Arc::make_mut(&mut measured).reflow(width);
    }
    assert!(!Arc::ptr_eq(&measured, &narrow));
    cache.intern_runs(&mut measured, retained.clone(), 16.0, 24.0, None);
    assert!(Arc::ptr_eq(&measured, &narrow));

    // A previously unseen finalized width becomes a reusable weak variant too.
    Arc::make_mut(&mut measured).reflow(Some(130.0));
    cache.intern_runs(&mut measured, retained.clone(), 16.0, 24.0, None);
    assert!(Arc::ptr_eq(
        &measured,
        &prepare(&cache, retained, Some(130.0))
    ));
    assert_eq!(cache.stats(), stats);
}

#[test]
fn source_defaults_clamp_and_font_revision_separate_keys() {
    let cache = cache();
    let original = prepare(&cache, runs(), None);
    for (text, size, height, clamp) in [
        ("ONE BIG WORD two more", 16.0, 24.0, None),
        (TEXT, 17.0, 24.0, None),
        (TEXT, 16.0, 25.0, None),
        (TEXT, 16.0, 24.0, Some(1)),
        (TEXT, 16.0, 24.0, Some(0)),
    ] {
        let other = cache
            .prepare_runs(text.into(), runs(), size, height, None, clamp)
            .unwrap();
        assert!(!Arc::ptr_eq(&original, &other));
    }
    let revision = cache.font_revision();
    cache
        .add_fonts(vec![Cow::Borrowed(include_bytes!(
            "fonts/IBMPlexSans-Regular.ttf"
        ))])
        .unwrap();
    assert_ne!(cache.font_revision(), revision);
    assert!(!Arc::ptr_eq(&original, &prepare(&cache, runs(), None)));
}

#[test]
fn finish_frame_releases_dead_rich_keys_and_preserves_live_variants() {
    let cache = cache();
    let retained = runs();
    let weak_runs = Arc::downgrade(&retained);
    let narrow = prepare(&cache, retained.clone(), Some(80.0));
    let wide = prepare(&cache, retained.clone(), Some(500.0));
    let weak_narrow = Arc::downgrade(&narrow);
    drop(narrow);
    drop(retained);
    cache.finish_frame();
    assert!(weak_narrow.upgrade().is_none());
    assert!(weak_runs.upgrade().is_some());
    assert!(Arc::ptr_eq(&wide, &prepare(&cache, runs(), Some(500.0))));
    let uniform = cache
        .prepare(TEXT.into(), &run(0).font, 16.0, 24.0, None, None)
        .unwrap();
    let weak_uniform = Arc::downgrade(&uniform);
    let weak_wide = Arc::downgrade(&wide);
    drop(uniform);
    drop(wide);
    assert!(weak_wide.upgrade().is_none());
    assert!(weak_uniform.upgrade().is_none());
    cache.finish_frame();
    assert!(weak_runs.upgrade().is_none());
}

#[test]
fn invalid_dimensions_and_run_boundaries_are_rejected_without_shaping() {
    let cache = cache();
    let _live = prepare(&cache, runs(), None);
    let stats = cache.stats();
    for value in [0.0, -0.0, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for clamp in [None, Some(0)] {
            for (size, height) in [(value, 24.0), (16.0, value)] {
                assert!(
                    cache
                        .prepare_runs(TEXT.into(), runs(), size, height, None, clamp)
                        .is_err()
                );
            }
            for change_size in [false, true] {
                let mut invalid = runs().to_vec();
                if change_size {
                    invalid[1].font_size = Some(value);
                } else {
                    invalid[1].line_height = Some(value);
                }
                assert!(
                    cache
                        .prepare_runs(TEXT.into(), invalid.into(), 16.0, 24.0, None, clamp)
                        .is_err()
                );
            }
        }
    }
    for width in [-1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(
            cache
                .prepare_runs(TEXT.into(), runs(), 16.0, 24.0, Some(width), None)
                .is_err()
        );
    }
    for (text, invalid) in [
        ("é", vec![run(1), run(1)]),
        (TEXT, vec![run(TEXT.len() - 1)]),
        (TEXT, vec![run(TEXT.len() + 1)]),
        (TEXT, vec![run(1), run(usize::MAX)]),
    ] {
        for clamp in [None, Some(0)] {
            assert!(
                cache
                    .prepare_runs(text.into(), invalid.clone().into(), 16.0, 24.0, None, clamp)
                    .is_err()
            );
        }
    }
    assert_eq!(cache.stats(), stats);
}

#[test]
fn decoration_and_weight_float_bits_have_total_key_equality() {
    let cache = cache();
    // A hidden paragraph exercises key identity without passing unusual font
    // weights to a platform font resolver. Equal NaN payloads must share even
    // when the caller supplies a fresh run allocation.
    let prepare_hidden = |run| {
        cache
            .prepare_runs("x".into(), Arc::from([run]), 16.0, 24.0, None, Some(0))
            .unwrap()
    };
    let mut base = run(1);
    base.font.weight = FontWeight(f32::NAN);
    base.color.h = f32::NAN;
    base.underline = Some(UnderlineStyle {
        thickness: px(f32::NAN),
        color: Some(white()),
        wavy: false,
    });
    base.strikethrough = Some(StrikethroughStyle {
        thickness: px(f32::NAN),
        color: Some(white()),
    });
    let original = prepare_hidden(base.clone());
    assert!(Arc::ptr_eq(&original, &prepare_hidden(base.clone())));
    let changes: &[fn(&mut TextRun)] = &[
        |r| r.font.weight = FontWeight(f32::from_bits(f32::NAN.to_bits() + 1)),
        |r| r.color.h = f32::from_bits(f32::NAN.to_bits() + 1),
        |r| r.underline.as_mut().unwrap().thickness = px(1.0),
        |r| r.underline.as_mut().unwrap().color = Some(black()),
        |r| r.underline.as_mut().unwrap().wavy = true,
        |r| r.strikethrough.as_mut().unwrap().thickness = px(1.0),
        |r| r.strikethrough.as_mut().unwrap().color = Some(black()),
    ];
    for change in changes {
        let mut changed = base.clone();
        change(&mut changed);
        assert!(!Arc::ptr_eq(&original, &prepare_hidden(changed)));
    }
    for change in [
        (|r: &mut TextRun, v| r.font.weight = FontWeight(v)) as fn(&mut TextRun, f32),
        |r, v| r.underline.as_mut().unwrap().thickness = px(v),
        |r, v| r.strikethrough.as_mut().unwrap().thickness = px(v),
    ] {
        let mut positive = base.clone();
        let mut negative = base.clone();
        change(&mut positive, 0.0);
        change(&mut negative, -0.0);
        assert!(!Arc::ptr_eq(
            &prepare_hidden(positive),
            &prepare_hidden(negative)
        ));
    }
}

#[test]
fn empty_rich_paragraphs_share_without_fonts() {
    let cache = TextLayoutCache::new(Arc::new(TextSystem::new(Arc::new(
        ParleyTextSystem::new_without_system_fonts("unused"),
    ))));
    let a = cache
        .prepare_runs("".into(), Arc::from([]), 16.0, 24.0, None, None)
        .unwrap();
    let b = cache
        .prepare_runs("".into(), Arc::from([]), 16.0, 24.0, None, None)
        .unwrap();
    assert!(Arc::ptr_eq(&a, &b));
    assert_eq!(cache.stats().paragraphs_shaped, 0);
}
