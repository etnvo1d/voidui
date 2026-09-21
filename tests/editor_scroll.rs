//! Scroll anchors and measured heights survive projection updates and cache eviction.
#![cfg(feature = "editing")]
use std::{borrow::Cow, cell::Cell, ops::Range, rc::Rc, sync::Arc};
use voidui::{
    core::geometry::Rect,
    editing::*,
    render::{self, ParleyTextSystem, TextAlign, TextLayoutCache, TextSystem},
};

#[derive(Clone, PartialEq)]
struct BoxDescription {
    height: f32,
    measures: Rc<Cell<usize>>,
}
struct BoxView(BoxDescription);
impl ViewDescription for BoxDescription {
    type View = BoxView;
    fn create(&self) -> BoxView {
        BoxView(self.clone())
    }
}
impl EmbeddedView for BoxView {
    fn measure(&mut self, _: f32, _: &TextLayoutCache) -> render::Result<ViewMetrics> {
        self.0.measures.set(self.0.measures.get() + 1);
        Ok(ViewMetrics::new(40.0, self.0.height))
    }
    fn paint(&mut self, _: &mut render::Painter<'_>, _: Rect<f32>) -> render::Result<()> {
        Ok(())
    }
}
struct Harness {
    layout: EditorLayout,
    system: Arc<TextSystem>,
    options: LayoutOptions,
    virtual_options: ViewportOptions,
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
            options: LayoutOptions {
                font: render::font("IBM Plex Sans"),
                font_size: 16.0,
                line_height: 24.0,
                width: Some(300.0),
            },
            virtual_options: ViewportOptions::default(),
        }
    }
    fn viewport(&self, y: f32) -> Rect<f32> {
        Rect::from_xywh(0.0, y, self.options.width.unwrap(), 180.0)
    }
    fn prepare(&mut self, state: &EditorState, plan: Projection, y: Option<f32>) -> f32 {
        self.layout
            .prepare_snapshot(
                state.snapshot(),
                plan,
                self.options.clone(),
                self.system.clone(),
                y.map(|y| self.viewport(y)),
                self.virtual_options,
            )
            .unwrap()
    }
    fn top(&self, pos: usize) -> f32 {
        self.layout
            .caret(
                pos,
                Bias::After,
                self.options.width.unwrap(),
                TextAlign::Left,
            )
            .unwrap()
            .origin
            .y
    }
}
fn close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.02,
        "expected {expected}, got {actual}"
    );
}
struct Fixture {
    state: EditorState,
    widget: Range<usize>,
    target: usize,
    measures: Rc<Cell<usize>>,
}
impl Fixture {
    fn new() -> Self {
        let source = format!(
            "{}widget\n{}**target**\n{}",
            "prefix\n".repeat(30),
            "gap\n".repeat(13),
            "tail\n".repeat(40)
        );
        let start = source.find("widget").unwrap();
        let target = source.find("**target").unwrap();
        Self {
            state: EditorState::new(source),
            widget: start..start + "widget\n".len(),
            target,
            measures: Rc::default(),
        }
    }
    fn plan(&self, height: f32, hidden: bool) -> Projection {
        let plan = Projection::new().block_view(BlockView::widget(
            self.widget.clone(),
            BoxDescription {
                height,
                measures: self.measures.clone(),
            },
        ));
        if hidden {
            plan.replace(Replacement::hide(ViewId(100), self.target..self.target + 2))
        } else {
            plan
        }
    }
}

#[test]
fn unrelated_marker_changes_preserve_block_geometry_and_scroll() {
    let f = Fixture::new();
    let mut h = Harness::new();
    h.prepare(&f.state, f.plan(80.0, true), None);
    let mut y = h.top(f.target) - 120.0;
    y = h.layout.set_viewport(h.viewport(y)).unwrap();
    let before_y = y;
    let height = h.layout.size().height;
    let measured = f.measures.get();
    for i in 0..20 {
        y = h.prepare(&f.state, f.plan(80.0, i % 2 != 0), Some(y));
        close(y, before_y);
        close(h.top(f.target) - y, 120.0);
        close(h.layout.size().height, height);
        assert_eq!(
            f.measures.get(),
            measured,
            "unrelated markers must not remeasure the block"
        );
    }
}

