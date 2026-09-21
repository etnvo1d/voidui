# Custom titlebars

Set `WindowOptions::decorations` to `WindowDecorations::Custom`, then put a
`title_bar(content)` at the top of your window. Default windows retain their system
chrome. `WindowDecorations::None` creates an undecorated window without installing
custom-chrome adapters.

```rust,no_run
use voidui::{Application, WindowOptions, WindowDecorations, div, title_bar};

fn main() -> anyhow::Result<()> {
    Application::new()
        .window(
            WindowOptions {
                decorations: WindowDecorations::Custom,
                ..Default::default()
            },
            div()
                .width(voidui::style::pct(100.0))
                .height(voidui::style::pct(100.0))
                .flex_col()
                .child(title_bar("My application"))
                .child(div().padding(24.0).child("Window content")),
        )
        .run()
}
```

## Layout and appearance

`TitlebarOptions` configures height, optional macOS traffic-light position, optional
button layout, and the Linux client resize margin. Measurements are logical pixels.
The default custom titlebar is 36 pixels high. Native titlebar mode is selected at
window creation; changing it at runtime is not currently supported.

On macOS, AppKit owns the red/yellow/green controls. `traffic_light_position: None`
retains the system position. Set `Some(Point::new(x, y))` to move the native group.
The titlebar reserves space from the measured group bounds; the group is not
painted with SVGs. AppKit supplies fullscreen and alternate-button behavior.

On Windows and Linux, controls are ordinary elements with font-independent SVG
icons. Style `.title-bar`, `.title-bar-content`, `.window-controls`,
`.window-control`, and `[data-window-button="close"]` with normal CSS. Hover,
active, focus and colors are application styles; the framework does not inject a
hard-coded theme. The example includes a complete stylesheet. The default button
width matches the configured bar height and may be overridden by CSS.

On Windows/Linux, set `TitlebarOptions::button_layout` to a parsed layout:

```rust
let layout: voidui::WindowButtonLayout = "close:minimize,maximize".parse().unwrap();
assert_eq!(layout.left, [voidui::WindowButton::Close]);
```

The colon separates left and right lists. `":"` deliberately hides all buttons.
Parsing rejects duplicate or unknown action names; desktop menu/icon slots are
ignored because they are not window buttons. Explicit lists are also available
through `WindowButtonLayout { left, right }`.

On Linux, an absent override follows the GNOME `button-layout` setting exported
through XDG Desktop Portal and observes updates. Desktop environments that do not
export this key use the default right-side minimize/maximize/close layout. Only
custom windows without an override start the scoped D-Bus tasks. There is no
settings polling. The app can supply a layout for other desktop environments.

## Interactive regions and actions

Use `.window_control_area(WindowControlArea::Drag)` on any element to designate a
drag region. Nested labels inherit the role. Ordinary event handlers, focusable
controls and text inputs stop inherited dragging. An explicit
`WindowControlArea::Client` excludes a whole application-controlled subtree.
`Min`, `Max`, and `Close` assign window-button semantics to custom elements.
Add `tabindex="0"` to your own controls for keyboard focus; built-in controls have it.

The hit regions use the same paint order, clipping, pointer-events, visibility and
modal rules as normal pointer input. Popovers and scrollbars never inherit a drag
role from their DOM parent. Overlapping client elements occlude native hit regions.

Button activation uses the normal click path. Releasing outside the pressed
control, losing focus, disabling/unmounting it or cancelling capture cancels the
click. Synchronous handlers can return `EventResponse::PREVENT_DEFAULT` to suppress
an action. Enter/Space and programmatic clicks use the same action queue.

Windows maps the rendered regions to `HTCAPTION`, `HTMINBUTTON`, `HTMAXBUTTON` and
`HTCLOSE` through a per-window subclass. The OS owns drag/resize loops and can
recognize the maximize area for Snap Layouts. Button mouse events are translated
into Winit's normal input path, preserving CSS and click cancellation. Native
caption dragging on Windows is handled before ordinary element events; use Client
regions to exclude interactive content rather than relying on a drag callback to
cancel it. The subclass never borrows the live widget tree during a native callback.

macOS/Linux start a Winit drag on a primary-button press in a drag region. Native
capture can consume the release, so UI capture is cancelled before starting the
native loop. Double-click respects macOS's titlebar preference and toggles maximize
on Linux. Right-click requests Winit's window menu where supported. Linux client
resize edges show resize cursors and call `drag_resize_window`; maximized and
fullscreen windows disable those edges.

## Reactive window state

Call `window_context()` inside a component to get a weak window handle.
`context.state()` subscribes that component to focus, maximize, fullscreen,
resizability, native control geometry and button-layout changes. Unchanged state
does not rerender. Fullscreen removes the button groups while keeping your content.
Reading state from an application callback does not subscribe a component.

`context.request(WindowAction::Close)` (also Minimize or ToggleMaximize) queues an
operation after event dispatch. The weak handle does not retain the native window
and safely returns false after the containing tree has been dropped. Close requests
are processed even for minimized windows. Headless hosts can use
`WidgetTree::set_window_state`, `take_window_actions`, and `set_update_waker`.

## Scope and verification

This port implements chrome configuration, composition and window interactions.
It does not import Zed's project menus, collaboration UI, native tabs, Linux
rounded/shadow frame rendering, or native accessibility for custom-drawn controls.
Winit does not expose every Wayland compositor capability, so runtime control
availability is limited to Winit's window flags; a compositor may ignore a request.
Use System decorations when compositor-owned appearance and behavior are required.

```sh
cargo run --example title_bar
cargo run --example title_bar -- --smoke
cargo test --workspace
cargo check --all-targets --target x86_64-pc-windows-gnu
cargo check --all-targets --target x86_64-unknown-linux-gnu
```

F11 in the example toggles fullscreen. The smoke test opens two windows, checks
native traffic-light bounds after resize on macOS, waits for idle exposure, and
checks that layout/scene counters remain unchanged, then minimizes and closes both
windows through the task-driven action queue.

macOS native rendering, measured traffic-light placement, native zoom/fullscreen,
resize and two-window idle behavior were exercised locally. Windows and Linux have
cross-compilation coverage, not runtime verification on their native systems.
Windows Snap Layouts and Linux compositor/portal behavior still require native QA.

## Provenance

Design reference: Zed/GPUI revision `7960b2a7c9568e90fbe0727332149e5b2a5fd57a`.
This implementation adapts the concepts to Winit and voidui's existing tree/event
runtime; it does not import the GPUI platform runtime or Zed workspace dependencies.
No Cargo registry dependency source was modified.

- [GPUI window configuration](https://github.com/zed-industries/zed/blob/7960b2a7c9568e90fbe0727332149e5b2a5fd57a/crates/gpui/src/platform.rs)
- [Zed platform titlebar composition](https://github.com/zed-industries/zed/blob/7960b2a7c9568e90fbe0727332149e5b2a5fd57a/crates/platform_title_bar/src/platform_title_bar.rs)
- [macOS native buttons](https://github.com/zed-industries/zed/blob/7960b2a7c9568e90fbe0727332149e5b2a5fd57a/crates/gpui_macos/src/window.rs)
- [Windows hit testing and input](https://github.com/zed-industries/zed/blob/7960b2a7c9568e90fbe0727332149e5b2a5fd57a/crates/gpui_windows/src/events.rs)
- [Linux desktop setting subscription](https://github.com/zed-industries/zed/blob/7960b2a7c9568e90fbe0727332149e5b2a5fd57a/crates/gpui_linux/src/linux/xdg_desktop_portal.rs)
