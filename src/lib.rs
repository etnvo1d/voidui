extern crate self as voidui;

pub mod core;
pub mod style;
pub mod widgets;

pub use core::rich_text::{Inline, InlineStyle, RichText, StyleSpan, StyleSpanBuilder, span};
#[cfg(feature = "editing")]
pub use editing::Editor;
pub use voidui_gpui_wgpu as render;
pub use widgets::{VirtualListOptions, asset_img, div, img, rich_text, text, virtual_list};
#[cfg(feature = "editing")]
pub use widgets::{input, input_group, rich_editor, textarea};

pub(crate) mod platform;
/// The running application's system clipboard, shared by its windows and opened
/// on first use. Event callbacks and components can read and write it directly;
/// `AppWindow` keeps its own shortcut and document-selection helpers.
pub mod clipboard {
    pub use crate::platform::clipboard::{read_text, write_text};
}
pub use core::{
    application::Application,
    window::{AppWindow, FrameStats, WindowOptions},
};

pub use core::{
    component::{AsyncCallback, Callback, ComponentElement, component},
    state::{State, state},
};
pub use voidui_macros::component;
#[doc(hidden)]
pub use voidui_macros::filter_style_methods as __voidui_filter_style_methods;

pub use core::element::{Element, IntoElement};

pub mod cache;
pub mod files;
pub mod tasks;
pub use core::task_hooks::{app_task_scope, on_mount, task_scope, window_task_scope};
pub use tasks::{OwnedTaskScope, Task, TaskError, TaskOptions, TaskRuntime, TaskScope};

#[cfg(feature = "editing")]
pub mod editing;

pub mod media;

pub mod svg;
pub use svg::{SvgDocument, svg, svg_from_str};

pub use core::event::{
    ClickEvent, ClickSource, DragEvent, DragPhase, EventDispatch, EventKind, EventResponse,
    KeyEvent, KeyEventType, MouseButton, MouseButtons, MouseEvent, MouseEventType, MouseWheel,
    WheelUnit,
};
pub use core::interaction::{EventBindings, EventHandler, IntoEventHandler};

pub use core::scroll::{
    ScrollAxis, ScrollContent, ScrollMetrics, ScrollOptions, ScrollbarGeometry, ScrollbarMode,
};
pub use style::scroll::{
    Overflow, OverscrollBehavior, ScrollbarColors, ScrollbarGutter, ScrollbarWidth,
};

pub use core::data::{List, Read};
pub use core::resource::{Resource, ResourceError, resource};
pub use core::store::{Selection, Store, selection, store};

pub use core::decoration::{
    TitlebarOptions, WindowAction, WindowButton, WindowButtonLayout, WindowContext,
    WindowControlArea, WindowDecorations, WindowState, window_context,
};
pub use widgets::title_bar::title_bar;

pub use core::children::{Children, IntoChild};
pub use core::environment::{provide_context, try_context, use_context};

pub use style::transform::{Transform, TransformFunction, TransformLength, TransformOrigin};
