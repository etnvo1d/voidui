//! Described widgets exercise reconciliation without a view registry or window.
#![cfg(feature = "editing")]
use std::{borrow::Cow, cell::RefCell, rc::Rc, sync::Arc, time::Instant};
use voidui::{
    core::{
        geometry::{Point, Rect},
        input::{InputContext, InputEvent, PointerPhase},
    },
    editing::*,
    render::{self, ParleyTextSystem, TextAlign, TextLayoutCache, TextSystem},
};

#[derive(Debug, Clone, PartialEq)]
enum Event {
    Create(u32),
    Mount(u32),
    Update(u32),
    Measure(u32),
    Input(u32),
    Cancel(u32),
    Unmount(u32),
}
type Log = Rc<RefCell<Vec<Event>>>;
#[derive(Clone)]
struct Description {
    value: u32,
    metrics: ViewMetrics,
    update: bool,
    log: Log,
}
impl PartialEq for Description {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
            && self.metrics == other.metrics
            && self.update == other.update
            && Rc::ptr_eq(&self.log, &other.log)
    }
}
impl Description {
    fn new(value: u32, log: &Log) -> Self {
        Self {
            value,
            metrics: ViewMetrics::new(40.0, 40.0),
            update: true,
            log: log.clone(),
        }
    }
}
struct Probe(Description, bool);
impl ViewDescription for Description {
    type View = Probe;
    fn create(&self) -> Probe {
        self.log.borrow_mut().push(Event::Create(self.value));
        Probe(self.clone(), false)
    }
    fn update(&self, view: &mut Probe) -> bool {
        if !self.update {
            return false;
        }
        self.log.borrow_mut().push(Event::Update(self.value));
        view.0 = self.clone();
        true
    }
}
impl EmbeddedView for Probe {
    fn has_focus(&self) -> bool {
        self.1
    }
    fn blur(&mut self) {
        self.1 = false;
    }
    fn cancel_pointer(&mut self) {
        self.0.log.borrow_mut().push(Event::Cancel(self.0.value));
    }
    fn measure(&mut self, _: f32, _: &TextLayoutCache) -> render::Result<ViewMetrics> {
        self.0.log.borrow_mut().push(Event::Measure(self.0.value));
        Ok(self.0.metrics)
    }
    fn paint(&mut self, _: &mut render::Painter<'_>, _: Rect<f32>) -> render::Result<()> {
        Ok(())
    }
    fn mounted(&mut self, _: Option<voidui::core::updates::WidgetInvalidator>) {
        self.0.log.borrow_mut().push(Event::Mount(self.0.value));
    }
    fn unmounted(&mut self) {
        self.0.log.borrow_mut().push(Event::Unmount(self.0.value));
    }
    fn input(&mut self, _: &InputEvent, _: InputContext<'_>, _: &Editor) -> bool {
        self.1 = true;
        self.0.log.borrow_mut().push(Event::Input(self.0.value));
        true
    }
}
struct Harness {
    layout: EditorLayout,
    system: Arc<TextSystem>,
}
impl Harness {
    fn new() -> Self {
        let fonts = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
        fonts
            .add_fonts(vec![Cow::Borrowed(include_bytes!(
                "../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
            ))])
            .unwrap();
        Self {
            layout: EditorLayout::default(),
            system: Arc::new(TextSystem::new(Arc::new(fonts))),
        }
    }
    fn prepare(
        &mut self,
        state: &EditorState,
        projection: Projection,
        viewport: Option<Rect<f32>>,
    ) -> render::Result<f32> {
        self.layout.prepare_snapshot(
            state.snapshot(),
            projection,
            LayoutOptions {
                font: render::font("IBM Plex Sans"),
                font_size: 16.0,
                line_height: 24.0,
                width: Some(240.0),
            },
            self.system.clone(),
            viewport,
            ViewportOptions {
                overscan: 0.0,
                ..Default::default()
            },
        )
    }
    fn object(&self, x: f32, y: f32) -> (ViewId, std::ops::Range<usize>, Rect<f32>) {
        self.layout
            .object_at(Point::new(x, y), 240.0, TextAlign::Left)
            .unwrap()
    }
    fn focus(&self, id: ViewId) {
        assert!(self.layout.dispatch_view(
            id,
            &InputEvent::Pointer {
                phase: PointerPhase::Down,
                position: Point::new(5.0, 20.0),
                modifiers: Default::default(),
                clicks: 1,
            },
            InputContext {
                text_layout: &TextLayoutCache::new(self.system.clone()),
                bounds: Rect::from_xywh(0.0, 0.0, 240.0, 100.0),
                style: &Default::default(),
                readonly: false,
                now: Instant::now(),
            },
            &Editor::new("")
        ));
    }
}
fn projection(value: u32, log: &Log) -> Projection {
    Projection::new().replace(Replacement::widget(0..1, Description::new(value, log)))
}
fn count(log: &Log, event: Event) -> usize {
    log.borrow().iter().filter(|e| **e == event).count()
}

