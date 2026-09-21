//! End-to-end source/projection/layout contracts with bundled fonts and no window.
#![cfg(feature = "editing")]
use std::{
    borrow::Cow,
    cell::{Cell, RefCell},
    collections::BTreeSet,
    rc::Rc,
    sync::Arc,
};
use voidui::{
    core::geometry::{Point, Rect},
    editing::*,
    render::{self, ParleyTextSystem, TextAlign, TextLayoutCache, TextSystem},
};
fn system() -> Arc<TextSystem> {
    let b = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    b.add_fonts(vec![Cow::Borrowed(include_bytes!(
        "../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
    ))])
    .unwrap();
    Arc::new(TextSystem::new(Arc::new(b)))
}
fn options() -> LayoutOptions {
    LayoutOptions {
        font: render::font("IBM Plex Sans"),
        font_size: 16.0,
        line_height: 24.0,
        width: Some(240.0),
    }
}
fn prepare(state: &EditorState, p: Projection) -> EditorLayout {
    let mut l = EditorLayout::default();
    l.prepare_snapshot(
        state.snapshot(),
        p,
        options(),
        system(),
        None,
        Default::default(),
    )
    .unwrap();
    l
}
#[test]
fn chunked_edits_snapshots_and_graphemes_do_not_materialize_document() {
    let mut s = EditorState::new(format!("{}👩‍💻\r\nend", "x".repeat(10000)));
    let old = s.snapshot();
    let end = s.document().len() - 3;
    assert!(s.document().buffer().is_chunked());
    assert!(!s.document().buffer().is_materialized());
    s.select(SelectionSet::single(Selection::caret(10000 + "👩‍💻".len())))
        .unwrap();
    s.delete(false, false).unwrap();
    assert_eq!(s.document().read(10000..10005).unwrap(), "\r\nend");
    assert!(!s.document().buffer().is_materialized());
    assert_eq!(old.text.read(10000..end).unwrap(), "👩‍💻\r\n");
    s.undo().unwrap();
    assert!(!s.document().buffer().is_materialized());
    assert_eq!(s.document().read(10000..end).unwrap(), "👩‍💻\r\n");
}
#[test]
fn replacements_and_insertions_have_directional_source_mapping() {
    let s = EditorState::new("**中** end");
    let mut p = Projection::new()
        .replace(Replacement::hide(ViewId(1), 0..2))
        .replace(Replacement::hide(ViewId(2), 5..7));
    p.validate(s.document()).unwrap();
    let text = p.project(s.document(), 0..s.document().len(), &[]).unwrap();
    assert_eq!(text.text, "中 end");
    assert_eq!(text.map.to_source(0, Bias::After), 2);
    assert_eq!(text.map.to_source(3, Bias::Before), 5);
    assert_eq!(text.map.to_source(3, Bias::After), 7);
    assert_eq!(text.map.to_display(3, Bias::Before), 1);
    let mut p = Projection::new().replace(Replacement::text(ViewId(3), 2..2, "hint"));
    p.validate(s.document()).unwrap();
    let t = p.project(s.document(), 0..s.document().len(), &[]).unwrap();
    assert_eq!(t.map.to_source(4, Bias::After), 2);
    assert_eq!(t.map.to_display(2, Bias::Before), 2);
    assert_eq!(t.map.to_display(2, Bias::After), 6);
}
#[test]
fn reveal_on_selection_and_projection_history_are_independent() {
    let mut s = EditorState::new("**hello** tail");
    s.select(SelectionSet::single(Selection::caret(12)))
        .unwrap();
    let p = Projection::new()
        .replace(Replacement::text(ViewId(1), 0..9, "hello").reveal_on_selection());
    s.set_projection(0, p.clone()).unwrap();
    assert!(!s.can_undo());
    assert_eq!(s.revision(), 0);
    assert_eq!(p.active(s.selections(), None).replacements.len(), 1);
    s.select(SelectionSet::single(Selection::caret(3))).unwrap();
    assert!(p.active(s.selections(), None).replacements.is_empty());
    let anchor = s.create_anchor(9, Bias::After).unwrap();
    s.transact(Transaction::new(0, [Edit::new(0..0, "x")]))
        .unwrap();
    assert_eq!(s.anchor(anchor).unwrap().byte, 10);
    assert!(s.projection().replacements.is_empty());
    s.undo().unwrap();
    assert_eq!(s.anchor(anchor).unwrap().byte, 9);
    assert!(s.set_projection(0, p).is_err());
}
#[test]
fn hidden_markers_use_source_coordinates_for_caret_hit_and_delete() {
    let s = EditorState::new("**hello**");
    let p = Projection::new()
        .replace(Replacement::hide(ViewId(1), 0..2))
        .replace(Replacement::hide(ViewId(2), 7..9));
    let l = prepare(&s, p);
    let caret = l.caret(2, Bias::After, 240.0, TextAlign::Left).unwrap();
    assert!(caret.origin.x.abs() < 0.1);
    let hit = l.hit_test(Point::new(0.0, 12.0), 240.0, TextAlign::Left);
    assert_eq!(hit.head, 2);
    let next = l.move_selection(
        Selection::caret(6),
        Motion::Right,
        false,
        240.0,
        TextAlign::Left,
        100.0,
    );
    assert!(next.head == 7 || next.head == 9);
    assert_eq!(
        l.deletion_range(
            Selection::caret(9),
            false,
            false,
            240.0,
            TextAlign::Left,
            100.0
        ),
        7..9
    );
}
#[test]
fn folding_multiple_paragraphs_reduces_layout_without_changing_source() {
    let s = EditorState::new("a\nb\nc\nd");
    let plain = prepare(&s, Projection::default());
    let folded = prepare(
        &s,
        Projection::new().replace(Replacement::text(ViewId(1), 2..6, "…")),
    );
    assert!(folded.size().height < plain.size().height);
    assert_eq!(s.text(), "a\nb\nc\nd");
    assert!(
        folded
            .caret(6, Bias::After, 240.0, TextAlign::Left)
            .is_some()
    );
}
/// A full-width annotation wraps below its source, like a live formula preview.
struct PreviewView;
impl EmbeddedView for PreviewView {
    fn measure(&mut self, width: f32, _: &TextLayoutCache) -> render::Result<ViewMetrics> {
        Ok(ViewMetrics::new(width, 80.0))
    }
    fn paint(&mut self, _: &mut render::Painter<'_>, _: Rect<f32>) -> render::Result<()> {
        Ok(())
    }
}