#[test]
fn changed_block_height_preserves_the_old_source_anchor() {
    let f = Fixture::new();
    let mut h = Harness::new();
    h.prepare(&f.state, f.plan(80.0, true), None);
    let mut y = h
        .layout
        .set_viewport(h.viewport(h.top(f.target) - 120.0))
        .unwrap();
    let initial = y;
    for height in [120.0, 48.0, 80.0] {
        y = h.prepare(&f.state, f.plan(height, true), Some(y));
        close(y, initial + height - 80.0);
        close(h.top(f.target) - y, 120.0);
    }
}

#[test]
fn measured_heights_survive_eviction_and_selection_updates() {
    let f = Fixture::new();
    let mut h = Harness::new();
    h.virtual_options.overscan = 0.0;
    h.virtual_options.max_cached_blocks = 8;
    h.prepare(&f.state, f.plan(80.0, true), None);
    let mut y = h
        .layout
        .set_viewport(h.viewport(h.top(f.target) - 48.0))
        .unwrap();
    let total = h.layout.size().height;
    let measured = f.measures.get();
    for i in 0..10 {
        y = h.prepare(&f.state, f.plan(80.0, i % 2 != 0), Some(y));
        close(h.layout.size().height, total);
        close(h.top(f.target) - y, 48.0);
        assert_eq!(
            f.measures.get(),
            measured,
            "offscreen content must stay unmounted"
        );
        assert!(h.layout.stats().cached_blocks <= 8);
    }
}

#[test]
fn edits_above_the_viewport_map_the_anchor_and_retained_heights() {
    let mut f = Fixture::new();
    let mut h = Harness::new();
    h.prepare(&f.state, f.plan(80.0, true), None);
    let mut y = h
        .layout
        .set_viewport(h.viewport(h.top(f.target) - 48.0))
        .unwrap();
    let before = y;
    let inserted = "new line\n";
    f.state
        .transact(Transaction::new(
            f.state.revision(),
            [Edit::new(0..0, inserted)],
        ))
        .unwrap();
    f.widget = f.widget.start + inserted.len()..f.widget.end + inserted.len();
    f.target += inserted.len();
    y = h.prepare(&f.state, f.plan(80.0, true), Some(y));
    close(y, before + 24.0);
    close(h.top(f.target) - y, 48.0);
}

#[test]
fn folding_above_the_viewport_keeps_the_same_source_on_screen() {
    let f = Fixture::new();
    let mut h = Harness::new();
    h.prepare(&f.state, f.plan(80.0, true), None);
    let mut y = h
        .layout
        .set_viewport(h.viewport(h.top(f.target) - 48.0))
        .unwrap();
    let initial = y;
    for folded in [true, false, true, false] {
        let mut plan = f.plan(80.0, true);
        if folded {
            plan = plan.replace(Replacement::hide(ViewId(101), 0.."prefix\n".len() * 10));
        }
        y = h.prepare(&f.state, plan, Some(y));
        close(h.top(f.target) - y, 48.0);
        if !folded {
            close(y, initial);
        }
    }
}

#[test]
fn bottom_anchor_tracks_the_end_after_appending_text() {
    let mut state = EditorState::new("line\n".repeat(100));
    let mut h = Harness::new();
    h.prepare(&state, Projection::default(), None);
    let y = h.layout.size().height - h.viewport(0.0).size.height;
    h.layout.set_viewport(h.viewport(y)).unwrap();
    let end = state.document().len();
    state
        .transact(Transaction::new(
            state.revision(),
            [Edit::new(end..end, "extra\n")],
        ))
        .unwrap();
    let y = h.prepare(&state, Projection::default(), Some(y));
    close(y, h.layout.size().height - h.viewport(0.0).size.height);
}

#[test]
fn width_reflow_returns_the_scroll_offset_for_the_same_source_anchor() {
    let line = "several words that wrap when the viewport becomes narrower\n";
    let state = EditorState::new(line.repeat(80));
    let mut h = Harness::new();
    h.prepare(&state, Projection::default(), None);
    let position = line.len() * 40;
    let mut y = h
        .layout
        .set_viewport(h.viewport(h.top(position) + 7.0))
        .unwrap();
    let screen_y = h.top(position) - y;
    let shaped = h.system.stats().paragraphs_shaped;
    for width in [160.0, 400.0, 300.0] {
        h.options.width = Some(width);
        y = h.layout.reflow(Some(width));
        close(h.top(position) - y, screen_y);
        close(h.layout.set_viewport(h.viewport(y)).unwrap(), y);
    }
    assert_eq!(
        h.system.stats().paragraphs_shaped,
        shaped,
        "width changes only rebreak retained glyphs"
    );
}