#[test]
fn fresh_equal_descriptions_reuse_instances_and_do_not_remeasure() {
    let mut state = EditorState::new("ab");
    let log = Log::default();
    let plan = || projection(1, &log).replace(Replacement::widget(1..2, Description::new(2, &log)));
    assert!(state.set_projection(0, plan()).unwrap());
    let generation = state.projection_revision();
    assert!(!state.set_projection(0, plan()).unwrap());
    assert_eq!(generation, state.projection_revision());
    let mut h = Harness::new();
    h.prepare(&state, plan(), None).unwrap();
    let before = log.borrow().clone();
    let shapes = h.system.stats().paragraphs_shaped;
    h.prepare(&state, plan(), None).unwrap();
    assert_eq!(*log.borrow(), before);
    assert_eq!(h.system.stats().paragraphs_shaped, shapes);
    assert_eq!(h.layout.stats().mounted_views, 2);
    assert_ne!(h.object(5.0, 20.0).0, h.object(45.0, 20.0).0);
}

#[test]
fn changed_props_update_focused_instance_and_invalidate_geometry() {
    let state = EditorState::new("a tail");
    let log = Log::default();
    let mut h = Harness::new();
    h.prepare(&state, projection(1, &log), None).unwrap();
    let id = h.object(5.0, 20.0).0;
    h.focus(id);
    let mut next = Description::new(2, &log);
    next.metrics = ViewMetrics::new(100.0, 60.0).baseline(20.0);
    let plan = || Projection::new().replace(Replacement::widget(0..1, next.clone()));
    h.prepare(&state, plan(), None).unwrap();
    assert_eq!(h.layout.focused_view(), Some(id));
    assert_eq!(h.object(5.0, 20.0).2.size.width, 100.0);
    assert_eq!(count(&log, Event::Update(2)), 1);
    assert_eq!(count(&log, Event::Create(2)), 0);
    assert_eq!(count(&log, Event::Unmount(1)), 0);
    h.focus(id);
    assert_eq!(log.borrow().last(), Some(&Event::Input(2)));
    let mut fresh = Harness::new();
    fresh.prepare(&state, plan(), None).unwrap();
    assert_eq!(h.layout.size(), fresh.layout.size());
    assert_eq!(
        h.layout.caret(1, Bias::After, 240.0, TextAlign::Left),
        fresh.layout.caret(1, Bias::After, 240.0, TextAlign::Left)
    );
}

