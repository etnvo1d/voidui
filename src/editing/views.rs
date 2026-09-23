//! Embedded content is mounted only while visible. Layout asks for metrics;
//! input and painting use the same retained view, with explicit focus ownership.
use super::{Editor, ViewId, ViewSpec};
use crate::{
    core::{
        event::{EventResponse, MouseEvent},
        geometry::{Point, Rect, Size},
        input::{InputContext, InputEvent, TextInputClient},
        updates::WidgetInvalidator,
    },
    render::{Painter, TextLayoutCache},
    style::selection::Cursor,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewMetrics {
    pub size: Size<f32>,
    /// Distance from the top edge to the shared text baseline.
    pub baseline: f32,
    /// Parent-relative alignment in the surrounding text flow.
    pub align: crate::render::InlineAlignment,
    /// Visual vertical offset in surrounding-font em units; positive moves down.
    pub offset_em: f32,
}
impl ViewMetrics {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            size: Size::new(width, height),
            baseline: height,
            align: Default::default(),
            offset_em: 0.0,
        }
    }
    pub fn baseline(mut self, baseline: f32) -> Self {
        self.baseline = baseline;
        self
    }
    /// Use the surrounding font to align this view in an inline formatting context.
    pub fn align(mut self, align: crate::render::InlineAlignment) -> Self {
        self.align = align;
        self
    }
    /// Move painting and hit bounds without contributing to line height.
    pub fn inline_offset_em(mut self, offset: f32) -> Self {
        self.offset_em = offset;
        self
    }
    pub fn validate(self) -> crate::render::Result<Self> {
        anyhow::ensure!(
            self.size.width.is_finite()
                && self.size.width >= 0.0
                && self.size.height.is_finite()
                && self.size.height > 0.0
                && self.baseline.is_finite()
                && self.offset_em.is_finite(),
            "invalid embedded view metrics"
        );
        Ok(self)
    }
}
/// Block-local clip and retained scroll offset for source-backed cell content.
/// Decorations and their controls stay fixed; text and inline views share this
/// transform for painting, hit testing, selection and IME caret placement.
#[derive(Debug, Clone, Copy)]
pub struct SourceViewport {
    pub bounds: Rect<f32>,
    pub offset: Point<f32>,
}

pub trait EmbeddedView: std::any::Any {
    fn source_viewport(&self) -> Option<SourceViewport> {
        None
    }
    /// Reveal an intrinsic cell-text rectangle after keyboard/IME navigation.
    /// Passive geometry queries never scroll a block.
    fn reveal_source(&mut self, _bounds: Rect<f32>) {}
    /// Opt into edge scrolling while a captured gesture selects source objects.
    fn selection_drag(&self) -> bool {
        false
    }
    /// Scroll local source content. The host separately scrolls the document.
    fn scroll_source(&mut self, _delta: Point<f32>) -> bool {
        false
    }

    /// Pure decoration can opt out of pointer hit testing so a containing
    /// source-backed block owns selection gestures across inline objects.
    fn pointer_events(&self) -> bool {
        true
    }