#[test]
fn inserted_preview_keeps_upstream_caret_on_source_text() {
    for source in [
        "$$x$$",
        "$$x$$\nnext",
        "$$\nx\n$$",
        "$$\r\nx\r\n$$\r\nnext",
        "$$x$$ tail",
        r"$$P_n(x) = f(x_0) + \frac{f'(x_0)}{1!}(x-x_{0})+\dots+\frac{f^n(x_{0})}{n!}(x - x_{0})^n$$",
    ] {
        let end = source.rfind("$$").unwrap() + 2;
        let state = EditorState::new(source);
        let plain = prepare(&state, Projection::new());
        let projection = Projection::new().replace(Replacement::object(ViewId(1), end..end));
        let projected = projection
            .project(state.document(), 0..source.len(), &[])
            .unwrap();
        assert_eq!(projected.map.to_display(end, Bias::Before), end);
        assert_eq!(
            projected.map.to_display(end, Bias::After),
            end + '\u{fffc}'.len_utf8()
        );

        let mut layout = EditorLayout::default();
        layout.configure_views(
            EditorViews::new().register(ViewId(1), || Box::new(PreviewView)),
            None,
        );
        layout
            .prepare_snapshot(
                state.snapshot(),
                projection,
                options(),
                system(),
                None,
                Default::default(),
            )
            .unwrap();

        for extend in [false, true] {
            let moved = layout.move_selection(
                Selection::caret(end - 1),
                Motion::Right,
                extend,
                240.0,
                TextAlign::Left,
                300.0,
            );
            assert_eq!(moved.head, end, "{source:?}");
            assert_eq!(moved.anchor, if extend { end - 1 } else { end });
            assert_eq!(moved.affinity, Bias::Before);
            let expected = plain
                .caret(end, Bias::Before, 240.0, TextAlign::Left)
                .unwrap();
            let caret = layout
                .caret(moved.head, moved.affinity, 240.0, TextAlign::Left)
                .unwrap();
            assert_eq!(
                caret, expected,
                "the preview must not take over the source caret: {source:?}"
            );

            if !extend {
                let back = layout.move_selection(
                    moved,
                    Motion::Left,
                    false,
                    240.0,
                    TextAlign::Left,
                    300.0,
                );
                assert_eq!(back.head, end - 1);
                let hit = layout.hit_test(
                    Point::new(
                        caret.origin.x - 0.1,
                        caret.origin.y + caret.size.height * 0.5,
                    ),
                    240.0,
                    TextAlign::Left,
                );
                assert_eq!(hit.head, end);
                assert_eq!(
                    layout.caret(hit.head, hit.affinity, 240.0, TextAlign::Left),
                    Some(caret)
                );
            }
        }

        // The same source offset also has a downstream position after the annotation.
        // Keeping affinity must allow the two positions to occupy different rows.
        let before = layout
            .caret(end, Bias::Before, 240.0, TextAlign::Left)
            .unwrap();
        let after = layout
            .caret(end, Bias::After, 240.0, TextAlign::Left)
            .unwrap();
        assert!(after.origin.y > before.origin.y, "{source:?}");
    }
}