#[test]
fn baseline_only_changes_relayout_without_replacing_the_instance() {
    let state = EditorState::new("a tail");
    let log = Log::default();
    let mut h = Harness::new();
    h.prepare(&state, projection(1, &log), None).unwrap();
    let old_caret = h.layout.caret(2, Bias::After, 240.0, TextAlign::Left);
    let mut next = Description::new(1, &log);
    next.metrics.baseline = 5.0;
    h.prepare(
        &state,
        Projection::new().replace(Replacement::widget(0..1, next)),
        None,
    )
    .unwrap();
    assert_ne!(
        h.layout.caret(2, Bias::After, 240.0, TextAlign::Left),
        old_caret
    );
    assert_eq!(count(&log, Event::Create(1)), 1);
    assert_eq!(count(&log, Event::Update(1)), 1);
}

#[test]
fn default_replacement_and_description_type_changes_release_old_views() {
    #[derive(PartialEq)]
    struct Other(Description);
    impl ViewDescription for Other {
        type View = Probe;
        fn create(&self) -> Probe {
            self.0.create()
        }
    }
    let state = EditorState::new("a");
    let log = Log::default();
    let mut h = Harness::new();
    let keyed = |d| Projection::new().replace(Replacement::widget(0..1, d).key("formula"));
    h.prepare(&state, keyed(Description::new(1, &log)), None)
        .unwrap();
    h.focus(h.object(5.0, 20.0).0);
    let mut next = Description::new(2, &log);
    next.update = false;
    h.prepare(&state, keyed(next), None).unwrap();
    assert_eq!(h.layout.focused_view(), None);
    assert_eq!(count(&log, Event::Unmount(1)), 1);
    h.prepare(
        &state,
        Projection::new()
            .replace(Replacement::widget(0..1, Other(Description::new(3, &log))).key("formula")),
        None,
    )
    .unwrap();
    assert_eq!(count(&log, Event::Unmount(2)), 1);
    assert_eq!(count(&log, Event::Create(3)), 1);
    assert_eq!(count(&log, Event::Update(3)), 0);
}

#[test]
fn source_mapping_preserves_existing_occurrences_when_a_new_one_is_inserted() {
    let mut state = EditorState::new("a");
    let log = Log::default();
    let mut h = Harness::new();
    h.prepare(&state, projection(1, &log), None).unwrap();
    let original = h.object(5.0, 20.0).0;
    state
        .transact(Transaction::new(state.revision(), [Edit::new(0..0, "b")]))
        .unwrap();
    let plan = projection(2, &log).replace(Replacement::widget(1..2, Description::new(1, &log)));
    h.prepare(&state, plan, None).unwrap();
    assert_ne!(h.object(5.0, 20.0).0, original);
    assert_eq!(h.object(45.0, 20.0).0, original);
    assert_eq!(count(&log, Event::Create(1)), 1);
    assert_eq!(count(&log, Event::Create(2)), 1);
}

#[test]
fn keys_follow_moves_but_never_cross_documents() {
    let state = EditorState::new("ab");
    let log = Log::default();
    let mut h = Harness::new();
    let plan = |swapped| {
        let (a, b) = if swapped { ("b", "a") } else { ("a", "b") };
        Projection::new()
            .replace(Replacement::widget(0..1, Description::new(1, &log)).key(a))
            .replace(Replacement::widget(1..2, Description::new(1, &log)).key(b))
    };
    h.prepare(&state, plan(false), None).unwrap();
    let a = h.object(5.0, 20.0).0;
    let b = h.object(45.0, 20.0).0;
    h.focus(a);
    h.prepare(&state, plan(true), None).unwrap();
    assert_eq!(h.object(5.0, 20.0).0, b);
    assert_eq!(h.object(45.0, 20.0).0, a);
    assert_eq!(h.layout.focused_view(), Some(a));
    assert_eq!(count(&log, Event::Create(1)), 2);
    h.prepare(&EditorState::new("ab"), plan(true), None)
        .unwrap();
    assert_eq!(h.layout.focused_view(), None);
    assert_eq!(count(&log, Event::Create(1)), 4);
    assert_eq!(count(&log, Event::Unmount(1)), 2);
}

