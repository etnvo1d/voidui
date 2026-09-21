//! Typed event bindings, bubbling, and pointer capture over the retained tree.
#![doc = include_str!("../../docs/events.md")]
mod bindings;
mod handler;
mod pointer;
pub use bindings::EventBindings;
pub(crate) use bindings::event_methods;
pub(crate) use handler::mode;
pub use handler::{EventHandler, EventOutput, IntoEventHandler};

use super::{event::*, geometry::Point, widget::WidgetId, widget_tree::WidgetTree};
use std::collections::HashMap;

#[derive(Default)]
pub(crate) struct EventState {
    bindings: HashMap<WidgetId, EventBindings>,
    pointer_listeners: usize,
    pub custom_listeners: usize,
    pointer: pointer::PointerState,
}
impl EventState {
    fn install(&mut self, id: WidgetId, next: EventBindings) {
        if let Some(old) = self.bindings.get_mut(&id) {
            self.pointer_listeners -= usize::from(old.has_pointer());
            self.pointer_listeners += usize::from(next.has_pointer());
            if next.is_empty() {
                self.bindings.remove(&id);
            } else {
                old.replace(next);
            }
        } else if !next.is_empty() {
            self.pointer_listeners += usize::from(next.has_pointer());
            self.bindings.insert(id, next);
        }
    }
    pub(crate) fn remove(&mut self, id: WidgetId) {
        if let Some(bindings) = self.bindings.remove(&id) {
            self.pointer_listeners -= usize::from(bindings.has_pointer());
        }
    }
    fn has(&self, id: WidgetId, kind: EventKind) -> bool {
        self.bindings.get(&id).is_some_and(|b| b.has(kind))
    }
}

impl WidgetTree {
    pub(crate) fn install_events(&mut self, id: WidgetId, next: EventBindings) {
        if self.events.pointer.capture.is_some_and(|c| c.owner == id) && !next.has(EventKind::Drag)
        {
            self.cancel_pointer_capture();
        }
        self.events.install(id, next);
    }
    pub(crate) fn event_enabled(&self, id: WidgetId) -> bool {
        if !self.is_rendered(id) || self.is_inert(id) {
            return false;
        }
        let Some(node) = self.nodes.get(id) else {
            return false;
        };
        if node.visual.as_ref().is_some_and(|v| v.inverse.is_none()) {
            return false;
        }
        if node.computed.layer.visibility != crate::style::layer::Visibility::Visible {
            return false;
        }
        let mut node = Some(id);
        while let Some(id) = node {
            let Some(current) = self.nodes.get(id) else {
                return false;
            };
            if current.props.attributes.contains_key("disabled") {
                return false;
            }
            node = current.parent;
        }
        true
    }
    fn event_at(&self, point: Point<f32>) -> Option<WidgetId> {
        match self.hit_test(point)? {
            super::top_layer::HitTarget::Element(id) if self.event_enabled(id) => Some(id),
            _ => None,
        }
    }
    fn dispatch_at(&mut self, id: WidgetId, kind: EventKind, event: &mut Event) -> EventResponse {
        let Some(node) = self.nodes.get(id) else {
            return EventResponse::CONTINUE;
        };
        event.retarget(
            id,
            node.global_bounds.origin,
            node.visual
                .as_ref()
                .and_then(|v| v.inverse)
                .unwrap_or_default(),
        );
        let runtime = self.tasks.runtime();
        let mut response = self
            .events
            .bindings
            .get_mut(&id)
            .map_or(EventResponse::CONTINUE, |b| {
                b.dispatch(kind, event, runtime)
            });
        if let Some(widget) = self.nodes.get_mut(id).and_then(|n| n.widget.as_mut()) {
            if widget.accepts_events() {
                response |= widget.on_event_with_tasks(event, runtime);
            }
            if kind == EventKind::Click && widget.accepts_click() && !response.prevent_default {
                widget.on_click_with_tasks(runtime);
            }
        }
        // Editors host independent widget trees. Offer wheels at the hovered
        // position before applying this tree's retained scrolling default.
        if !response.prevent_default
            && self.pointer_capture().is_none()
            && let Event::Mouse(mouse) = event
            && mouse.state == MouseEventType::Scrolled
            && let Some(position) = self.window_to_layout(id, mouse.position)
        {
            let bounds = self.content_bounds(id);
            if let Some(input) = self
                .nodes
                .get_mut(id)
                .and_then(|node| node.widget.as_mut())
                .and_then(|widget| widget.text_input_mut())
            {
                response |= input.mouse_scroll(&MouseEvent { position, ..*mouse }, bounds);
            }
        }
        response
    }
    fn bubble(&mut self, target: WidgetId, mut event: Event) -> EventResponse {
        if !self.event_enabled(target) {
            return EventResponse::CONTINUE;
        }
        let Some(kind) = event.kind() else {
            return EventResponse::CONTINUE;
        };
        let mut current = Some(target);
        let mut response = EventResponse::CONTINUE;
        while let Some(id) = current {
            current = self.nodes.get(id).and_then(|node| node.parent);
            let current_response = self.dispatch_at(id, kind, &mut event);
            response |= current_response;
            if let Event::Mouse(mouse) = &mut event
                && mouse.state == MouseEventType::Scrolled
            {
                mouse.wheel = current_response.remaining_scroll(mouse.wheel);
            }
            if response.stop_propagation {
                break;
            }
        }
        response
    }
    fn click_target(&self, mut id: WidgetId) -> Option<WidgetId> {
        if !self.event_enabled(id) {
            return None;
        }
        loop {
            let node = self.nodes.get(id)?;
            if node
                .props
                .window_control_area
                .is_some_and(|area| area.action().is_some())
                || self.events.has(id, EventKind::Click)
                || node.widget.as_ref().is_some_and(|w| w.accepts_click())
            {
                return Some(id);
            }
            id = node.parent?;
        }
    }
    /// Programmatic activation uses the same bubbling path as pointer/keyboard
    /// clicks. Unlike hit testing, it is also available before initial layout.
    pub fn click(&mut self, id: WidgetId) -> bool {
        self.click_with_source(id, ClickSource::Programmatic)
            .is_some()
    }
    pub(crate) fn click_with_source(
        &mut self,
        id: WidgetId,
        source: ClickSource,
    ) -> Option<EventResponse> {
        self.click_target(id)?;
        let event = ClickEvent {
            target: id,
            current_target: id,
            source,
            modifiers: self.events.pointer.modifiers,
            position: (source == ClickSource::Pointer)
                .then_some(self.events.pointer.last_position)
                .flatten(),
        };
        let response = self.bubble(id, Event::Click(event));
        if !response.prevent_default {
            if let Some(action) = self.window_control_for(id).1.action() {
                self.window_context().request(action);
            }
        }
        Some(response)
    }
    /// Dispatch a logical key from the focused element, or the active modal/root
    /// when no element has focus. Call before text editing and native defaults.
    pub fn dispatch_key(
        &mut self,
        key: Key,
        state: KeyEventType,
        modifiers: ModifiersState,
        repeat: bool,
    ) -> EventResponse {
        self.events.pointer.modifiers = modifiers;
        let Some(target) = self.focused().or_else(|| self.active_modal()).or(self.root) else {
            return EventResponse::CONTINUE;
        };
        self.bubble(
            target,
            Event::Key(KeyEvent {
                target,
                current_target: target,
                key,
                state,
                modifiers,
                repeat,
            }),
        )
    }
}

