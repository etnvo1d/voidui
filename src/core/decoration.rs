//! Window chrome policy and semantic control areas, independent of the renderer.
//! Inspired by GPUI's TitlebarOptions, WindowButtonLayout and WindowControlArea.
#![doc = include_str!("../../docs/titlebar.md")]
use super::{
    geometry::{Point, Rect},
    state::Subscribers,
};
use std::{
    cell::RefCell,
    rc::{Rc, Weak},
    str::FromStr,
};

/// Who draws the titlebar. Custom keeps native macOS controls; None hides all chrome.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum WindowDecorations {
    #[default]
    System,
    Custom,
    None,
}

/// Custom titlebar policy. Distances are logical pixels, independent of monitor DPI.
#[derive(Debug, Clone, PartialEq)]
pub struct TitlebarOptions {
    pub height: f32,
    /// Leave None to keep AppKit's button placement. Ignored outside macOS.
    pub traffic_light_position: Option<Point<f32>>,
    /// None follows the desktop preference when available. Ignored on macOS.
    pub button_layout: Option<WindowButtonLayout>,
    /// Client-side resize hit margin for Linux. Windows uses system frame metrics.
    pub resize_border: f32,
}
impl Default for TitlebarOptions {
    fn default() -> Self {
        Self {
            height: 36.0,
            traffic_light_position: None,
            button_layout: None,
            resize_border: 5.0,
        }
    }
}
impl TitlebarOptions {
    pub(crate) fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.height.is_finite() && self.height > 0.0,
            "titlebar height must be finite and positive"
        );
        anyhow::ensure!(
            self.resize_border.is_finite() && self.resize_border >= 0.0,
            "resize border must be finite and nonnegative"
        );
        if let Some(p) = self.traffic_light_position {
            anyhow::ensure!(
                p.x.is_finite() && p.y.is_finite() && p.x >= 0.0 && p.y >= 0.0,
                "traffic light position must be finite and nonnegative"
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowButton {
    Minimize,
    Maximize,
    Close,
}
impl WindowButton {
    pub fn name(self) -> &'static str {
        match self {
            Self::Minimize => "minimize",
            Self::Maximize => "maximize",
            Self::Close => "close",
        }
    }
    pub fn area(self) -> WindowControlArea {
        match self {
            Self::Minimize => WindowControlArea::Min,
            Self::Maximize => WindowControlArea::Max,
            Self::Close => WindowControlArea::Close,
        }
    }
}

/// Ordered button lists. Unlike a platform-wide left/right flag, this also permits
/// missing buttons and split layouts. Each action may occur only once when parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowButtonLayout {
    pub left: Vec<WindowButton>,
    pub right: Vec<WindowButton>,
}
impl Default for WindowButtonLayout {
    fn default() -> Self {
        Self {
            left: vec![],
            right: vec![
                WindowButton::Minimize,
                WindowButton::Maximize,
                WindowButton::Close,
            ],
        }
    }
}
impl FromStr for WindowButtonLayout {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (left, right) = value
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("button layout must contain one ':'"))?;
        anyhow::ensure!(!right.contains(':'), "button layout must contain one ':'");
        let mut seen = Vec::new();
        let mut parse = |side: &str| -> anyhow::Result<Vec<WindowButton>> {
            let mut result = Vec::new();
            for name in side.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                let button = match name {
                    "minimize" => WindowButton::Minimize,
                    "maximize" => WindowButton::Maximize,
                    "close" => WindowButton::Close,
                    // Desktop layouts may include menu/icon slots which are not buttons.
                    "menu" | "appmenu" | "icon" => continue,
                    _ => anyhow::bail!("unknown window button: {name}"),
                };
                anyhow::ensure!(!seen.contains(&button), "duplicate window button: {name}");
                seen.push(button);
                result.push(button);
            }
            Ok(result)
        };
        Ok(Self {
            left: parse(left)?,
            right: parse(right)?,
        })
    }
}