struct BoxView {
    mounted: Rc<Cell<usize>>,
    unmounted: Rc<Cell<usize>>,
}
impl EmbeddedView for BoxView {
    fn measure(&mut self, _: f32, _: &TextLayoutCache) -> render::Result<ViewMetrics> {
        Ok(ViewMetrics::new(48.0, 40.0))
    }
    fn paint(&mut self, _: &mut render::Painter<'_>, _: Rect<f32>) -> render::Result<()> {
        Ok(())
    }
    fn mounted(&mut self, _: Option<voidui::core::updates::WidgetInvalidator>) {
        self.mounted.set(self.mounted.get() + 1);
    }
    fn unmounted(&mut self) {
        self.unmounted.set(self.unmounted.get() + 1);
    }
}

#[test]
fn dynamic_view_factories_compose_after_explicit_registrations() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mounted = Rc::new(Cell::new(0));
    let unmounted = Rc::new(Cell::new(0));
    let mut views = EditorViews::new();
    for provider in 0..3 {
        let (calls, mounted, unmounted) = (calls.clone(), mounted.clone(), unmounted.clone());
        views = views.register_dynamic(move |id| {
            calls.borrow_mut().push((provider, id));
            (id.0 == provider + 1).then(|| {
                Box::new(BoxView {
                    mounted: mounted.clone(),
                    unmounted: unmounted.clone(),
                }) as Box<dyn EmbeddedView>
            })
        });
    }
    let (m, u) = (mounted.clone(), unmounted.clone());
    views = views.register(ViewId(1), move || {
        Box::new(BoxView {
            mounted: m.clone(),
            unmounted: u.clone(),
        })
    });
    assert!(views.contains(ViewId(1)));
    assert!(!views.contains(ViewId(2)));
    assert!(calls.borrow().is_empty());

    let state = EditorState::new("abc");
    let projection = Projection::new()
        .replace(Replacement::object(ViewId(1), 0..1))
        .replace(Replacement::object(ViewId(2), 1..2));
    let mut layout = EditorLayout::default();
    let system = system();
    for _ in 0..2 {
        // Reconciliation with a clone must not recreate the dynamic instances.
        layout.configure_views(views.clone(), None);
        layout
            .prepare_snapshot(
                state.snapshot(),
                projection.clone(),
                options(),
                system.clone(),
                None,
                Default::default(),
            )
            .unwrap();
    }
    assert_eq!(*calls.borrow(), [(0, ViewId(2)), (1, ViewId(2))]);
    assert_eq!(mounted.get(), 2);
    assert_eq!(unmounted.get(), 0);
    drop(layout);
    assert_eq!(unmounted.get(), 2);
}