#[test]
fn runtime_handles_do_not_alias_explicit_ids_or_removed_widgets() {
    let state = EditorState::new("ab");
    let log = Log::default();
    let mut h = Harness::new();
    h.prepare(&state, projection(1, &log), None).unwrap();
    let previous_id = h.object(5.0, 20.0).0;
    let d = Description::new(2, &log);
    h.layout.configure_views(
        EditorViews::new().register(previous_id, move || Box::new(d.create())),
        None,
    );
    let plan = projection(1, &log).replace(Replacement::object(previous_id, 1..2));
    h.prepare(&state, plan, None).unwrap();
    assert_ne!(h.object(5.0, 20.0).0, previous_id);
    assert_eq!(h.object(45.0, 20.0).0, previous_id);
    assert_eq!(count(&log, Event::Create(2)), 1);
    assert_eq!(count(&log, Event::Unmount(1)), 1);
}

#[test]
fn offscreen_descriptions_stay_lazy_and_remount_with_latest_props() {
    let state = EditorState::new("x\n".repeat(1000));
    let log = Log::default();
    let mut h = Harness::new();
    let plan = |value| {
        let mut p = Projection::new();
        for i in 0..1000 {
            p = p.replace(Replacement::widget(
                i * 2..i * 2 + 1,
                Description::new(value, &log),
            ));
        }
        p
    };
    let top = Rect::from_xywh(0.0, 0.0, 240.0, 80.0);
    h.prepare(&state, plan(1), Some(top)).unwrap();
    assert!(h.layout.stats().mounted_views < 10);
    h.layout
        .set_viewport(Rect::from_xywh(0.0, 10000.0, 240.0, 80.0))
        .unwrap();
    assert!(count(&log, Event::Unmount(1)) > 0);
    h.prepare(&state, plan(2), Some(top)).unwrap();
    assert!(count(&log, Event::Create(2)) > 0);
    assert_eq!(h.object(5.0, 20.0).1, 0..1);
    h.prepare(&state, Projection::new(), Some(top)).unwrap();
    assert_eq!(h.layout.stats().mounted_views, 0);
}

#[test]
fn described_blocks_and_inline_views_in_source_backed_cells_need_no_registry() {
    let state = EditorState::new("a\nb");
    let log = Log::default();
    let mut h = Harness::new();
    let grid = GridBlock {
        rows: vec![vec![2..3]],
        column_weights: vec![1.0],
        padding: 0.0,
        gap: 0.0,
        rule: None,
    };
    // A registered layout with the same provisional handle must not capture a
    // self-contained block. A second grid exercises descriptions inside cells.
    h.layout.configure_views(
        EditorViews::new()
            .block(ViewId(0), grid.clone())
            .block(ViewId(10), grid),
        None,
    );
    let plan = Projection::new()
        .block_view(BlockView::widget(0..2, Description::new(1, &log)))
        .block(ViewId(10), 2..3)
        .replace(Replacement::widget(2..3, Description::new(2, &log)));
    h.prepare(&state, plan, None).unwrap();
    assert_eq!(count(&log, Event::Mount(1)), 1);
    assert_eq!(count(&log, Event::Mount(2)), 1);
    assert_eq!(h.layout.stats().mounted_views, 2);
}

#[test]
fn invalid_descriptions_restore_previous_inputs_and_layout() {
    for update in [true, false] {
        let state = EditorState::new("a");
        let log = Log::default();
        let mut h = Harness::new();
        let mut original = Description::new(1, &log);
        original.update = update;
        h.prepare(
            &state,
            Projection::new().replace(Replacement::widget(0..1, original)),
            None,
        )
        .unwrap();
        let bounds = h.object(5.0, 20.0).2;
        let mut invalid = Description::new(2, &log);
        invalid.metrics.size.height = 0.0;
        invalid.update = update;
        assert!(
            h.prepare(
                &state,
                Projection::new().replace(Replacement::widget(0..1, invalid)),
                None
            )
            .is_err()
        );
        let restored = h.object(5.0, 20.0);
        assert_eq!(restored.2, bounds);
        h.focus(restored.0);
        assert_eq!(log.borrow().last(), Some(&Event::Input(1)));
    }
}