/// Assign a native role to a laid-out element. Client stops inherited dragging,
/// for example around a search field or an application toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowControlArea {
    Client,
    Drag,
    Min,
    Max,
    Close,
}
impl WindowControlArea {
    pub(crate) fn action(self) -> Option<WindowAction> {
        match self {
            Self::Min => Some(WindowAction::Minimize),
            Self::Max => Some(WindowAction::ToggleMaximize),
            Self::Close => Some(WindowAction::Close),
            _ => None,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowAction {
    Minimize,
    ToggleMaximize,
    Close,
}

/// Observed native state. Controls subscribe to changes without polling or redrawing
/// idle windows. Native button bounds include the actual occupied logical rectangle.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowState {
    pub decorations: WindowDecorations,
    pub titlebar: TitlebarOptions,
    pub button_layout: WindowButtonLayout,
    pub native_controls: Option<Rect<f32>>,
    pub maximized: bool,
    pub fullscreen: bool,
    pub focused: bool,
    pub minimizable: bool,
    pub resizable: bool,
}
impl Default for WindowState {
    fn default() -> Self {
        Self {
            decorations: WindowDecorations::System,
            titlebar: Default::default(),
            button_layout: Default::default(),
            native_controls: None,
            maximized: false,
            fullscreen: false,
            focused: true,
            minimizable: true,
            resizable: true,
        }
    }
}
struct WindowData {
    state: RefCell<WindowState>,
    readers: RefCell<Subscribers>,
    actions: RefCell<Vec<WindowAction>>,
    wake: RefCell<Option<Rc<dyn Fn()>>>,
}
#[derive(Clone)]
pub(crate) struct WindowHost(Rc<WindowData>);
impl Default for WindowHost {
    fn default() -> Self {
        Self(Rc::new(WindowData {
            state: Default::default(),
            readers: Default::default(),
            actions: Default::default(),
            wake: Default::default(),
        }))
    }
}
impl WindowHost {
    pub(crate) fn is_custom(&self) -> bool {
        self.0.state.borrow().decorations == WindowDecorations::Custom
    }
    pub(crate) fn context(&self) -> WindowContext {
        WindowContext(Rc::downgrade(&self.0))
    }
    pub(crate) fn state(&self) -> WindowState {
        self.0.state.borrow().clone()
    }
    pub(crate) fn update(&self, next: WindowState) {
        if *self.0.state.borrow() != next {
            *self.0.state.borrow_mut() = next;
            self.0.readers.borrow_mut().notify();
        }
    }
    pub(crate) fn set_waker(&self, wake: Rc<dyn Fn()>) {
        *self.0.wake.borrow_mut() = Some(wake);
    }
    pub(crate) fn take_actions(&self) -> Vec<WindowAction> {
        std::mem::take(&mut *self.0.actions.borrow_mut())
    }
}

/// Weak main-thread window handle. Safe to retain in callbacks after window closure.
#[derive(Clone)]
pub struct WindowContext(Weak<WindowData>);
impl WindowContext {
    /// Reading during a component render subscribes that component to native changes.
    pub fn state(&self) -> Option<WindowState> {
        let data = self.0.upgrade()?;
        data.readers.borrow_mut().track();
        Some(data.state.borrow().clone())
    }
    /// Queue an operation for the host after event dispatch; never reenter the tree
    /// from an OS callback. Returns false when the containing tree has been dropped.
    pub fn request(&self, action: WindowAction) -> bool {
        let Some(data) = self.0.upgrade() else {
            return false;
        };
        data.actions.borrow_mut().push(action);
        if let Some(wake) = data.wake.borrow().as_ref() {
            wake();
        }
        true
    }
}
/// Access the containing window inside a component, including headless test trees.
pub fn window_context() -> WindowContext {
    super::state::window_context()
}

impl super::widget_tree::WidgetTree {
    pub fn window_context(&self) -> WindowContext {
        self.window_host.context()
    }
    /// Supply native state in a custom/headless host. Only subscribed components update.
    pub fn set_window_state(&mut self, state: WindowState) {
        self.window_host.update(state);
    }
    pub fn take_window_actions(&mut self) -> Vec<WindowAction> {
        self.window_host.take_actions()
    }
}