#[test]
fn dynamic_views_follow_model_and_projection_changes_without_reregistration() {
    let ids = Rc::new(RefCell::new(BTreeSet::new()));
    let mounted = Rc::new(Cell::new(0));
    let unmounted = Rc::new(Cell::new(0));
    let (model, m, u) = (ids.clone(), mounted.clone(), unmounted.clone());
    let views = EditorViews::new().register_dynamic(move |id| {
        model.borrow().contains(&id).then(|| {
            Box::new(BoxView {
                mounted: m.clone(),
                unmounted: u.clone(),
            }) as Box<dyn EmbeddedView>
        })
    });
    let state = EditorState::new("a\nb");
    let mut layout = EditorLayout::default();
    layout.configure_views(views, None);
    let system = system();
    let mut prepare = |projection| {
        layout.prepare_snapshot(
            state.snapshot(),
            projection,
            options(),
            system.clone(),
            Some(Rect::from_xywh(0.0, 0.0, 240.0, 200.0)),
            Default::default(),
        )
    };
    prepare(Projection::new()).unwrap();
    let inline = Projection::new().replace(Replacement::object(ViewId(10), 0..1));
    let error = prepare(inline.clone()).unwrap_err();
    assert!(error.to_string().contains("no view factory for ViewId(10)"));
    assert_eq!(mounted.get(), 0);

    // A miss must not be cached: the parser can publish data after configuration.
    ids.borrow_mut().insert(ViewId(10));
    prepare(inline.clone()).unwrap();
    assert_eq!(mounted.get(), 1);
    ids.borrow_mut().insert(ViewId(20));
    prepare(inline.block(ViewId(20), 2..3)).unwrap();
    assert_eq!(mounted.get(), 2);
    assert_eq!(unmounted.get(), 0);

    ids.borrow_mut().remove(&ViewId(10));
    prepare(Projection::new().block(ViewId(20), 2..3)).unwrap();
    assert_eq!(unmounted.get(), 1);
    ids.borrow_mut().clear();
    prepare(Projection::new()).unwrap();
    assert_eq!(unmounted.get(), 2);
    assert_eq!(layout.stats().mounted_views, 0);
}

#[test]
fn dynamic_views_remount_offscreen_and_when_factories_change() {
    let state = EditorState::new(format!("[image]\n{}", "row\n".repeat(1000)));
    let mounted = Rc::new(Cell::new(0));
    let unmounted = Rc::new(Cell::new(0));
    let calls = Rc::new(Cell::new(0));
    let configure = || {
        let (calls, m, u) = (calls.clone(), mounted.clone(), unmounted.clone());
        EditorViews::new().register_dynamic(move |_| {
            calls.set(calls.get() + 1);
            Some(Box::new(BoxView {
                mounted: m.clone(),
                unmounted: u.clone(),
            }))
        })
    };
    let mut layout = EditorLayout::default();
    layout.configure_views(configure(), None);
    let system = system();
    let projection = Projection::new().replace(Replacement::object(ViewId(1), 0..7));
    let viewport = Rect::from_xywh(0.0, 0.0, 240.0, 120.0);
    layout
        .prepare_snapshot(
            state.snapshot(),
            projection.clone(),
            options(),
            system.clone(),
            Some(viewport),
            Default::default(),
        )
        .unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(mounted.get(), 1);
    layout
        .set_viewport(Rect::from_xywh(0.0, 5000.0, 240.0, 120.0))
        .unwrap();
    assert_eq!(unmounted.get(), 1);
    layout.set_viewport(viewport).unwrap();
    assert_eq!(calls.get(), 2);
    assert_eq!(mounted.get(), 2);

    // A different callback invalidates retained instances and cached geometry.
    layout.configure_views(configure(), None);
    assert_eq!(unmounted.get(), 2);
    layout
        .prepare_snapshot(
            state.snapshot(),
            projection,
            options(),
            system,
            Some(viewport),
            Default::default(),
        )
        .unwrap();
    assert_eq!(calls.get(), 3);
    assert_eq!(mounted.get(), 3);
    layout.configure_views(EditorViews::new(), None);
    assert_eq!(unmounted.get(), 3);
    drop(layout);
    assert_eq!(unmounted.get(), 3);
}