#[test]
fn duplicate_keys_are_rejected_and_reveal_removes_focused_instances() {
    let state = EditorState::new("ab");
    let log = Log::default();
    let mut bad = Projection::new()
        .replace(Replacement::widget(0..1, Description::new(1, &log)).key("same"))
        .replace(Replacement::widget(1..2, Description::new(2, &log)).key("same"));
    assert!(bad.validate(state.document()).is_err());
    assert!(log.borrow().is_empty());
    let plan = Projection::new()
        .replace(Replacement::widget(0..1, Description::new(1, &log)).reveal_on_selection());
    let mut h = Harness::new();
    h.prepare(&state, plan.clone(), None).unwrap();
    h.focus(h.object(5.0, 20.0).0);
    let revealed = plan.active(&SelectionSet::single(Selection::caret(0)), None);
    h.prepare(&state, revealed, None).unwrap();
    assert_eq!(h.layout.focused_view(), None);
    assert_eq!(h.layout.captured_view(), None);
    assert_eq!(count(&log, Event::Cancel(1)), 1);
    assert_eq!(count(&log, Event::Unmount(1)), 1);
    assert_eq!(h.layout.stats().mounted_views, 0);
}

#[test]
fn dropping_an_editor_cancels_capture_before_unmounting_the_view() {
    let state = EditorState::new("ab");
    let log = Log::default();
    {
        let mut h = Harness::new();
        h.prepare(&state, projection(1, &log), None).unwrap();
        h.focus(h.object(5.0, 20.0).0);
        assert!(h.layout.captured_view().is_some());
    }
    let events = log.borrow();
    let cancelled = events
        .iter()
        .position(|event| *event == Event::Cancel(1))
        .unwrap();
    let unmounted = events
        .iter()
        .position(|event| *event == Event::Unmount(1))
        .unwrap();
    assert!(cancelled < unmounted);
}

#[test]
fn width_changes_remeasure_inline_and_block_widgets() {
    #[derive(PartialEq)]
    struct Responsive;
    struct ResponsiveView;
    impl ViewDescription for Responsive {
        type View = ResponsiveView;
        fn create(&self) -> ResponsiveView {
            ResponsiveView
        }
    }
    impl EmbeddedView for ResponsiveView {
        fn measure(&mut self, width: f32, _: &TextLayoutCache) -> render::Result<ViewMetrics> {
            Ok(ViewMetrics::new(width, 40.0))
        }
        fn paint(&mut self, _: &mut render::Painter<'_>, _: Rect<f32>) -> render::Result<()> {
            Ok(())
        }
    }
    for block in [false, true] {
        let state = EditorState::new("a");
        let plan = || {
            if block {
                Projection::new().block_view(BlockView::widget(0..1, Responsive))
            } else {
                Projection::new().replace(Replacement::widget(0..1, Responsive))
            }
        };
        let mut h = Harness::new();
        h.prepare(&state, plan(), None).unwrap();
        assert_eq!(h.object(5.0, 20.0).2.size.width, 240.0);
        h.layout.reflow(Some(120.0));
        assert_eq!(h.object(5.0, 20.0).2.size.width, 120.0);
        // prepare_snapshot has a separate cache reuse path from direct reflow.
        h.prepare(&state, plan(), None).unwrap();
        assert_eq!(h.object(5.0, 20.0).2.size.width, 240.0);
    }
}

