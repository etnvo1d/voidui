//! Embedded event ownership is independent of source selection and keyboard focus.
use super::*;

impl EditorLayout {
    /// Whether this source position belongs to an IME-preserving compound block.
    pub fn is_source_backed(&self, position: usize) -> bool {
        let state = self.engine.borrow();
        let Some(e) = state.as_ref() else {
            return false;
        };
        let Some(i) = e.block_at(position) else {
            return false;
        };
        e.blocks[i].view.is_some_and(|id| {
            e.projection
                .blocks
                .iter()
                .any(|b| b.id == id && b.source_backed)
        })
    }

    /// Find the background interaction layer after testing inline child widgets.
    pub fn decoration_at(&self, point: Point<f32>) -> Option<(ViewId, Rect<f32>)> {
        let mut state = self.engine.borrow_mut();
        let e = state.as_mut()?;
        let i = e.heights.at(point.y);
        e.ensure(i).ok()?;
        let b = &e.blocks[i];
        let c = &e.cache[&i];
        c.decoration.as_ref()?;
        let bounds = Rect::from_xywh(
            b.style.inset_left,
            e.heights.top(i) + b.style.space_before,
            c.width,
            c.height,
        );
        contains(bounds, point).then_some((b.view?, bounds))
    }
    /// Scroll a source-backed block to its active caret without moving the page.
    pub fn reveal_source_caret(
        &self,
        position: usize,
        affinity: Bias,
        width: f32,
        align: TextAlign,
    ) {
        let mut state = self.engine.borrow_mut();
        let Some(e) = state.as_mut() else {
            return;
        };
        let Some(i) = e.block_at(position) else {
            return;
        };
        if e.ensure(i).is_err() || e.source_viewport(i).is_none() {
            return;
        }
        let Some(id) = e.blocks[i].view else {
            return;
        };
        if let Some(mut r) = e.caret(i, position, affinity, width, align) {
            let offset = e.source_offset(i);
            r.origin.x += offset.x - e.blocks[i].style.inset_left;
            r.origin.y += offset.y - e.heights.top(i) - e.blocks[i].style.space_before;
            if let Ok(view) = e.views.borrow_mut().get(id) {
                view.reveal_source(r);
            }
        }
    }
    /// Structured selections can supply clipboard text without owning an IME.
    pub fn selected_view_text(&self) -> Option<String> {
        let host = self.views.borrow();
        host.views.get(&host.focused?)?.view.selected_text()
    }