#[test]
fn inline_objects_share_wrapping_hit_testing_and_source_deletion() {
    let s = EditorState::new("left [image] right");
    let p = Projection::new().replace(Replacement::object(ViewId(7), 5..12));
    let mount = Rc::new(Cell::new(0));
    let unmount = Rc::new(Cell::new(0));
    let (m, u) = (mount.clone(), unmount.clone());
    let views = EditorViews::new().register(ViewId(7), move || {
        Box::new(BoxView {
            mounted: m.clone(),
            unmounted: u.clone(),
        })
    });
    let mut l = EditorLayout::default();
    l.configure_views(views, None);
    l.prepare_snapshot(
        s.snapshot(),
        p,
        options(),
        system(),
        None,
        Default::default(),
    )
    .unwrap();
    assert!(l.size().height >= 40.0);
    let a = l.caret(5, Bias::Before, 240.0, TextAlign::Left).unwrap();
    let z = l.caret(12, Bias::After, 240.0, TextAlign::Left).unwrap();
    assert!((z.origin.x - a.origin.x - 48.0).abs() < 0.1);
    let object = l
        .object_at(
            Point::new(a.origin.x + 20.0, a.origin.y + 10.0),
            240.0,
            TextAlign::Left,
        )
        .unwrap();
    assert_eq!(object.0, ViewId(7));
    assert_eq!(object.1, 5..12);
    assert_eq!(
        l.deletion_range(
            Selection::caret(12),
            false,
            false,
            240.0,
            TextAlign::Left,
            100.0
        ),
        5..12
    );
    assert_eq!(mount.get(), 1);
    drop(l);
    assert_eq!(unmount.get(), 1);
}
#[test]
fn paragraph_alignment_indentation_and_spacing_drive_carets() {
    let s = EditorState::new("one\ntwo");
    let p = Projection::new().paragraph(
        0..4,
        ParagraphStyle {
            align: Some(TextAlign::Right),
            inset_left: 20.0,
            inset_right: 10.0,
            space_before: 8.0,
            space_after: 12.0,
            ..Default::default()
        },
    );
    let l = prepare(&s, p);
    let a = l.caret(0, Bias::After, 240.0, TextAlign::Left).unwrap();
    assert!(a.origin.x > 100.0);
    assert!((a.origin.y - 8.0).abs() < 0.1);
    assert!(
        (l.caret(4, Bias::After, 240.0, TextAlign::Left)
            .unwrap()
            .origin
            .y
            - 44.0)
            .abs()
            < 0.1
    );
}
#[test]
fn dynamic_block_layouts_resolve_runtime_ids() {
    let s = EditorState::new("a|b\nc|d");
    let mut l = EditorLayout::default();
    let grid: std::rc::Rc<dyn BlockLayout> = std::rc::Rc::new(GridBlock {
        rows: vec![vec![0..1, 2..3], vec![4..5, 6..7]],
        column_weights: vec![1.0, 1.0],
        padding: 4.0,
        gap: 1.0,
        rule: None,
    });
    let views = EditorViews::new().block_dynamic(move |id| (id.0 >= 1000).then(|| grid.clone()));
    l.configure_views(views, None);
    l.prepare_snapshot(
        s.snapshot(),
        Projection::new().block(ViewId(4321), 0..7),
        options(),
        system(),
        None,
        Default::default(),
    )
    .unwrap();
    let a = l.caret(0, Bias::After, 240.0, TextAlign::Left).unwrap();
    let b = l.caret(2, Bias::After, 240.0, TextAlign::Left).unwrap();
    assert!(
        b.origin.x > a.origin.x + 80.0,
        "grid columns were not applied"
    );
}
#[test]
fn source_backed_grid_cells_share_editor_geometry() {
    let s = EditorState::new("a|b\nc|d");
    let mut l = EditorLayout::default();
    let grid = GridBlock {
        rows: vec![vec![0..1, 2..3], vec![4..5, 6..7]],
        column_weights: vec![1.0, 1.0],
        padding: 4.0,
        gap: 1.0,
        rule: None,
    };
    l.configure_views(EditorViews::new().block(ViewId(10), grid), None);
    l.prepare_snapshot(
        s.snapshot(),
        Projection::new().block(ViewId(10), 0..7),
        options(),
        system(),
        None,
        Default::default(),
    )
    .unwrap();
    let a = l.caret(0, Bias::After, 240.0, TextAlign::Left).unwrap();
    let b = l.caret(2, Bias::After, 240.0, TextAlign::Left).unwrap();
    assert!(b.origin.x > a.origin.x + 80.0);
    assert_eq!(
        l.hit_test(
            Point::new(b.origin.x, b.origin.y + 12.0),
            240.0,
            TextAlign::Left
        )
        .head,
        2
    );
    assert_eq!(
        l.move_selection(
            Selection::caret(0),
            Motion::Down,
            false,
            240.0,
            TextAlign::Left,
            100.0
        )
        .head,
        4
    );
}
#[test]
fn viewport_limits_glyph_cache_and_does_not_flatten_large_sources() {
    let s = EditorState::new("short paragraph\n".repeat(20000));
    let sys = system();
    let mut l = EditorLayout::default();
    l.prepare_snapshot(
        s.snapshot(),
        Projection::default(),
        options(),
        sys.clone(),
        Some(Rect::from_xywh(0.0, 0.0, 240.0, 300.0)),
        ViewportOptions {
            max_cached_blocks: 64,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!s.document().buffer().is_materialized());
    assert!(sys.stats().paragraphs_shaped < 100);
    assert_eq!(l.paragraph_count(), 20001);
    for y in [10000.0, 200000.0, 400000.0, 0.0] {
        l.set_viewport(Rect::from_xywh(0.0, y, 240.0, 300.0))
            .unwrap();
        assert!(l.stats().cached_blocks <= 64);
    }
    assert!(sys.stats().paragraphs_shaped < 250);
    let caret = l.caret(s.document().len(), Bias::After, 240.0, TextAlign::Left);
    assert!(caret.is_some());
}

#[test]
fn compound_block_can_virtualize_children_and_materialize_a_remote_caret() {
    struct VirtualRows;
    impl BlockLayout for VirtualRows {
        fn layout(
            &self,
            source: std::ops::Range<usize>,
            m: &mut dyn BlockMeasure,
        ) -> render::Result<BlockArrangement> {
            let count = source.len() / 4;
            let view = m.viewport().unwrap();
            let first = (view.origin.y.max(0.0) / 24.0) as usize;
            let last = ((view.origin.y + view.size.height) / 24.0).ceil() as usize;
            let mut rows: std::collections::BTreeSet<_> = (first..last.min(count)).collect();
            if let Some(position) = m.required_position() {
                rows.insert(((position - source.start) / 4).min(count - 1));
            }
            let mut out = BlockArrangement {
                viewport_dependent: true,
                size: voidui::core::geometry::Size::new(m.available_width(), count as f32 * 24.0),
                ..Default::default()
            };
            for row in rows {
                let range = source.start + row * 4..source.start + row * 4 + 3;
                m.measure_text(range.clone(), m.available_width())?;
                out.cells.push(TextCell {
                    source: range,
                    bounds: Rect::from_xywh(0.0, row as f32 * 24.0, m.available_width(), 24.0),
                });
            }
            Ok(out)
        }
    }
    let s = EditorState::new("row\n".repeat(10000));
    let sys = system();
    let mut l = EditorLayout::default();
    l.configure_views(EditorViews::new().block(ViewId(4), VirtualRows), None);
    l.prepare_snapshot(
        s.snapshot(),
        Projection::new().block(ViewId(4), 0..s.document().len()),
        options(),
        sys.clone(),
        Some(Rect::from_xywh(0.0, 0.0, 240.0, 240.0)),
        Default::default(),
    )
    .unwrap();
    assert!(sys.stats().paragraphs_shaped < 20);
    assert_eq!(l.paragraph_count(), 2); // The final source newline retains an editable empty paragraph.
    l.set_viewport(Rect::from_xywh(0.0, 2400.0, 240.0, 240.0))
        .unwrap();
    assert_eq!(
        l.hit_test(Point::new(0.0, 2405.0), 240.0, TextAlign::Left)
            .head,
        400
    );
    let caret = l.caret(36000, Bias::After, 240.0, TextAlign::Left).unwrap();
    assert!((caret.origin.y - 216000.0).abs() < 0.1);
    assert!(sys.stats().paragraphs_shaped < 60);
    assert!(!s.document().buffer().is_materialized());
}

#[test]
fn text_snapshot_line_indices_match_small_and_chunked_storage() {
    let text = "a\r\n中\u{85}last\n";
    let small = Document::with_buffer_options(
        text,
        BufferOptions {
            inline_bytes: usize::MAX,
        },
    )
    .snapshot();
    let large = Document::with_buffer_options(text, BufferOptions { inline_bytes: 0 }).snapshot();
    assert_eq!(
        small.line_ranges().collect::<Vec<_>>(),
        large.line_ranges().collect::<Vec<_>>()
    );
    for byte in text.char_indices().map(|(i, _)| i).chain([text.len()]) {
        assert_eq!(small.line_at(byte), large.line_at(byte), "{byte}");
    }
}

#[test]
fn incremental_line_index_matches_a_fresh_layout_through_edits_and_undo() {
    let mut s = EditorState::new("head\nbody\ntail\n");
    let sys = system();
    let mut l = EditorLayout::default();
    for tx in [
        Transaction::new(0, [Edit::new(5..5, "new\n")]),
        Transaction::new(1, [Edit::new(0..5, "")]),
        Transaction::new(2, [Edit::new(1..6, "joined")]),
    ] {
        l.prepare_snapshot(
            s.snapshot(),
            Projection::new(),
            options(),
            sys.clone(),
            None,
            Default::default(),
        )
        .unwrap();
        s.transact(tx).unwrap();
        l.prepare_snapshot(
            s.snapshot(),
            Projection::new(),
            options(),
            sys.clone(),
            None,
            Default::default(),
        )
        .unwrap();
        let fresh = prepare(&s, Projection::new());
        assert_eq!(l.size(), fresh.size());
        assert_eq!(l.paragraph_count(), fresh.paragraph_count());
        for p in s
            .text()
            .char_indices()
            .map(|(i, _)| i)
            .chain([s.document().len()])
        {
            assert_eq!(
                l.caret(p, Bias::After, 240.0, TextAlign::Left),
                fresh.caret(p, Bias::After, 240.0, TextAlign::Left)
            );
        }
    }
    while s.can_undo() {
        s.undo().unwrap();
        l.prepare_snapshot(
            s.snapshot(),
            Projection::new(),
            options(),
            sys.clone(),
            None,
            Default::default(),
        )
        .unwrap();
        assert_eq!(l.size(), prepare(&s, Projection::new()).size());
    }
}

#[test]
fn embedded_views_unmount_offscreen_and_remount_from_cached_geometry() {
    let s = EditorState::new(format!("[image]\n{}", "row\n".repeat(1000)));
    let mounted = Rc::new(Cell::new(0));
    let unmounted = Rc::new(Cell::new(0));
    let (m, u) = (mounted.clone(), unmounted.clone());
    let mut l = EditorLayout::default();
    l.configure_views(
        EditorViews::new().register(ViewId(1), move || {
            Box::new(BoxView {
                mounted: m.clone(),
                unmounted: u.clone(),
            })
        }),
        None,
    );
    l.prepare_snapshot(
        s.snapshot(),
        Projection::new().replace(Replacement::object(ViewId(1), 0..7)),
        options(),
        system(),
        Some(Rect::from_xywh(0.0, 0.0, 240.0, 120.0)),
        Default::default(),
    )
    .unwrap();
    assert_eq!(mounted.get(), 1);
    l.set_viewport(Rect::from_xywh(0.0, 5000.0, 240.0, 120.0))
        .unwrap();
    assert_eq!(unmounted.get(), 1);
    l.set_viewport(Rect::from_xywh(0.0, 0.0, 240.0, 120.0))
        .unwrap();
    assert_eq!(mounted.get(), 2);
    drop(l);
    assert_eq!(unmounted.get(), 2);
}

#[test]
fn deleting_a_table_cell_character_does_not_delete_the_table() {
    let s = EditorState::new("a|b\nc|d");
    let mut l = EditorLayout::default();
    l.configure_views(
        EditorViews::new().block(
            ViewId(1),
            GridBlock {
                rows: vec![vec![0..1, 2..3], vec![4..5, 6..7]],
                column_weights: vec![1.0, 1.0],
                padding: 4.0,
                gap: 1.0,
                rule: None,
            },
        ),
        None,
    );
    l.prepare_snapshot(
        s.snapshot(),
        Projection::new().block(ViewId(1), 0..7),
        options(),
        system(),
        None,
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        l.deletion_range(
            Selection::caret(1),
            false,
            false,
            240.0,
            TextAlign::Left,
            100.0
        ),
        0..1
    );
    assert_eq!(l.next_cell(1, false).unwrap().head, 2);
}

#[test]
fn custom_object_baseline_is_respected_with_mixed_text_heights() {
    struct Baseline;
    impl EmbeddedView for Baseline {
        fn measure(&mut self, _: f32, _: &TextLayoutCache) -> render::Result<ViewMetrics> {
            Ok(ViewMetrics::new(30.0, 60.0).baseline(20.0))
        }
        fn paint(&mut self, _: &mut render::Painter<'_>, _: Rect<f32>) -> render::Result<()> {
            Ok(())
        }
    }
    let mut s = EditorState::new("a [box] b");
    s.transact(
        Transaction::new(0, []).format(0..1, StylePatch::new().font_size(24.0).line_height(36.0)),
    )
    .unwrap();
    let mut l = EditorLayout::default();
    l.configure_views(
        EditorViews::new().register(ViewId(1), || Box::new(Baseline)),
        None,
    );
    l.prepare_snapshot(
        s.snapshot(),
        Projection::new().replace(Replacement::object(ViewId(1), 2..7)),
        options(),
        system(),
        None,
        Default::default(),
    )
    .unwrap();
    assert!(l.size().height >= 60.0);
    let a = l.caret(2, Bias::Before, 240.0, TextAlign::Left).unwrap();
    assert_eq!(a.size.height, 60.0);
    let z = l.caret(7, Bias::After, 240.0, TextAlign::Left).unwrap();
    assert!((z.origin.x - a.origin.x - 30.0).abs() < 0.1);
}