#[test]
fn skipped_revisions_remount_unkeyed_widgets_but_preserve_explicit_keys() {
    for keyed in [false, true] {
        let mut state = EditorState::new("a tail");
        let log = Log::default();
        let mut h = Harness::new();
        let plan = || {
            let replacement = Replacement::widget(0..1, Description::new(1, &log));
            Projection::new().replace(if keyed {
                replacement.key("formula")
            } else {
                replacement
            })
        };
        h.prepare(&state, plan(), None).unwrap();
        for edit in [Edit::new(2..6, "text"), Edit::new(2..6, "tail")] {
            state
                .transact(Transaction::new(state.revision(), [edit]))
                .unwrap();
        }
        h.prepare(&state, plan(), None).unwrap();
        assert_eq!(count(&log, Event::Create(1)), if keyed { 1 } else { 2 });
    }
}

#[test]
fn described_widget_alignment_changes_invalidate_retained_geometry() {
    use voidui::{IntoElement, div};
    fn render_box(_: &()) -> voidui::Element {
        div().width(18.0).height(14.0).into_element()
    }
    let state = EditorState::new("[box] a");
    let mut harness = Harness::new();
    let projection = |align| {
        Projection::new().replace(Replacement::widget(
            0..5,
            WidgetView::describe((), render_box).inline_align(align),
        ))
    };
    harness
        .prepare(&state, projection(render::InlineAlignment::Baseline), None)
        .unwrap();
    let (_, _, before) = harness.object(5.0, 12.0);
    harness
        .prepare(&state, projection(render::InlineAlignment::Middle), None)
        .unwrap();
    let (_, _, after) = harness.object(5.0, 12.0);
    assert_eq!(before.size, after.size);
    assert!((before.origin.y - after.origin.y).abs() > 0.01);
    harness
        .prepare(&state, projection(render::InlineAlignment::Baseline), None)
        .unwrap();
    assert_eq!(harness.object(5.0, 12.0).2, before);
    assert_eq!(state.text(), "[box] a");
    assert_eq!(state.selections().primary(), Selection::caret(0));
    assert!(!state.can_undo());
}

#[test]
fn relative_widget_offsets_move_hit_bounds_but_not_text_or_line_height() {
    use voidui::{IntoElement, div};
    fn render_box(_: &()) -> voidui::Element {
        div().width(18.0).height(14.0).into_element()
    }
    // The second row allows the translated box to extend into an earlier row.
    let state = EditorState::new("before\n[box] a");
    let mut harness = Harness::new();
    let projection = |offset| {
        Projection::new().replace(Replacement::widget(
            7..12,
            WidgetView::describe((), render_box)
                .inline_align(render::InlineAlignment::Middle)
                .inline_offset_em(offset),
        ))
    };
    harness.prepare(&state, projection(0.0), None).unwrap();
    let (id, _, before) = harness.object(5.0, 36.0);
    let caret = harness
        .layout
        .caret(13, Bias::After, 240.0, TextAlign::Left);
    let height = harness.layout.size().height;
    for offset in [-0.1, -1.0, 0.0] {
        harness.prepare(&state, projection(offset), None).unwrap();
        let after = harness
            .layout
            .view_bounds(id, 240.0, TextAlign::Left)
            .unwrap();
        assert!((after.origin.y - before.origin.y - offset * 16.0).abs() < 0.001);
        assert_eq!(after.size, before.size);
        assert_eq!(harness.layout.size().height, height);
        assert_eq!(
            harness
                .layout
                .caret(13, Bias::After, 240.0, TextAlign::Left),
            caret
        );
        assert_eq!(
            harness
                .object(after.origin.x + 5.0, after.origin.y + 0.25)
                .0,
            id
        );
        assert!(
            harness
                .layout
                .object_at(
                    Point::new(
                        after.origin.x + 5.0,
                        after.origin.y + after.size.height + 0.25
                    ),
                    240.0,
                    TextAlign::Left,
                )
                .is_none()
        );
    }
}

