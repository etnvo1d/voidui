use super::{EventHandler, IntoEventHandler};
use crate::{core::event::*, tasks::TaskRuntime};

#[derive(Clone)]
struct Binding {
    kind: EventKind,
    source: Option<crate::core::reconcile::ComponentId>,
    handler: EventHandler<Event>,
}
#[derive(Default, Clone)]
struct BindingList {
    entries: Vec<Binding>,
}
/// Event descriptions stay outside CSS properties. Empty descriptions occupy one
/// optional pointer; only nodes with listeners allocate retained event storage.
#[derive(Default, Clone)]
pub struct EventBindings(Option<Box<BindingList>>);
impl EventBindings {
    pub fn is_empty(&self) -> bool {
        self.0.is_none()
    }
    pub(crate) fn has(&self, kind: EventKind) -> bool {
        self.0
            .as_ref()
            .is_some_and(|list| list.entries.iter().any(|b| b.kind == kind))
    }
    pub(crate) fn has_pointer(&self) -> bool {
        self.0
            .as_ref()
            .is_some_and(|list| list.entries.iter().any(|b| b.kind.is_pointer()))
    }
    fn set(&mut self, kind: EventKind, handler: EventHandler<Event>) {
        let list = self.0.get_or_insert_with(Default::default);
        if let Some(binding) = list.entries.iter_mut().find(|b| b.kind == kind) {
            binding.handler.replace(handler);
            return;
        }
        if list.entries.len() == list.entries.capacity() {
            list.entries.reserve_exact(list.entries.capacity().max(1));
        }
        list.entries.push(Binding {
            kind,
            source: None,
            handler,
        });
    }
    pub(crate) fn component_source(&mut self, source: crate::core::reconcile::ComponentId) {
        if let Some(list) = &mut self.0 {
            for binding in &mut list.entries {
                binding.source.get_or_insert(source);
            }
        }
    }
    pub(crate) fn append(&mut self, other: Self) {
        let Some(mut other) = other.0 else {
            return;
        };
        if let Some(list) = &mut self.0 {
            list.entries.append(&mut other.entries);
        } else {
            self.0 = Some(other);
        }
    }
    pub(crate) fn replace(&mut self, mut next: Self) {
        if let (Some(old), Some(new)) = (&mut self.0, &mut next.0) {
            for binding in &mut new.entries {
                if let Some(index) = old
                    .entries
                    .iter()
                    .position(|b| b.kind == binding.kind && b.source == binding.source)
                {
                    let mut previous = old.entries.remove(index);
                    // Match by kind and occurrence, independent of registration order.
                    // The incoming allocation is reused as the new retained table.
                    std::mem::swap(&mut previous.handler, &mut binding.handler);
                    binding.handler.replace(previous.handler);
                }
            }
        }
        *self = next;
    }
    pub(crate) fn dispatch(
        &mut self,
        kind: EventKind,
        event: &Event,
        runtime: &TaskRuntime,
    ) -> EventResponse {
        let mut response = EventResponse::CONTINUE;
        if let Some(list) = &mut self.0 {
            for binding in &mut list.entries {
                if binding.kind == kind {
                    response |= binding.handler.dispatch(event.clone(), runtime);
                }
            }
        }
        response
    }
}
fn mouse(event: Event) -> MouseEvent {
    let Event::Mouse(event) = event else {
        unreachable!("mouse event binding")
    };
    event
}
fn key(event: Event) -> KeyEvent {
    let Event::Key(event) = event else {
        unreachable!("key event binding")
    };
    event
}
fn drag(event: Event) -> DragEvent {
    let Event::Drag(event) = event else {
        unreachable!("drag event binding")
    };
    event
}
fn click(event: Event) -> ClickEvent {
    let Event::Click(event) = event else {
        unreachable!("click event binding")
    };
    event
}

macro_rules! bindings_methods {
    ($($name:ident, $kind:ident, $payload:ty, $extract:ident;)*) => { $(
        pub fn $name<M>(&mut self, callback: impl IntoEventHandler<$payload, M>) {
            self.set(EventKind::$kind, EventHandler::new(callback).map_input($extract));
        }
    )* };
}
impl EventBindings {
    bindings_methods! {
        on_click, Click, ClickEvent, click;
        on_mouse_enter, MouseEnter, MouseEvent, mouse;
        on_mouse_leave, MouseLeave, MouseEvent, mouse;
        on_mouse_down, MouseDown, MouseEvent, mouse;
        on_mouse_up, MouseUp, MouseEvent, mouse;
        on_mouse_move, MouseMove, MouseEvent, mouse;
        on_mouse_scroll, MouseScroll, MouseEvent, mouse;
        on_drag, Drag, DragEvent, drag;
        on_key_down, KeyDown, KeyEvent, key;
        on_key_up, KeyUp, KeyEvent, key;
    }
}

// All builders keep their concrete type, including after event registration.
// This single declaration supplies the same APIs to widgets, components, and Elements.
#[doc(hidden)]
#[macro_export]
macro_rules! __voidui_event_methods {
    () => {
        $crate::__voidui_event_methods!(@methods
            on_click, $crate::core::event::ClickEvent, "Handle pointer, keyboard, or programmatic activation.";
            on_mouse_enter, $crate::core::event::MouseEvent, "Handle entry into this node and its descendants; this event does not bubble.";
            on_mouse_leave, $crate::core::event::MouseEvent, "Handle exit from this node and its descendants; this event does not bubble.";
            on_mouse_down, $crate::core::event::MouseEvent, "Handle any native mouse button press. Synchronous responses can prevent defaults.";
            on_mouse_up, $crate::core::event::MouseEvent, "Handle mouse button release, including releases delivered to the drag capture owner.";
            on_mouse_move, $crate::core::event::MouseEvent, "Handle motion in logical coordinates. During dragging the capture owner is the target.";
            on_mouse_scroll, $crate::core::event::MouseEvent, "Handle wheel/trackpad deltas, preserving line or logical-pixel units.";
            on_drag, $crate::core::event::DragEvent, "Handle Start/Move/End/Cancel with automatic primary-button capture and owner cursor.";
            on_key_down, $crate::core::event::KeyEvent, "Handle key presses before editing, shortcuts, and other native defaults.";
            on_key_up, $crate::core::event::KeyEvent, "Handle key releases from the focused node, bubbling to its ancestors.";
        );
    };
    (@methods $($name:ident, $payload:ty, $doc:literal;)*) => { $(
        #[doc = $doc]
        pub fn $name<M>(mut self, callback: impl $crate::core::interaction::IntoEventHandler<$payload, M>) -> Self {
            self.events.$name(callback); self
        }
    )* };
}
pub(crate) use crate::__voidui_event_methods as event_methods;