    /// Supply the cursor at the widget's current paint bounds, without changing
    /// selection or dispatching events. A captured view may be queried outside
    /// its bounds or with no position after window exit. `None` uses the editor's
    /// cursor; `Some(Cursor::Auto)` explicitly uses the platform's default.
    fn pointer_cursor(
        &mut self,
        _position: Option<Point<f32>>,
        _bounds: Rect<f32>,
    ) -> Option<Cursor> {
        None
    }
    fn needs_layout(&self) -> bool {
        false
    }
    fn measure(
        &mut self,
        available_width: f32,
        cache: &TextLayoutCache,
    ) -> crate::render::Result<ViewMetrics>;
    fn paint(&mut self, painter: &mut Painter<'_>, bounds: Rect<f32>) -> crate::render::Result<()>;
    /// Keep events inside this view out of the source editor's default handling.
    /// Like CodeMirror's WidgetType.ignoreEvent, this defaults to true. The view
    /// still receives `input`; returning false allows unhandled events to select
    /// or edit source. Merely pressing a view never changes the source selection.
    fn ignore_event(&self, _event: &InputEvent) -> bool {
        true
    }
    /// Event positions and `cx.bounds` use the same coordinates as paint bounds.
    /// Return true to consume the event regardless of `ignore_event`.
    fn input(&mut self, _event: &InputEvent, _cx: InputContext<'_>, _editor: &Editor) -> bool {
        false
    }
    /// Optional clipboard text for a structured selection without a nested IME.
    fn selected_text(&self) -> Option<String> {
        None
    }
    /// Whether keyboard input belongs to this view after an interaction.
    /// Pointer capture is independent of keyboard focus.
    fn has_focus(&self) -> bool {
        self.text_input().is_some()
    }
    /// Cancel a pointer gesture without producing a release or click.
    fn cancel_pointer(&mut self) {}
    /// Return keyboard focus to the source editor or another view.
    fn blur(&mut self) {
        if let Some(input) = self.text_input_mut() {
            input.focus_changed(false, std::time::Instant::now());
        }
    }
    /// Wheel input follows the hovered view, without taking keyboard focus.
    /// Positions and bounds share the coordinate system used for painting.
    fn mouse_scroll(
        &mut self,
        _event: &MouseEvent,
        _bounds: Rect<f32>,
        _editor: &Editor,
    ) -> EventResponse {
        EventResponse::CONTINUE
    }
    fn bounds_for_range(
        &mut self,
        _range: std::ops::Range<usize>,
        _cache: &TextLayoutCache,
    ) -> Option<Rect<f32>> {
        None
    }
    fn text_input(&self) -> Option<&dyn TextInputClient> {
        None
    }
    fn text_input_mut(&mut self) -> Option<&mut dyn TextInputClient> {
        None
    }
    fn mounted(&mut self, _invalidate: Option<WidgetInvalidator>) {}
    fn unmounted(&mut self) {}
}
/// Optional registry for explicitly identified views and source-backed layouts.
/// Prefer `Replacement::widget` for content whose description is known at projection time.
/// Clone this configuration across component renders to preserve mounted instances.
#[derive(Clone, Default)]
pub struct EditorViews {
    factories: BTreeMap<ViewId, Rc<dyn Fn() -> Box<dyn EmbeddedView>>>,
    dynamic: Vec<Rc<dyn Fn(ViewId) -> Option<Box<dyn EmbeddedView>>>>,
    pub(crate) blocks: BTreeMap<ViewId, Rc<dyn super::BlockLayout>>,
    dynamic_blocks: Vec<Rc<dyn Fn(ViewId) -> Option<Rc<dyn super::BlockLayout>>>>,
}
impl EditorViews {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn register(
        mut self,
        id: ViewId,
        factory: impl Fn() -> Box<dyn EmbeddedView> + 'static,
    ) -> Self {
        self.factories.insert(id, Rc::new(factory));
        self
    }
    /// Resolve IDs that are discovered at runtime, such as parsed formula IDs.
    /// Return `None` for IDs owned by another component. Explicit registrations
    /// take priority; dynamic factories run in registration order until one succeeds.
    ///
    /// The factory runs only when an instance is needed, not on every layout or
    /// paint. Instances are retained by ID and can be recreated after leaving the
    /// viewport, so keep persistent data outside the view and IDs stable across
    /// projections. Capture a shared model to look up the current data by ID.
    pub fn register_dynamic(
        mut self,
        factory: impl Fn(ViewId) -> Option<Box<dyn EmbeddedView>> + 'static,
    ) -> Self {
        self.dynamic.push(Rc::new(factory));
        self
    }
    pub fn block(mut self, id: ViewId, layout: impl super::BlockLayout + 'static) -> Self {
        self.blocks.insert(id, Rc::new(layout));
        self
    }
    /// Resolve block layouts for IDs discovered at runtime, such as one per parsed
    /// table. Explicit `block` registrations take priority; resolvers then run in
    /// registration order. Return a shared layout to avoid allocating per lookup.
    pub fn block_dynamic(
        mut self,
        resolve: impl Fn(ViewId) -> Option<Rc<dyn super::BlockLayout>> + 'static,
    ) -> Self {
        self.dynamic_blocks.push(Rc::new(resolve));
        self
    }
    pub(crate) fn block_layout(&self, id: ViewId) -> Option<Rc<dyn super::BlockLayout>> {
        self.blocks
            .get(&id)
            .cloned()
            .or_else(|| self.dynamic_blocks.iter().find_map(|resolve| resolve(id)))
    }
    /// Whether an ID has an explicit view or block registration. Dynamic factories
    /// are not consulted: checking membership must not create disposable views.
    pub fn contains(&self, id: ViewId) -> bool {
        self.factories.contains_key(&id) || self.blocks.contains_key(&id)
    }
    fn create(&self, id: ViewId) -> Option<Box<dyn EmbeddedView>> {
        if let Some(factory) = self.factories.get(&id) {
            return Some(factory());
        }
        self.dynamic.iter().find_map(|factory| factory(id))
    }
    pub(crate) fn same(&self, other: &Self) -> bool {
        self.blocks.len() == other.blocks.len()
            && self
                .blocks
                .iter()
                .all(|(id, a)| other.blocks.get(id).is_some_and(|b| Rc::ptr_eq(a, b)))
            && self.factories.len() == other.factories.len()
            && self
                .factories
                .iter()
                .all(|(id, a)| other.factories.get(id).is_some_and(|b| Rc::ptr_eq(a, b)))
            && self.dynamic.len() == other.dynamic.len()
            && self
                .dynamic
                .iter()
                .zip(&other.dynamic)
                .all(|(a, b)| Rc::ptr_eq(a, b))
            && self.dynamic_blocks.len() == other.dynamic_blocks.len()
            && self
                .dynamic_blocks
                .iter()
                .zip(&other.dynamic_blocks)
                .all(|(a, b)| Rc::ptr_eq(a, b))
    }
}
pub(crate) struct MountedView {
    pub view: Box<dyn EmbeddedView>,
    description: Option<ViewSpec>,
    dirty: bool,
}
#[derive(Default)]
pub(crate) struct MountedViews {
    pub views: BTreeMap<ViewId, MountedView>,
    pub focused: Option<ViewId>,
    pub pointer: Option<ViewId>,
    pub invalidate: Option<WidgetInvalidator>,
    pub factories: EditorViews,
    /// Current projection inputs, including descriptions for offscreen views.
    pub descriptions: BTreeMap<ViewId, ViewSpec>,
}
impl MountedViews {
    pub fn get(&mut self, id: ViewId) -> crate::render::Result<&mut Box<dyn EmbeddedView>> {
        if let Some(mounted) = self.views.get_mut(&id) {
            let next = self.descriptions.get(&id);
            if mounted.description.as_ref() != next {
                let updated = match (next, mounted.description.as_ref()) {
                    (Some(next), Some(previous)) => next.update(previous, mounted.view.as_mut()),
                    _ => false,
                };
                if updated {
                    mounted.description = next.cloned();
                    mounted.dirty = true;
                    if self.focused == Some(id) && !mounted.view.has_focus() {
                        self.focused = None;
                    }
                } else {
                    self.remove(id);
                }
            }
        }
        if !self.views.contains_key(&id) {
            let description = self.descriptions.get(&id).cloned();
            let mut view = if let Some(description) = &description {
                description.create()
            } else {
                self.factories
                    .create(id)
                    .ok_or_else(|| anyhow::anyhow!("no view factory for {:?}", id))?
            };
            view.mounted(self.invalidate.clone());
            self.views.insert(
                id,
                MountedView {
                    view,
                    description,
                    dirty: true,
                },
            );
        }
        Ok(&mut self.views.get_mut(&id).unwrap().view)
    }
    pub fn needs_update(&self, id: ViewId) -> bool {
        self.views.get(&id).is_none_or(|mounted| {
            mounted.dirty || mounted.description.as_ref() != self.descriptions.get(&id)
        })
    }
    pub fn measure(
        &mut self,
        id: ViewId,
        width: f32,
        cache: &TextLayoutCache,
    ) -> crate::render::Result<ViewMetrics> {
        let metrics = self.get(id)?.measure(width, cache)?.validate()?;
        self.views.get_mut(&id).unwrap().dirty = false;
        Ok(metrics)
    }
    pub fn remove(&mut self, id: ViewId) {
        if let Some(mut mounted) = self.views.remove(&id) {
            if self.pointer == Some(id) {
                self.pointer = None;
                mounted.view.cancel_pointer();
            }
            if self.focused == Some(id) {
                self.focused = None;
                mounted.view.blur();
            }
            mounted.view.unmounted();
        }
    }
    /// Removed projection entries cannot keep focus or retain a stale instance.
    pub fn retain_present(&mut self, present: &BTreeSet<ViewId>) {
        let removed: Vec<_> = self
            .views
            .keys()
            .filter(|id| !present.contains(id))
            .copied()
            .collect();
        for id in removed {
            self.remove(id);
        }
    }
    pub fn retain(&mut self, visible: &BTreeSet<ViewId>) {
        let mut keep = visible.clone();
        keep.extend(self.focused);
        keep.extend(self.pointer);
        self.retain_present(&keep);
    }
}
impl Drop for MountedViews {
    fn drop(&mut self) {
        let ids: Vec<_> = self.views.keys().copied().collect();
        for id in ids {
            self.remove(id);
        }
    }
}

/// Adapt any existing VoidUI subtree, including images and controls. The subtree
/// shares the parent's font service and invalidation wakeup, not a separate window.
pub struct WidgetView {
    runtime: Option<crate::TaskRuntime>,
    factory: Rc<dyn Fn() -> crate::Element>,
    tree: Option<crate::core::widget_tree::WidgetTree>,
    cache: Option<TextLayoutCache>,
    bounds: Option<Rect<f32>>,
    size: Size<f32>,
    ignore_events: fn(&InputEvent) -> bool,
    input_capture: Option<crate::core::widget::WidgetId>,
    align: crate::render::InlineAlignment,
    offset_em: f32,
}
impl WidgetView {
    pub fn new(factory: impl Fn() -> crate::Element + 'static) -> Self {
        Self {
            runtime: None,
            factory: Rc::new(factory),
            tree: None,
            cache: None,
            bounds: None,
            size: Size::default(),
            ignore_events: |_| true,
            input_capture: None,
            align: Default::default(),
            offset_em: 0.0,
        }
    }
    /// Choose which events the source editor ignores. Unhandled events for which
    /// this returns false fall back to ordinary source selection and editing.
    pub fn ignore_events(mut self, policy: fn(&InputEvent) -> bool) -> Self {
        self.ignore_events = policy;
        self
    }
    /// Choose a CSS parent-relative alignment for this embedded widget.
    pub fn inline_align(mut self, align: crate::render::InlineAlignment) -> Self {
        self.align = align;
        self
    }
    /// Apply a visual relative offset using the surrounding text's font size.
    /// Positive values move down; line height and the baseline stay unchanged.
    pub fn inline_offset_em(mut self, offset: f32) -> Self {
        self.offset_em = offset;
        self
    }
    pub(crate) fn set_inline_offset_em(&mut self, offset: f32) {
        self.offset_em = offset;
    }
    pub(crate) fn set_inline_align(&mut self, align: crate::render::InlineAlignment) {
        self.align = align;
    }
    pub(crate) fn set_event_policy(&mut self, policy: fn(&InputEvent) -> bool) {
        self.ignore_events = policy;
    }
    pub(crate) fn rebuild(&mut self, factory: impl Fn() -> crate::Element + 'static) {
        self.factory = Rc::new(factory);
        if let Some(tree) = &mut self.tree {
            tree.reconcile_root((self.factory)());
        }
        self.bounds = None;
    }
    fn tree(&mut self) -> &mut crate::core::widget_tree::WidgetTree {
        self.tree.get_or_insert_with(|| {
            let mut t = crate::core::widget_tree::WidgetTree::with_task_runtime(
                self.runtime.clone().unwrap_or_default(),
            );
            t.build_root((self.factory)());
            t
        })
    }
    /// Keep hit testing and painting at the same position, even if the parent
    /// scrolled or moved since the previous frame.
    fn place(&mut self, bounds: Rect<f32>) -> crate::render::Result<()> {
        use crate::core::layout::{AvailableSpace as A, Size as S};
        let cache = self
            .cache
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("measure embedded widget before placing it"))?;
        let tree = self.tree.as_mut().unwrap();
        let changes = tree.update_styles(std::time::Instant::now());
        if self.bounds != Some(bounds) || changes.layout || changes.paint {
            let root = tree.root().unwrap();
            tree.layout_subtree(
                root,
                S {
                    width: A::Definite(bounds.size.width),
                    height: A::Definite(bounds.size.height),
                },
                bounds.origin,
                cache,
            );
            self.bounds = Some(bounds);
        }
        Ok(())
    }
}
impl EmbeddedView for WidgetView {
    fn pointer_cursor(
        &mut self,
        position: Option<Point<f32>>,
        bounds: Rect<f32>,
    ) -> Option<Cursor> {
        if let Err(error) = self.place(bounds) {
            log::error!("embedded cursor layout failed: {error:#}");
            return None;
        }
        Some(
            self.tree
                .as_ref()?
                .pointer_cursor_at(position, self.input_capture),
        )
    }
    fn ignore_event(&self, event: &InputEvent) -> bool {
        (self.ignore_events)(event)
    }
    fn has_focus(&self) -> bool {
        self.tree
            .as_ref()
            .is_some_and(|tree| tree.focused().is_some())
    }
    fn cancel_pointer(&mut self) {
        if let Some(tree) = &mut self.tree {
            tree.cancel_pointer_capture();
            if let Some(id) = self.input_capture.take()
                && let Some(cache) = &self.cache
            {
                tree.dispatch_input(
                    id,
                    &InputEvent::Pointer {
                        phase: crate::core::input::PointerPhase::Cancel,
                        // Cancellation does not hit-test or move the selection.
                        position: Default::default(),
                        modifiers: Default::default(),
                        clicks: 0,
                    },
                    cache,
                );
            }
        }
    }
    fn blur(&mut self) {
        self.cancel_pointer();
        if let Some(tree) = &mut self.tree {
            tree.set_focused(None);
        }
    }
    fn unmounted(&mut self) {
        self.blur();
    }
    fn needs_layout(&self) -> bool {
        self.tree.as_ref().is_some_and(|t| t.has_pending_updates())
    }
    fn mounted(&mut self, invalidate: Option<WidgetInvalidator>) {
        if let Some(invalidate) = invalidate {
            self.runtime = invalidate.task_runtime();
            self.tree().set_update_waker(move || invalidate.repaint());
        }
    }
    fn measure(
        &mut self,
        width: f32,
        cache: &TextLayoutCache,
    ) -> crate::render::Result<ViewMetrics> {
        use crate::core::layout::{AvailableSpace as A, Size as S};
        self.cache = Some(TextLayoutCache::new(cache.system().clone()));
        self.size = self.tree().layout(
            S {
                width: A::Definite(width.max(0.0)),
                height: A::MaxContent,
            },
            cache,
        );
        self.bounds = None;
        ViewMetrics {
            size: self.size,
            baseline: self.size.height,
            align: self.align,
            offset_em: self.offset_em,
        }
        .validate()
    }
    fn paint(&mut self, painter: &mut Painter<'_>, bounds: Rect<f32>) -> crate::render::Result<()> {
        self.place(bounds)?;
        self.tree.as_mut().unwrap().draw(painter)
    }
    fn mouse_scroll(&mut self, event: &MouseEvent, bounds: Rect<f32>, _: &Editor) -> EventResponse {
        if let Err(error) = self.place(bounds) {
            log::error!("embedded wheel layout failed: {error:#}");
            return EventResponse::CONTINUE;
        }
        let tree = self.tree.as_mut().unwrap();
        tree.dispatch_mouse_move(Some(event.position), event.modifiers);
        tree.dispatch_mouse_scroll(event.wheel, event.modifiers)
    }
    fn input(&mut self, event: &InputEvent, cx: InputContext<'_>, _: &Editor) -> bool {
        use crate::core::input::PointerPhase;
        if let Err(error) = self.place(cx.bounds) {
            log::error!("embedded input layout failed: {error:#}");
            return false;
        }
        if matches!(
            event,
            InputEvent::Pointer {
                phase: PointerPhase::Cancel,
                ..
            }
        ) {
            self.cancel_pointer();
            return true;
        }
        let Some(tree) = self.tree.as_mut() else {
            return false;
        };
        match event {
            InputEvent::Pointer {
                phase,
                position,
                modifiers,
                ..
            } => {
                let mut handled = tree
                    .dispatch_mouse_move(Some(*position), *modifiers)
                    .response
                    .prevent_default;
                if matches!(phase, PointerPhase::Down | PointerPhase::Up) {
                    handled |= tree
                        .dispatch_mouse_button(
                            crate::MouseButton::Left,
                            *phase == PointerPhase::Down,
                            *modifiers,
                        )
                        .response
                        .prevent_default;
                }
                // Match native input routing: a scrollbar owns the gesture before
                // any nested text control can start a selection drag.
                if *phase == PointerPhase::Down {
                    self.input_capture = if handled {
                        None
                    } else {
                        tree.input_at(*position)
                    };
                }
                if let Some(id) = self.input_capture {
                    handled |= tree.dispatch_input(id, event, cx.text_layout)
                        == crate::core::event::EventResult::Handled;
                }
                if *phase == PointerPhase::Up {
                    self.input_capture = None;
                }
                handled
            }
            _ => tree.focused().is_some_and(|id| {
                tree.dispatch_input(id, event, cx.text_layout)
                    == crate::core::event::EventResult::Handled
            }),
        }
    }
    fn bounds_for_range(
        &mut self,
        range: std::ops::Range<usize>,
        cache: &TextLayoutCache,
    ) -> Option<Rect<f32>> {
        let tree = self.tree.as_mut()?;
        tree.input_bounds_for_range(tree.focused()?, range, cache)
    }
    fn text_input(&self) -> Option<&dyn TextInputClient> {
        let tree = self.tree.as_ref()?;
        tree.text_input(tree.focused()?)
    }
    fn text_input_mut(&mut self) -> Option<&mut dyn TextInputClient> {
        let tree = self.tree.as_mut()?;
        let id = tree.focused()?;
        tree.nodes.get_mut(id)?.widget.as_mut()?.text_input_mut()
    }
}