#[test]
fn source_backed_decorations_share_geometry_mounting_capture_and_reflow() {
    struct Decorated {
        log: Log,
    }
    impl BlockLayout for Decorated {
        fn layout(
            &self,
            source: std::ops::Range<usize>,
            m: &mut dyn BlockMeasure,
        ) -> render::Result<BlockArrangement> {
            let value = m.source().read(source.clone()).unwrap().as_bytes()[0] as u32;
            let width = m.available_width();
            let size = m.measure_text(source.clone(), width - 20.0)?;
            let mut description = Description::new(value, &self.log);
            if value == b'!' as u32 {
                description.metrics.size.height = 0.0;
            }
            let arrangement = BlockArrangement {
                size: voidui::core::geometry::Size::new(width, size.height + 20.0),
                cells: vec![TextCell {
                    source,
                    bounds: Rect::from_xywh(10.0, 10.0, width - 20.0, size.height),
                }],
                ..Default::default()
            };
            Ok(if value == b'-' as u32 {
                arrangement
            } else {
                arrangement.with_decoration(description)
            })
        }
    }
    let log = Log::default();
    let mut h = Harness::new();
    let id = ViewId(70);
    h.layout.configure_views(
        EditorViews::new().block(id, Decorated { log: log.clone() }),
        None,
    );
    let mut state = EditorState::new("cell");
    let plan = || Projection::new().block(id, 0..4);
    h.prepare(&state, plan(), None).unwrap();
    let original = h.layout.decoration_at(Point::new(4.0, 4.0)).unwrap();
    assert_eq!(original.0, id);
    // Source-backed decoration hit bounds must not turn text into an atomic object.
    assert!(
        h.layout
            .object_at(Point::new(15.0, 15.0), 240.0, TextAlign::Left)
            .is_none()
    );
    assert_eq!(
        h.layout.view_bounds(id, 240.0, TextAlign::Left),
        Some(original.1)
    );
    // Visible decorations must survive viewport refresh even without focus or
    // pointer capture; their retained scroll offsets live in the mounted view.
    h.layout
        .set_viewport(Rect::from_xywh(0.0, 0.0, 240.0, 120.0))
        .unwrap();
    h.layout
        .set_viewport(Rect::from_xywh(0.0, 0.0, 240.0, 120.0))
        .unwrap();
    assert_eq!(count(&log, Event::Mount(b'c' as u32)), 1);
    assert_eq!(count(&log, Event::Unmount(b'c' as u32)), 0);
    h.focus(id);
    assert_eq!(h.layout.captured_view(), Some(id));
    h.prepare(&state, plan(), None).unwrap();
    assert_eq!(count(&log, Event::Mount(b'c' as u32)), 1);
    assert_eq!(h.layout.focused_view(), Some(id));
    h.layout.reflow(Some(180.0));
    assert_eq!(
        h.layout
            .view_bounds(id, 180.0, TextAlign::Left)
            .unwrap()
            .size
            .width,
        180.0
    );
    assert_eq!(h.layout.captured_view(), Some(id));
    state
        .transact(Transaction::new(state.revision(), [Edit::new(0..1, "n")]))
        .unwrap();
    h.prepare(&state, plan(), None).unwrap();
    assert_eq!(count(&log, Event::Update(b'n' as u32)), 1);
    // Failed layout restores the last usable decoration inputs as well as text.
    state
        .transact(Transaction::new(state.revision(), [Edit::new(0..1, "!")]))
        .unwrap();
    assert!(h.prepare(&state, plan(), None).is_err());
    h.focus(id);
    assert_eq!(log.borrow().last(), Some(&Event::Input(b'n' as u32)));
    // Removing only the decoration releases ownership before the next event.
    state
        .transact(Transaction::new(state.revision(), [Edit::new(0..1, "-")]))
        .unwrap();
    h.prepare(&state, plan(), None).unwrap();
    assert!(h.layout.decoration_at(Point::new(4.0, 4.0)).is_none());
    h.prepare(&state, Projection::new(), None).unwrap();
    assert_eq!(h.layout.focused_view(), None);
    assert_eq!(h.layout.captured_view(), None);
    assert_eq!(count(&log, Event::Unmount(b'n' as u32)), 1);
}
