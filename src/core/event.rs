//! Owned event snapshots. Coordinates use logical window pixels; local_position
//! is relative to the current receiver's border box at the time of dispatch.
pub use super::keycode::MouseButton;
use super::{geometry::Point, widget::WidgetId};
pub use winit::keyboard::{Key, ModifiersState, NamedKey};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyEventType {
    Pressed,
    Released,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseEventType {
    Entered,
    Left,
    Pressed,
    Released,
    Moved,
    Scrolled,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WheelUnit {
    Lines,
    #[default]
    Pixels,
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MouseWheel {
    pub x: f32,
    pub y: f32,
    pub unit: WheelUnit,
}

bitflags::bitflags! {
    /// Held standard buttons. Additional native buttons are identified by the
    /// MouseEvent.button field on their down/up events.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct MouseButtons: u8 {
        const LEFT = 1;
        const RIGHT = 1 << 1;
        const MIDDLE = 1 << 2;
        const BACK = 1 << 3;
        const FORWARD = 1 << 4;
    }
}
impl MouseButtons {
    pub(crate) fn flag(button: MouseButton) -> Self {
        match button {
            MouseButton::Left => Self::LEFT,
            MouseButton::Right => Self::RIGHT,
            MouseButton::Middle => Self::MIDDLE,
            MouseButton::Back => Self::BACK,
            MouseButton::Forward => Self::FORWARD,
            MouseButton::Other(_) => Self::empty(),
        }
    }
}
impl From<winit::event::MouseButton> for MouseButton {
    fn from(button: winit::event::MouseButton) -> Self {
        use winit::event::MouseButton as B;
        match button {
            B::Left => Self::Left,
            B::Right => Self::Right,
            B::Middle => Self::Middle,
            B::Back => Self::Back,
            B::Forward => Self::Forward,
            B::Other(n) => Self::Other(n),
        }
    }
}

#[derive(Debug, Clone)]
pub struct KeyEvent {
    pub target: WidgetId,
    pub current_target: WidgetId,
    pub key: Key,
    pub repeat: bool,
    pub modifiers: ModifiersState,
    pub state: KeyEventType,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MouseEvent {
    pub target: WidgetId,
    pub current_target: WidgetId,
    pub related_target: Option<WidgetId>,
    pub button: Option<MouseButton>,
    pub buttons: MouseButtons,
    pub modifiers: ModifiersState,
    pub position: Point<f32>,
    pub local_position: Point<f32>,
    pub wheel: MouseWheel,
    pub state: MouseEventType,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickSource {
    Pointer,
    Keyboard,
    Programmatic,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClickEvent {
    pub target: WidgetId,
    pub current_target: WidgetId,
    pub source: ClickSource,
    pub position: Option<Point<f32>>,
    pub modifiers: ModifiersState,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragPhase {
    Start,
    Move,
    End,
    Cancel,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DragEvent {
    /// The nearest enabled drag handler chosen on the primary button press.
    pub target: WidgetId,
    pub current_target: WidgetId,
    pub phase: DragPhase,
    pub button: MouseButton,
    pub modifiers: ModifiersState,
    pub origin: Point<f32>,
    pub position: Point<f32>,
    pub local_position: Point<f32>,
    /// Movement since the previous sample, in logical window pixels.
    pub delta: Point<f32>,
    pub total_delta: Point<f32>,
}

#[derive(Debug, Clone)]
pub enum ImeEvent {
    Preedit(String, Option<(usize, usize)>),
    Commit(String),
}
#[derive(Debug, Clone)]
pub enum ClipboardEvent {
    Copy,
    Cut,
    Paste(String),
}
#[derive(Debug, Clone)]
pub enum Event {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Click(ClickEvent),
    Drag(DragEvent),
    Ime(ImeEvent),
    Clipboard(ClipboardEvent),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventResult {
    Handled,
    Unhandled,
}

bitflags::bitflags! {
    /// Wheel axes consumed by a nested control, in the original event's axes.
    /// Shift-wheel conversion happens after unused axes reach the scroll host.
    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
    pub struct ScrollAxes: u8 {
        const HORIZONTAL = 1;
        const VERTICAL = 2;
    }
}

/// Only synchronous handlers may decide event propagation and native defaults.
/// Combine independent decisions with |; () is equivalent to CONTINUE.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct EventResponse {
    pub prevent_default: bool,
    pub stop_propagation: bool,
    pub consumed_scroll: ScrollAxes,
}
impl EventResponse {
    /// Consume only these wheel axes, leaving other movement to ancestor scrollers.
    pub fn consume_scroll(axes: ScrollAxes) -> Self {
        Self {
            consumed_scroll: axes,
            ..Self::CONTINUE
        }
    }
    pub fn remaining_scroll(self, mut wheel: MouseWheel) -> MouseWheel {
        if self.consumed_scroll.contains(ScrollAxes::HORIZONTAL) {
            wheel.x = 0.0;
        }
        if self.consumed_scroll.contains(ScrollAxes::VERTICAL) {
            wheel.y = 0.0;
        }
        wheel
    }

    pub const CONTINUE: Self = Self {
        prevent_default: false,
        stop_propagation: false,
        consumed_scroll: ScrollAxes::empty(),
    };
    pub const PREVENT_DEFAULT: Self = Self {
        prevent_default: true,
        stop_propagation: false,
        consumed_scroll: ScrollAxes::empty(),
    };
    pub const STOP_PROPAGATION: Self = Self {
        prevent_default: false,
        stop_propagation: true,
        consumed_scroll: ScrollAxes::empty(),
    };
    pub const HANDLED: Self = Self {
        prevent_default: true,
        stop_propagation: true,
        consumed_scroll: ScrollAxes::empty(),
    };
}
impl std::ops::BitOr for EventResponse {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self {
            prevent_default: self.prevent_default || rhs.prevent_default,
            stop_propagation: self.stop_propagation || rhs.stop_propagation,
            consumed_scroll: self.consumed_scroll | rhs.consumed_scroll,
        }
    }
}
impl std::ops::BitOrAssign for EventResponse {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = *self | rhs;
    }
}
impl From<EventResult> for EventResponse {
    fn from(result: EventResult) -> Self {
        match result {
            EventResult::Handled => Self::HANDLED,
            EventResult::Unhandled => Self::CONTINUE,
        }
    }
}

/// Native/headless adapters use this to run defaults and request a frame only
/// when CSS state or focus changed. Callback state writes schedule themselves.
#[derive(Debug, Default, Clone, Copy)]
pub struct EventDispatch {
    pub response: EventResponse,
    pub changed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventKind {
    Click,
    MouseEnter,
    MouseLeave,
    MouseDown,
    MouseUp,
    MouseMove,
    MouseScroll,
    Drag,
    KeyDown,
    KeyUp,
}
impl EventKind {
    pub(crate) fn is_pointer(self) -> bool {
        !matches!(self, Self::KeyDown | Self::KeyUp)
    }
}
impl Event {
    pub fn kind(&self) -> Option<EventKind> {
        Some(match self {
            Self::Click(_) => EventKind::Click,
            Self::Drag(_) => EventKind::Drag,
            Self::Key(event) => match event.state {
                KeyEventType::Pressed => EventKind::KeyDown,
                KeyEventType::Released => EventKind::KeyUp,
            },
            Self::Mouse(event) => match event.state {
                MouseEventType::Entered => EventKind::MouseEnter,
                MouseEventType::Left => EventKind::MouseLeave,
                MouseEventType::Pressed => EventKind::MouseDown,
                MouseEventType::Released => EventKind::MouseUp,
                MouseEventType::Moved => EventKind::MouseMove,
                MouseEventType::Scrolled => EventKind::MouseScroll,
            },
            _ => return None,
        })
    }
    pub(crate) fn retarget(
        &mut self,
        current: WidgetId,
        origin: Point<f32>,
        inverse: crate::render::Affine,
    ) {
        match self {
            Self::Key(e) => e.current_target = current,
            Self::Click(e) => e.current_target = current,
            Self::Mouse(e) => {
                e.current_target = current;
                let [x, y] = inverse.map([e.position.x, e.position.y]);
                e.local_position = Point::new(x - origin.x, y - origin.y);
            }
            Self::Drag(e) => {
                e.current_target = current;
                let [x, y] = inverse.map([e.position.x, e.position.y]);
                e.local_position = Point::new(x - origin.x, y - origin.y);
            }
            _ => {}
        }
    }
}
