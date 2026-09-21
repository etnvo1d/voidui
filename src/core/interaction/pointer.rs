//! Pointer geometry, hover transitions, and drag ownership share one state machine.
use super::*;
use crate::style::selection::Cursor;

#[derive(Default)]
pub(super) struct PointerState {
    pub last_position: Option<Point<f32>>,
    pub modifiers: ModifiersState,
    buttons: MouseButtons,
    pub capture: Option<Capture>,
}
#[derive(Clone, Copy)]
pub(super) struct Capture {
    pub owner: WidgetId,
    origin: Point<f32>,
    previous: Point<f32>,
    moved: bool,
    cursor: Cursor,
}
impl WidgetTree {
    fn needs_pointer_tracking(&self) -> bool {
        self.window_host.is_custom()
            || self.has_dynamic_css()
            || !self.top_layers.is_empty()
            || self.click_handlers != 0
            || self.input_handlers != 0
            || self.events.pointer_listeners != 0
            || self.events.custom_listeners != 0
    }
    fn mouse_event(
        &self,
        target: WidgetId,
        state: MouseEventType,
        button: Option<MouseButton>,
    ) -> MouseEvent {
        let position = self.events.pointer.last_position.unwrap_or_default();
        MouseEvent {
            target,
            current_target: target,
            related_target: None,
            button,
            buttons: self.events.pointer.buttons,
            modifiers: self.events.pointer.modifiers,
            position,
            local_position: position,
            wheel: MouseWheel::default(),
            state,
        }
    }
    fn hover_transition(&mut self, hit: Option<WidgetId>) -> bool {
        let previous = self.statused_widgets.top_hovered;
        if previous == hit {
            return false;
        }
        let path = |mut node: Option<WidgetId>| {
            let mut path = Vec::new();
            while let Some(id) = node {
                let Some(current) = self.nodes.get(id) else {
                    break;
                };
                path.push(id);
                node = current.parent;
            }
            path
        };
        let (mut old, mut new) = (path(previous), path(hit));
        // Remove shared ancestors so moving between siblings never emits a leave
        // and re-enter for their parent, or toggles its :hover state twice.
        while !old.is_empty() && old.last() == new.last() {
            old.pop();
            new.pop();
        }
        self.statused_widgets.top_hovered = hit;
        let mut changed = false;
        for &id in &old {
            let mut status = self.nodes[id].status;
            status.set_hovered(false);
            changed |= self.set_status(id, status);
        }
        for &id in &new {
            let mut status = self.nodes[id].status;
            status.set_hovered(true);
            changed |= self.set_status(id, status);
        }
        for id in old {
            if self.event_enabled(id) {
                let mut event = self.mouse_event(id, MouseEventType::Left, None);
                event.related_target = hit;
                self.dispatch_at(id, EventKind::MouseLeave, &mut Event::Mouse(event));
            }
        }
        for id in new.into_iter().rev() {
            if self.event_enabled(id) {
                let mut event = self.mouse_event(id, MouseEventType::Entered, None);
                event.related_target = previous;
                self.dispatch_at(id, EventKind::MouseEnter, &mut Event::Mouse(event));
            }
        }
        changed
    }
    /// Compatibility helper for hosts that do not need default-prevention results.
    pub fn pointer_moved(&mut self, point: Option<Point<f32>>) -> bool {
        self.dispatch_mouse_move(point, self.events.pointer.modifiers)
            .changed
    }
    pub fn dispatch_mouse_move(
        &mut self,
        point: Option<Point<f32>>,
        modifiers: ModifiersState,
    ) -> EventDispatch {
        self.events.pointer.modifiers = modifiers;
        let previous = self.events.pointer.last_position;
        self.pointer_position = point;
        if let Some(point) = point {
            self.events.pointer.last_position = Some(point);
        }
        let mut dispatch = EventDispatch {
            changed: self.validate_pointer_capture(),
            ..Default::default()
        };
        if self.scroll_drag.is_some() {
            if let Some(point) = point {
                dispatch.changed |= self.move_scrollbar_drag(point);
            }
            dispatch.response.prevent_default = true;
            return dispatch;
        }
        if !self.needs_pointer_tracking() {
            return dispatch;
        }
        // A capture does not depend on layout validity. Pending layout must not
        // interrupt a drag whose own callback is moving or resizing its owner.
        let hit = point.and_then(|point| self.event_at(point));
        if point.is_none() || self.layout_ready {
            dispatch.changed |= self.hover_transition(hit);
        }
        self.pointer_revision = self.paint_order.borrow().revision;
        if let Some(point) = point {
            let target = self
                .events
                .pointer
                .capture
                .map(|capture| capture.owner)
                .or(hit);
            if let Some(target) = target {
                let event = self.mouse_event(target, MouseEventType::Moved, None);
                dispatch.response |= self.bubble(target, Event::Mouse(event));
            }
            if previous != Some(point)
                && let Some(mut capture) = self.events.pointer.capture
            {
                capture.moved |= point != capture.origin;
                self.events.pointer.capture = Some(capture);
                dispatch.response |= self.dispatch_drag(capture, DragPhase::Move, point);
                if let Some(active) = &mut self.events.pointer.capture {
                    active.previous = point;
                }
                if capture.moved {
                    self.click_pressed = None;
                }
            }
        }
        dispatch
    }
    /// Refresh only hit/hover state after layout or layer changes. This never
    /// synthesizes motion or another drag sample for a stationary pointer.
    pub(crate) fn refresh_event_pointer(&mut self) -> bool {
        let mut changed = self.validate_pointer_capture();
        if !self.layout_ready || self.styles_pending() {
            return changed;
        }
        if !self.paint_order_dirty.get()
            && !self.paint_clip_dirty.get()
            && self.pointer_revision == self.paint_order.borrow().revision
        {
            return changed;
        }
        if self.needs_pointer_tracking() {
            let hit = self.pointer_position.and_then(|point| self.event_at(point));
            changed |= self.hover_transition(hit);
            self.pointer_revision = self.paint_order.borrow().revision;
        }
        changed
    }
    pub fn pointer_pressed(&mut self, pressed: bool) -> bool {
        self.dispatch_mouse_button(MouseButton::Left, pressed, self.events.pointer.modifiers)
            .changed
    }
    /// Route every mouse button. Only the primary button activates click, focus,
    /// text selection, and the automatic on_drag gesture.
    pub fn dispatch_mouse_button(
        &mut self,
        button: MouseButton,
        pressed: bool,
        modifiers: ModifiersState,
    ) -> EventDispatch {
        self.events.pointer.modifiers = modifiers;
        let mut result = EventDispatch {
            changed: self.validate_pointer_capture(),
            ..Default::default()
        };
        let flag = MouseButtons::flag(button);
        let was_pressed = self.events.pointer.buttons.contains(flag) && !flag.is_empty();
        self.events.pointer.buttons.set(flag, pressed);
        if self.scroll_drag.is_some() {
            if button == MouseButton::Left && !pressed {
                self.scroll_drag = None;
                result.changed |= self.set_pointer_active(None, false);
            }
            result.response.prevent_default = true;
            return result;
        }
        let capture = self.events.pointer.capture;
        let hit = self.pointer_position.and_then(|point| self.event_at(point));
        // Capture owns the pointer stream, including additional button events.
        // Only the matching primary release ends the gesture below.
        let target = capture.map(|capture| capture.owner).or(hit);
        if let Some(target) = target {
            let state = if pressed {
                MouseEventType::Pressed
            } else {
                MouseEventType::Released
            };
            let event = self.mouse_event(target, state, Some(button));
            result.response = self.bubble(target, Event::Mouse(event));
        }
        if button != MouseButton::Left {
            return result;
        }
        if pressed {
            // Ignore duplicated presses for gesture ownership, while still exposing
            // the raw button event. A second button never steals a primary drag.
            if was_pressed {
                return result;
            }
            if !result.response.prevent_default
                && self.events.pointer.capture.is_none()
                && let Some(point) = self.pointer_position
                && self.start_scrollbar_drag(point)
            {
                result.response.prevent_default = true;
                result.changed = true;
                return result;
            }
            self.click_pressed = hit.and_then(|id| self.click_target(id));
            result.changed |= self.set_pointer_active(hit, true);
            if !result.response.prevent_default {
                result.changed |= self.focus_pointer_target(hit);
                if self.events.pointer.capture.is_none()
                    && let (Some(owner), Some(position)) = (
                        hit.and_then(|id| self.drag_target(id)),
                        self.events.pointer.last_position,
                    )
                {
                    let capture = Capture {
                        owner,
                        origin: position,
                        previous: position,
                        moved: false,
                        cursor: self.owner_cursor(owner),
                    };
                    self.events.pointer.capture = Some(capture);
                    self.end_selection_drag();
                    result.response |= self.dispatch_drag(capture, DragPhase::Start, position);
                }
            } else {
                self.click_pressed = None;
            }
        } else {
            result.changed |= self.set_pointer_active(None, false);
            if let Some(capture) = self.events.pointer.capture.take() {
                let point = self
                    .events
                    .pointer
                    .last_position
                    .unwrap_or(capture.previous);
                result.response |= self.dispatch_drag(capture, DragPhase::End, point);
                if capture.moved {
                    self.click_pressed = None;
                }
            }
            if !result.response.prevent_default
                && let Some(pressed) = self.click_pressed.take()
                && hit.and_then(|id| self.click_target(id)) == Some(pressed)
                && let Some(hit) = hit
            {
                self.click_with_source(hit, ClickSource::Pointer);
            }
            self.click_pressed = None;
        }
        result
    }
    /// Dispatch wheel handlers and retained scrolling. `prevent_default` also
    /// reports consumed scrolling, so hosts can avoid scrolling the delta twice.
    pub fn dispatch_mouse_scroll(
        &mut self,
        wheel: MouseWheel,
        modifiers: ModifiersState,
    ) -> EventResponse {
        self.events.pointer.modifiers = modifiers;
        self.validate_pointer_capture();
        let target = self
            .pointer_capture()
            .or_else(|| self.pointer_position.and_then(|p| self.event_at(p)));
        let Some(target) = target else {
            return EventResponse::CONTINUE;
        };
        let mut event = self.mouse_event(target, MouseEventType::Scrolled, None);
        event.wheel = wheel;
        let mut response = self.bubble(target, Event::Mouse(event));
        let legacy_input = self.input_handlers != 0
            && self
                .pointer_position
                .and_then(|p| self.input_at(p))
                .is_some_and(|id| self.scroll_metrics(id).is_none());
        let remaining = response.remaining_scroll(wheel);
        if !response.consumed_scroll.is_empty() && remaining.x == 0.0 && remaining.y == 0.0 {
            response |= EventResponse::PREVENT_DEFAULT;
        }
        if !response.prevent_default && !legacy_input {
            // A hosted tree must report consumed scrolling so its parent does
            // not apply the same wheel delta a second time.
            if self
                .scroll_wheel_from(target, remaining, modifiers)
                .consumed
            {
                response |= EventResponse::PREVENT_DEFAULT;
            }
        }
        response
    }
    fn set_pointer_active(&mut self, hit: Option<WidgetId>, pressed: bool) -> bool {
        let hit = if pressed {
            hit
        } else {
            self.statused_widgets
                .mouse_down
                .get(&MouseButton::Left)
                .copied()
                .flatten()
        };
        self.statused_widgets
            .mouse_down
            .insert(MouseButton::Left, if pressed { hit } else { None });
        let mut current = hit;
        let mut changed = false;
        while let Some(id) = current {
            let Some(node) = self.nodes.get(id) else {
                break;
            };
            let mut status = node.status;
            current = node.parent;
            status.set_active(pressed);
            changed |= self.set_status(id, status);
        }
        changed
    }
    fn focus_pointer_target(&mut self, hit: Option<WidgetId>) -> bool {
        if hit.is_none() && self.active_modal().is_some() {
            return false;
        }
        let mut focus = hit;
        while let Some(id) = focus {
            let node = &self.nodes[id];
            let p = &node.props;
            if !p.attributes.contains_key("disabled")
                && (node.scroll.is_some()
                    || p.attributes.contains_key("tabindex")
                    || matches!(p.tag.as_str(), "button" | "input" | "textarea")
                    || self.text_input(id).is_some())
            {
                break;
            }
            focus = node.parent;
        }
        let focus = focus.or_else(|| self.pointer_position.and_then(|p| self.input_at(p)));
        self.set_focused(focus.or_else(|| self.active_modal()))
    }
    fn drag_target(&self, mut id: WidgetId) -> Option<WidgetId> {
        loop {
            if self.events.has(id, EventKind::Drag) {
                return Some(id);
            }
            // A drag handle explicitly attached to an input wins; an ancestor's
            // drag handle must not steal ordinary editing selection from the input.
            if self.text_input(id).is_some() {
                return None;
            }
            id = self.nodes.get(id)?.parent?;
        }
    }
    fn dispatch_drag(
        &mut self,
        capture: Capture,
        phase: DragPhase,
        position: Point<f32>,
    ) -> EventResponse {
        let event = DragEvent {
            target: capture.owner,
            current_target: capture.owner,
            phase,
            button: MouseButton::Left,
            modifiers: self.events.pointer.modifiers,
            origin: capture.origin,
            position,
            local_position: position,
            delta: Point::new(
                position.x - capture.previous.x,
                position.y - capture.previous.y,
            ),
            total_delta: Point::new(position.x - capture.origin.x, position.y - capture.origin.y),
        };
        // Cancellation is delivered directly even if the owner has just become
        // disabled/inert. Unmount then drops the handler and cancels async calls.
        self.dispatch_at(capture.owner, EventKind::Drag, &mut Event::Drag(event))
    }
    pub fn pointer_capture(&self) -> Option<WidgetId> {
        self.scroll_drag
            .map(|d| d.owner)
            .or_else(|| self.events.pointer.capture.map(|c| c.owner))
    }
    /// End capture without manufacturing a mouse-up/click. Call on focus loss,
    /// suspension, or when an external host reports native capture cancellation.
    pub fn cancel_pointer_capture(&mut self) -> bool {
        self.click_pressed = None;
        self.events.pointer.buttons = MouseButtons::empty();
        let changed = self.scroll_drag.take().is_some() | self.set_pointer_active(None, false);
        if let Some(capture) = self.events.pointer.capture.take() {
            self.dispatch_drag(
                capture,
                DragPhase::Cancel,
                self.events
                    .pointer
                    .last_position
                    .unwrap_or(capture.previous),
            );
        }
        self.end_selection_drag();
        changed
    }
    pub(crate) fn validate_pointer_capture(&mut self) -> bool {
        if self.scroll_drag.is_some_and(|d| {
            !self.event_enabled(d.owner) || self.scrollbar_geometry(d.owner, d.axis).is_none()
        }) {
            return self.cancel_pointer_capture();
        }
        if self.events.pointer.capture.is_some_and(|c| {
            !self.event_enabled(c.owner) || !self.events.has(c.owner, EventKind::Drag)
        }) {
            self.cancel_pointer_capture()
        } else {
            false
        }
    }
    fn owner_cursor(&self, owner: WidgetId) -> Cursor {
        let node = &self.nodes[owner];
        match node.computed.selection.cursor {
            Cursor::Auto
                if node.widget.as_ref().is_some_and(|w| {
                    w.text_input().is_some()
                        || (w.prepared_text().is_some()
                            && node.computed.selection.used_user_select
                                != crate::style::selection::UserSelect::None)
                }) =>
            {
                Cursor::Icon(winit::window::CursorIcon::Text)
            }
            cursor => cursor,
        }
    }
    /// Resolve the effective cursor. A capture owner takes precedence over hit
    /// testing, selection, layout readiness, and leaving the native window.
    pub fn pointer_cursor(&self) -> Cursor {
        self.pointer_cursor_at(self.pointer_position, None)
    }
    /// Resolve a cursor without synthesizing a pointer move. Hosts pass their
    /// captured text client so nested drags retain their cursor across siblings
    /// and window exit. The same query works for hosted trees before first paint.
    pub fn pointer_cursor_at(
        &self,
        position: Option<Point<f32>>,
        input_capture: Option<WidgetId>,
    ) -> Cursor {
        if self.scroll_drag.is_some() {
            return Cursor::Icon(winit::window::CursorIcon::Default);
        }
        if let Some(capture) = self.events.pointer.capture {
            return if self.event_enabled(capture.owner) {
                self.owner_cursor(capture.owner)
            } else {
                capture.cursor
            };
        }
        if let Some(id) = input_capture.filter(|id| self.input_enabled(*id)) {
            return self
                .input_pointer_cursor(id, position)
                .unwrap_or_else(|| self.owner_cursor(id));
        }
        let Some(point) = position else {
            return Cursor::Auto;
        };
        if self.scrollbar_at(point).is_some() {
            return Cursor::Icon(winit::window::CursorIcon::Default);
        }
        self.input_at(point)
            .and_then(|id| self.input_pointer_cursor(id, Some(point)))
            .unwrap_or_else(|| self.selection_cursor(point))
    }
}