#[test]
fn explicit_scrolling_replaces_the_remembered_anchor() {
    let f = Fixture::new();
    let mut h = Harness::new();
    h.prepare(&f.state, f.plan(80.0, true), None);
    let initial = h
        .layout
        .set_viewport(h.viewport(h.top(f.target) - 120.0))
        .unwrap();
    let requested = initial + 48.0;
    let y = h.layout.set_viewport(h.viewport(requested)).unwrap();
    close(y, requested);
    close(h.prepare(&f.state, f.plan(80.0, false), Some(y)), requested);
}

#[test]
fn caret_measurements_between_frames_do_not_lose_the_displayed_anchor() {
    let f = Fixture::new();
    let mut h = Harness::new();
    h.virtual_options.overscan = 0.0;
    // Begin far below the widget, which has not yet been measured.
    let y = h.prepare(&f.state, f.plan(80.0, true), Some(1100.0));
    let before = h.top(f.target) - y;
    h.top(f.widget.start);
    let next = h.layout.set_viewport(h.viewport(y)).unwrap();
    close(next, y + 80.0 - h.options.line_height);
    close(h.top(f.target) - next, before);
    close(h.layout.set_viewport(h.viewport(next)).unwrap(), next);
}

#[test]
fn failed_measurement_keeps_the_previous_anchor_and_height_knowledge() {
    let f = Fixture::new();
    let mut h = Harness::new();
    h.prepare(&f.state, f.plan(80.0, true), None);
    let y = h
        .layout
        .set_viewport(h.viewport(h.top(f.target) - 120.0))
        .unwrap();
    let size = h.layout.size();
    assert!(
        h.layout
            .prepare_snapshot(
                f.state.snapshot(),
                f.plan(0.0, true),
                h.options.clone(),
                h.system.clone(),
                Some(h.viewport(y)),
                h.virtual_options,
            )
            .is_err()
    );
    assert_eq!(h.layout.size(), size);
    close(h.prepare(&f.state, f.plan(80.0, false), Some(y)), y);
    close(h.top(f.target) - y, 120.0);
}

#[test]
fn evicted_wrapped_text_keeps_its_measured_height() {
    let line = "words with wide letters WWW and many more words to wrap\n";
    let state = EditorState::new(line.repeat(100));
    let mut h = Harness::new();
    h.virtual_options.overscan = 0.0;
    h.virtual_options.max_cached_blocks = 8;
    h.options.width = Some(100.0);
    h.prepare(&state, Projection::default(), None);
    let position = line.len() * 50;
    let mut y = h
        .layout
        .set_viewport(h.viewport(h.top(position) + 7.0))
        .unwrap();
    let height = h.layout.size().height;
    let shaped = h.system.stats().paragraphs_shaped;
    for _ in 0..10 {
        y = h.prepare(&state, Projection::default(), Some(y));
        close(h.layout.size().height, height);
        close(h.top(position) - y, -7.0);
    }
    assert_eq!(h.system.stats().paragraphs_shaped, shaped);
}

#[test]
fn pending_manual_scroll_is_respected_when_the_control_resizes() {
    use voidui::{
        core::{
            geometry::Point,
            layout::{AvailableSpace, Size},
            widget_tree::WidgetTree,
        },
        rich_editor,
        style::css::Stylesheet,
    };
    let h = Harness::new();
    let cache = TextLayoutCache::new(h.system.clone());
    let editor = Editor::new("line\n".repeat(100));
    let mut tree = WidgetTree::new();
    tree.build_root(rich_editor(&editor).id("editor"));
    let layout = Size {
        width: AvailableSpace::Definite(400.0),
        height: AvailableSpace::Definite(300.0),
    };
    for width in [300.0, 200.0] {
        tree.set_stylesheets(vec![Stylesheet::parse(&format!(
            "#editor{{width:{width}px;height:180px;font-family:'IBM Plex Sans';font-size:16px;line-height:24px}}"
        )).unwrap()]);
        tree.layout(layout, &cache);
        tree.refresh_scroll_content(&cache);
        let edit = tree.find_by_id("editor").unwrap();
        if width == 300.0 {
            // Do not run another frame before resizing: the control's scroll
            // offset is newer than its last measured viewport.
            assert!(tree.scroll_to(edit, Point::new(0.0, 900.0)));
        } else {
            close(tree.scroll_metrics(edit).unwrap().offset.y, 900.0);
        }
    }
}