    /// Query an already measured view without manufacturing hover or drag events.
    pub fn view_cursor(
        &self,
        id: ViewId,
        position: Option<Point<f32>>,
        bounds: Rect<f32>,
    ) -> Option<crate::style::selection::Cursor> {
        self.views
            .borrow_mut()
            .views
            .get_mut(&id)?
            .view
            .pointer_cursor(position, bounds)
    }
    pub fn with_focused_input_mut<R>(
        &self,
        f: impl FnOnce(&mut dyn crate::core::input::TextInputClient) -> R,
    ) -> Option<R> {
        let mut host = self.views.borrow_mut();
        let id = host.focused?;
        Some(f(host.views.get_mut(&id)?.view.text_input_mut()?))
    }
    pub fn with_focused_input<R>(
        &self,
        f: impl FnOnce(&dyn crate::core::input::TextInputClient) -> R,
    ) -> Option<R> {
        let host = self.views.borrow();
        let id = host.focused?;
        Some(f(host.views.get(&id)?.view.text_input()?))
    }
    /// Offer an event to a widget before the source editor changes selection.
    /// `cx.bounds` must be the widget's current paint bounds. A consumed press
    /// owns subsequent pointer events until release/cancel, even outside it.
    pub fn dispatch_view(
        &self,
        id: ViewId,
        event: &crate::core::input::InputEvent,
        cx: crate::core::input::InputContext<'_>,
        editor: &crate::editing::Editor,
    ) -> bool {
        let mut host = self.views.borrow_mut();
        use crate::core::input::{InputEvent, PointerPhase};
        let captured = matches!(event, InputEvent::Pointer { .. }) && host.pointer == Some(id);
        let Ok(view) = host.get(id) else { return false };
        let ignored = view.ignore_event(event);
        let handled = view.input(event, cx, editor) || ignored || captured;
        if matches!(
            event,
            InputEvent::Pointer {
                phase: PointerPhase::Cancel,
                ..
            }
        ) || (!handled
            && matches!(
                event,
                InputEvent::Pointer {
                    phase: PointerPhase::Down,
                    ..
                }
            ))
        {
            // A press delegated to source selection must not leave a pending
            // widget click or button state waiting for a release it will not get.
            view.cancel_pointer();
        }
        let focused = view.has_focus();
        if focused && host.focused != Some(id) {
            if let Some(previous) = host.focused.take()
                && let Some(previous) = host.views.get_mut(&previous)
            {
                previous.view.blur();
            }
            host.focused = Some(id);
        } else if !focused && host.focused == Some(id) {
            host.focused = None;
        }
        if let InputEvent::Pointer { phase, .. } = event {
            match phase {
                PointerPhase::Down if handled => host.pointer = Some(id),
                PointerPhase::Up | PointerPhase::Cancel => host.pointer = None,
                _ => {}
            }
        }
        handled
    }
    /// Only selection gestures opt in; scrollbar thumbs and buttons never start
    /// document autoscrolling merely because they captured the pointer.
    pub fn captured_selection_view(&self) -> Option<ViewId> {
        let host = self.views.borrow();
        let id = host.pointer?;
        host.views.get(&id)?.view.selection_drag().then_some(id)
    }
    pub fn scroll_view_source(&self, id: ViewId, delta: Point<f32>) -> bool {
        self.views
            .borrow_mut()
            .views
            .get_mut(&id)
            .is_some_and(|mounted| mounted.view.scroll_source(delta))
    }
    /// The view that owns the current pointer gesture, not keyboard focus.
    pub fn captured_view(&self) -> Option<ViewId> {
        self.views.borrow().pointer
    }
    /// End a hosted gesture without changing text selection or synthesizing a click.
    pub fn cancel_view_pointer(&self) {
        let mut host = self.views.borrow_mut();
        if let Some(id) = host.pointer.take()
            && let Some(mounted) = host.views.get_mut(&id)
        {
            mounted.view.cancel_pointer();
        }
    }
    pub fn dispatch_view_scroll(
        &self,
        id: ViewId,
        event: &crate::core::event::MouseEvent,
        bounds: Rect<f32>,
        editor: &crate::editing::Editor,
    ) -> crate::core::event::EventResponse {
        self.views
            .borrow_mut()
            .get(id)
            .map_or(crate::core::event::EventResponse::CONTINUE, |view| {
                view.mouse_scroll(event, bounds, editor)
            })
    }
    pub fn focused_bounds_for_range(
        &self,
        range: Range<usize>,
        cache: &TextLayoutCache,
    ) -> Option<Rect<f32>> {
        let mut views = self.views.borrow_mut();
        let id = views.focused?;
        views
            .views
            .get_mut(&id)?
            .view
            .bounds_for_range(range, cache)
    }
    pub fn focused_view(&self) -> Option<ViewId> {
        self.views.borrow().focused
    }
    pub fn release_view_focus(&self) {
        let mut views = self.views.borrow_mut();
        if let Some(id) = views.focused.take() {
            if let Some(mounted) = views.views.get_mut(&id) {
                mounted.view.blur();
            }
        }
    }
    /// Find a mounted widget's geometry without hit-testing the current pointer.
    /// Captured events can be outside this rectangle or over another widget.
    pub fn view_bounds(&self, id: ViewId, width: f32, align: TextAlign) -> Option<Rect<f32>> {
        let mut state = self.engine.borrow_mut();
        let e = state.as_mut()?;
        let i = e
            .cache
            .iter()
            .find_map(|(i, c)| {
                c.objects
                    .iter()
                    .any(|(object, _, _)| *object == id)
                    .then_some(*i)
            })
            .or_else(|| {
                let range = e
                    .projection
                    .blocks
                    .iter()
                    .find(|b| b.id == id)
                    .map(|b| &b.range)
                    .or_else(|| {
                        e.projection
                            .replacements
                            .iter()
                            .find(|r| r.id == id)
                            .map(|r| &r.range)
                    })?;
                e.block_at(range.start)
            })?;
        e.ensure(i).ok()?;
        let b = &e.blocks[i];
        let c = &e.cache[&i];
        if b.view == Some(id) && c.decoration.is_some() {
            return Some(Rect::from_xywh(
                b.style.inset_left,
                e.heights.top(i) + b.style.space_before,
                c.width,
                c.height,
            ));
        }
        let objects =
            e.cache[&i].object_bounds(e.content_width_at(b, width), b.style.align.unwrap_or(align));
        let (_, _, mut bounds) = objects.iter().find(|(object, _, _)| *object == id)?.clone();
        bounds.origin.x += b.style.inset_left - e.source_offset(i).x;
        bounds.origin.y += e.heights.top(i) + b.style.space_before - e.source_offset(i).y;
        Some(bounds)
    }
}
