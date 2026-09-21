//! Platform-independent text input routing. Custom widgets can implement this
//! protocol without adopting VoidUI's editor document or selection types.
use super::{
    event::EventResult,
    geometry::{Point, Rect},
};
use crate::{
    render::TextLayoutCache,
    style::{
        selection::{Cursor, SelectionColors},
        text::TextStyle,
    },
};
use std::{ops::Range, time::Instant};
use winit::keyboard::{Key, ModifiersState};

#[derive(Debug, Clone)]
pub struct KeyInput {
    pub key: Key,
    pub modifiers: ModifiersState,
    pub repeat: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerPhase {
    Down,
    Move,
    Up,
    Cancel,
}
#[derive(Debug, Clone)]
pub enum InputEvent {
    Key(KeyInput),
    /// Committed text is separate from physical/logical key commands.
    Text(String),
    Preedit(String, Option<(usize, usize)>),
    Commit(String),
    CancelComposition,
    Pointer {
        phase: PointerPhase,
        position: Point<f32>,
        modifiers: ModifiersState,
        clicks: u8,
    },
    Scroll(Point<f32>),
    Cut,
    Paste(String),
}

pub struct InputContext<'a> {
    pub text_layout: &'a TextLayoutCache,
    pub bounds: Rect<f32>,
    pub style: &'a TextStyle,
    pub readonly: bool,
    pub now: Instant,
}
/// UTF-8 byte offsets address the platform-visible text, including preedit.
/// Native adapters that require UTF-16 must
/// convert explicitly; callers never confuse byte, scalar, and grapheme indices.
pub trait TextInputClient {
    /// Resolve the pointer over content hosted inside this control. Positions and
    /// bounds use layout coordinates, as for input events. `None` positions allow
    /// captured gestures to keep their cursor outside the window. Return `None`
    /// to use the control's CSS cursor. Queries must not dispatch input events.
    fn pointer_cursor(&self, _position: Option<Point<f32>>, _bounds: Rect<f32>) -> Option<Cursor> {
        None
    }
    /// Offer native wheel events to embedded content before the host scrolls.
    /// Positions and bounds use layout coordinates; wheel units remain unchanged.
    /// Returning PREVENT_DEFAULT prevents the host from scrolling the same delta.
    fn mouse_scroll(
        &mut self,
        _event: &super::event::MouseEvent,
        _bounds: Rect<f32>,
    ) -> super::event::EventResponse {
        super::event::EventResponse::CONTINUE
    }
    fn attributes_changed(
        &mut self,
        _attributes: &std::collections::BTreeMap<
            winit::keyboard::SmolStr,
            winit::keyboard::SmolStr,
        >,
    ) {
    }
    /// Expose retained editor content to the shared scrollbar/scroll-chaining host.
    /// Return logical content size (excluding CSS padding) and the current offset.
    /// Returning None keeps legacy custom clients on their own scrolling path.
    fn scroll_content(
        &mut self,
        _bounds: Rect<f32>,
        _cache: &TextLayoutCache,
    ) -> Option<super::scroll::ScrollContent> {
        None
    }
    /// Apply the host's already-clamped offset without moving the caret.
    fn set_scroll_offset(&mut self, _offset: Point<f32>) {}
    fn handle_input(&mut self, event: &InputEvent, cx: InputContext<'_>) -> EventResult;
    fn selected_text(&self) -> String;
    fn selection_range(&self) -> Range<usize>;
    fn text_for_range(&self, range: Range<usize>) -> Option<String>;
    fn marked_range(&self) -> Option<Range<usize>> {
        None
    }
    fn bounds_for_range(&mut self, range: Range<usize>, cx: InputContext<'_>) -> Option<Rect<f32>>;
    fn focus_changed(&mut self, focused: bool, now: Instant);
    fn is_composing(&self) -> bool {
        self.marked_range().is_some()
    }
    fn is_empty(&self) -> bool;
    /// Install cascaded typography and report whether it actually changed. The
    /// `::placeholder` and `::selection` pseudo-elements live outside the node's
    /// computed style, so only this answer can invalidate the retained scene.
    fn update_presentation(
        &mut self,
        _style: &TextStyle,
        _placeholder: TextStyle,
        _selection: SelectionColors,
    ) -> bool {
        false
    }
    fn next_frame(&self, _now: Instant) -> Option<Instant> {
        None
    }
    fn tick(&mut self, _now: Instant) {}
}

use super::{widget::WidgetId, widget_tree::WidgetTree};
impl WidgetTree {
    /// Query a text client's hosted content using the same coordinate conversion
    /// as native input. Disabled, removed and inert clients cannot supply cursors.
    pub(crate) fn input_pointer_cursor(
        &self,
        id: WidgetId,
        position: Option<Point<f32>>,
    ) -> Option<Cursor> {
        if !self.input_enabled(id) {
            return None;
        }
        let position = match position {
            Some(point) => Some(self.window_to_layout(id, point)?),
            None => None,
        };
        self.text_input(id)?
            .pointer_cursor(position, self.content_bounds(id))
    }
    pub fn text_input(&self, id: WidgetId) -> Option<&dyn TextInputClient> {
        self.nodes.get(id)?.widget.as_ref()?.text_input()
    }
    pub fn dispatch_input(
        &mut self,
        id: WidgetId,
        event: &InputEvent,
        cache: &TextLayoutCache,
    ) -> EventResult {
        self.flush_updates();
        if !self.input_enabled(id) {
            return EventResult::Unhandled;
        }
        let local_event;
        let event = if let InputEvent::Pointer {
            phase,
            position,
            modifiers,
            clicks,
        } = event
        {
            let Some(position) = self.window_to_layout(id, *position) else {
                return EventResult::Unhandled;
            };
            local_event = InputEvent::Pointer {
                phase: *phase,
                position,
                modifiers: *modifiers,
                clicks: *clicks,
            };
            &local_event
        } else {
            event
        };
        let bounds = self.content_bounds(id);
        let readonly = self.nodes[id].props.attributes.contains_key("readonly");
        let node = &mut self.nodes[id];
        let Some(input) = node.widget.as_mut().and_then(|w| w.text_input_mut()) else {
            return EventResult::Unhandled;
        };
        let result = input.handle_input(
            event,
            InputContext {
                text_layout: cache,
                bounds,
                readonly,
                style: &node.computed.text,
                now: Instant::now(),
            },
        );
        self.refresh_input_scroll(id, cache);
        result
    }
    pub fn input_bounds_for_range(
        &mut self,
        id: WidgetId,
        range: Range<usize>,
        cache: &TextLayoutCache,
    ) -> Option<Rect<f32>> {
        if !self.input_enabled(id) {
            return None;
        }
        let bounds = self.content_bounds(id);
        let readonly = self.nodes[id].props.attributes.contains_key("readonly");
        let node = &mut self.nodes[id];
        let rect = node.widget.as_mut()?.text_input_mut()?.bounds_for_range(
            range,
            InputContext {
                text_layout: cache,
                bounds,
                readonly,
                style: &node.computed.text,
                now: Instant::now(),
            },
        )?;
        Some(self.layout_rect_to_window(id, rect))
    }
    pub(crate) fn input_enabled(&self, id: WidgetId) -> bool {
        !self.is_inert(id)
            && self.is_rendered(id)
            && self.nodes.get(id).is_some_and(|n| {
                !n.props.attributes.contains_key("disabled")
                    && n.computed.layout.display != super::layout::Display::None
                    && n.computed.layer.visibility == crate::style::layer::Visibility::Visible
            })
    }
    pub(crate) fn input_focus_changed(&mut self, id: WidgetId, focused: bool) {
        if let Some(input) = self
            .nodes
            .get_mut(id)
            .and_then(|n| n.widget.as_mut())
            .and_then(|w| w.text_input_mut())
        {
            input.focus_changed(focused, Instant::now());
        }
    }
    pub(crate) fn input_at(&self, point: Point<f32>) -> Option<WidgetId> {
        let super::top_layer::HitTarget::Element(mut id) = self.hit_test(point)? else {
            return None;
        };
        loop {
            if !self.input_enabled(id) {
                return None;
            }
            let node = &self.nodes[id];
            let widget = node.widget.as_ref()?;
            if widget.text_input().is_some() {
                return Some(id);
            }
            // Interactive addons keep their own focus and pointer behavior.
            if widget.accepts_click()
                || node.props.attributes.contains_key("tabindex")
                || node.props.tag == "button"
            {
                return None;
            }
            if widget.delegates_focus() {
                return self.first_input(id);
            }
            id = node.parent?;
        }
    }
    fn first_input(&self, id: WidgetId) -> Option<WidgetId> {
        if !self.input_enabled(id) {
            return None;
        }
        if self.text_input(id).is_some() {
            return Some(id);
        }
        self.nodes[id]
            .children
            .iter()
            .find_map(|id| self.first_input(*id))
    }
    /// Earliest requested caret or captured-selection update for custom hosts.
    pub fn next_input_frame(&self, now: Instant) -> Option<Instant> {
        let id = self.focused()?;
        self.input_enabled(id)
            .then(|| self.text_input(id)?.next_frame(now))
            .flatten()
    }
    /// Advance focused input animations and edge scrolling at the supplied time.
    pub fn tick_input(&mut self, now: Instant) {
        if let Some(id) = self.focused().filter(|id| self.input_enabled(*id)) {
            if let Some(input) = self.nodes[id]
                .widget
                .as_mut()
                .and_then(|w| w.text_input_mut())
            {
                input.tick(now);
            }
        }
    }
}