impl WidgetTree {
    /// Resolve from the frontmost target, then walk ancestors. Explicit Client
    /// regions and ordinary interactive controls stop inherited titlebar dragging.
    pub fn window_control_at(&self, point: Point<f32>) -> super::decoration::WindowControlArea {
        let mut area = super::decoration::WindowControlArea::Client;
        self.visit_hit_regions(|region| {
            if !region.contains(point) {
                return true;
            }
            area = self.window_control_region(region);
            false
        });
        area
    }
    pub(crate) fn window_control_region(
        &self,
        region: super::stacking::HitRegion,
    ) -> super::decoration::WindowControlArea {
        use super::{decoration::WindowControlArea as Area, stacking::Phase, top_layer::HitTarget};
        match (region.phase, region.target) {
            (Phase::Scrollbar | Phase::Backdrop, _) => Area::Client,
            (_, HitTarget::Element(id)) => self.window_control_for(id).1,
            _ => Area::Client,
        }
    }
    pub(crate) fn window_control_for(
        &self,
        mut id: WidgetId,
    ) -> (WidgetId, super::decoration::WindowControlArea) {
        use super::decoration::WindowControlArea as Area;
        if !self.event_enabled(id) {
            return (id, Area::Client);
        }
        loop {
            let node = &self.nodes[id];
            if let Some(area) = node.props.window_control_area {
                return (id, area);
            }
            let interactive = self.events.bindings.get(&id).is_some_and(|b| !b.is_empty())
                || node.props.attributes.contains_key("tabindex")
                || node.widget.as_ref().is_some_and(|w| {
                    w.accepts_click() || w.accepts_events() || w.text_input().is_some()
                });
            if interactive {
                return (id, Area::Client);
            }
            // A top-layer popup is an independent interaction surface even if
            // its DOM parent is a draggable titlebar.
            if node.top_layer.is_some() {
                return (id, Area::Client);
            }
            let Some(parent) = node.parent else {
                return (id, Area::Client);
            };
            id = parent;
        }
    }
}
