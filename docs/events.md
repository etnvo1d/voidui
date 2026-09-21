# Events and dragging

All event methods accept synchronous and asynchronous callbacks through one API.
They preserve the concrete builder type, so children, styles, and widget-specific
methods can appear before or after event registration. Function components and
already converted `Element` values support the same event methods without adding
CSS/layout nodes.

```rust
use voidui::{div, files, state, component};
use std::path::PathBuf;

let view = component(|| {
    let output = state(|| None);
    let path = PathBuf::from("README.md");
    div()
        .on_click(async move || -> anyhow::Result<()> {
            output.set(Some(files::read_text(&path).await?));
            Ok(())
        })
        .tag("button")
        .child("Read")
});
```

Use `on_click` for both synchronous and asynchronous callbacks. Calls to the
removed `on_click_async` alias migrate by renaming the method; the closure stays
the same.

## Supported events

| Method | Optional callback argument | Routing |
| --- | --- | --- |
| `on_click` | `ClickEvent` | Bubbles; primary-button click, keyboard activation, or programmatic click |
| `on_mouse_enter` | `MouseEvent` | Direct notification when entering a node or its descendants |
| `on_mouse_leave` | `MouseEvent` | Direct notification when leaving a node and its descendants |
| `on_mouse_down` | `MouseEvent` | Bubbles; every native mouse button |
| `on_mouse_up` | `MouseEvent` | Bubbles; matching drag release targets the capture owner |
| `on_mouse_move` | `MouseEvent` | Bubbles; capture retargets movement to its owner |
| `on_mouse_scroll` | `MouseEvent` | Bubbles; preserves line or logical-pixel wheel units |
| `on_drag` | `DragEvent` | Direct notifications to the nearest drag owner |
| `on_key_down` | `KeyEvent` | Bubbles from focus, before text editing and native defaults |
| `on_key_up` | `KeyEvent` | Bubbles from focus, including on editable widgets |

A callback can ignore its event with `||`, or receive an owned snapshot. Rust's
trait inference may require an explicit event parameter type:

```rust
use voidui::{div, MouseEvent, KeyEvent};
let view = div()
    .on_mouse_move(|event: MouseEvent| {
        println!("Local position: {:?}", event.local_position);
    })
    .on_key_up(async |event: KeyEvent| {
        println!("Released {:?}", event.key);
    });
```

A normal closure returning a future also works, subject to Rust's normal capture
rules. Native async closures can borrow their owned captures across await. Event
callbacks must be repeatable (`Fn` or `AsyncFn`), not consuming `FnOnce` handlers.

Every async event starts an independent task in its retained handler slot. It may
capture UI-only `State`/`Rc` handles. Arguments and captures stay alive across await;
no borrowed Winit event or temporary tree reference is retained. For high-frequency
motion/scroll, prefer short synchronous updates. Async work remains bounded by the
shared task admission limits; the framework does not silently debounce/drop events.

## Coordinates, targets, and buttons

`MouseEvent.position` and `DragEvent.position` are logical window coordinates.
`local_position` is relative to `current_target`'s border-box origin at dispatch.
Values may be negative or exceed the receiver's bounds during capture. Native
physical coordinates and pixel wheel deltas are converted once using window DPI.
Line wheel deltas retain `WheelUnit::Lines`; pixel deltas use `WheelUnit::Pixels`.
Signs are preserved from Winit.

`target` is the initial receiver; `current_target` changes while bubbling. A drag's
target is its capture owner. Enter/leave notifications target each entered/exited
node separately and expose the opposite hit as `related_target`. Moving between
siblings does not emit leave/enter for their shared ancestors. Layout refreshes can
change enter/leave state, but do not fabricate mouse-move or drag samples.

During capture, button, move, and scroll notifications target the owner; hover
enter/leave notifications still describe the actual hit path. Down/up events identify the exact `MouseButton`, including `Other(u16)`. The
`buttons` flags describe held standard buttons after the change. `modifiers`
contains Winit's current modifier state. `ClickEvent.source` distinguishes pointer,
keyboard, and programmatic activation; only pointer clicks carry a pointer position.

## Propagation and native defaults

Synchronous handlers may return `()`, `EventResponse`, `EventResult`, or a `Result`
containing these. `()` continues normally. The response decisions are independent:

```rust
use voidui::{div, EventResponse, KeyEvent};
use voidui::core::event::{Key, NamedKey};

let view = div().attr("tabindex", "0")
    .on_key_down(|event: KeyEvent| {
        if event.key == Key::Named(NamedKey::Tab) {
            // Prevent focus traversal, but still notify ancestor key handlers.
            EventResponse::PREVENT_DEFAULT
        } else {
            EventResponse::CONTINUE
        }
    });
```

`STOP_PROPAGATION` stops ancestor dispatch without preventing other registrations
on the same node or its defaults.
`PREVENT_DEFAULT` prevents defaults without stopping bubbling. `HANDLED` sets both;
responses can also be combined with `|`. Legacy `EventResult::Handled` maps to both.

General key handlers run before editor key commands, document-selection shortcuts,
button activation, and Tab traversal. Mouse-down default prevention suppresses
focus, text selection, click arming, and automatic drag capture. Mouse-move prevention
suppresses editable/document selection movement. Scroll prevention suppresses the
shared scroll-container default, including editable controls. Release always clears capture/pressed state.
IME preedit and committed text remain separate from key notifications.

Async handlers return `()` or `Result<(), E>`. They cannot return propagation/default
decisions because the native event has finished before the task runs:

```compile_fail
use voidui::{div, EventResponse};
let view = div().on_key_down(async || EventResponse::PREVENT_DEFAULT);
```

Sync errors, panics, and async errors use the task runtime's error handler. A failed
sync callback continues dispatch unless another handler prevented it. Install a
logger or `TaskRuntime::set_error_handler` to surface failures.

Hidden, disabled, inert, and stale targets cannot receive ordinary input. Disabled
ancestors suppress descendant input. Hit testing respects clipping, stacking,
modals, and `pointer-events`. A pointer-events-none ancestor can still observe a
bubble from an explicitly targetable descendant. Key handlers on a leaf need a
focusable element (`button`, an input, or `tabindex`). Without focus, keys route to
the active modal or root.

## Drag capture and cursor ownership

```rust
use voidui::{component, div, state, DragEvent, DragPhase};
use voidui::core::geometry::Point;
use voidui::style::selection::Cursor;
use winit::window::CursorIcon;

let view = component(|| {
    let offset = state(Point::<f32>::default);
    let origin = state(Point::<f32>::default);
    let position = offset.get();
    div().absolute().left(position.x).top(position.y)
        .width(120.0).height(80.0)
        .cursor(Cursor::Icon(CursorIcon::Grab))
        .on_drag(move |event: DragEvent| {
            match event.phase {
                DragPhase::Start => origin.set(offset.get()),
                DragPhase::Move => {
                    let start = origin.get();
                    offset.set(Point::new(
                        start.x + event.total_delta.x,
                        start.y + event.total_delta.y,
                    ));
                }
                DragPhase::End | DragPhase::Cancel => {}
            }
        })
});
```

An unprevented primary press chooses the nearest enabled `on_drag` handler on the
hit path. Capture is established synchronously, even for an async handler:

1. `Start` is sent on press. There is no implicit movement threshold.
2. `Move` follows coordinate changes, including outside the element and viewport.
3. `End` follows the matching primary release, even when hit testing finds nothing.
4. `Cancel` follows focus loss, suspension, explicit cancellation, removal of the
   owner/handler, disabling/inerting/hiding the owner, or a modal making it inert.

Other buttons cannot steal or release a primary capture. Nested draggable nodes
choose the nearest handler. An editable input blocks an ancestor's automatic drag
so ordinary text selection still works; explicitly attaching `on_drag` to the input
opts that node into the drag gesture.

`origin`, `delta`, and `total_delta` use window coordinates. Moving/resizing the
owner does not change the gesture's coordinate origin. Capture remains valid while
layout is pending. Any actual drag movement suppresses the later click; an unmoved
press/release can still click. Cancellation never manufactures a mouse-up or click.

During capture, `pointer_cursor()` resolves the **owner's current computed cursor**
before considering the hovered node, text selection, or layout readiness. This
supports `.handle:active { cursor: grabbing; }`, dynamic cursor changes, and
`cursor:none`. Leaving the native window does not clear capture or cursor ownership.
Release restores the actual hovered element's cursor, or the default outside it.
If release invalidates layout, native cursor application waits for resolved hit
geometry and is refreshed before the completed frame is exposed to the host.
Native cross-window delivery uses Winit/platform mouse capture; the framework does
not confine or lock the cursor, or attempt to override another application's cursor.

Synchronous `Cancel` runs while the owner still exists, even when just disabled.
Unmount immediately drops its event slots and cancels their async tasks, so an async
cancellation callback is not guaranteed to run after removal. Keep required cleanup
in synchronous cancellation or resource destructors.

## Component authors and retained identity

Event callbacks live in a sparse table keyed by retained `WidgetId`. Plain nodes
have no listener vector or task scope. Registering events does not wrap the widget
or change its type, and callback-only reconciliation does not invalidate layout.
Re-registering the same event on one builder replaces that registration. Component
root handlers are appended after inner handlers; retained component origins keep
independent slots stable when an inner binding disappears.

In-flight async calls keep the old callback's captures. Replacing an async callback
preserves those calls; removing the slot or changing it to sync cancels them.
Reordering other event registrations does not cancel surviving slots.

For a custom event payload, use `EventHandler<E>` directly:

```rust
use voidui::{EventHandler, TaskRuntime};
let runtime = TaskRuntime::default();
let mut handler = EventHandler::<String>::new(async |value: String| {
    println!("Saved: {value}");
});
handler.dispatch("document".into(), &runtime);
runtime.tick();
runtime.shutdown();
```

Call `replace` when reconciling a callback description and drop the handler when
its owner unmounts. `clone` makes a fresh description with independent task ownership;
`cancel` stops its current calls. No event-specific async executor is needed.
Custom `Widget` implementations can opt into `accepts_events` and route their
`on_event_with_tasks` through these handlers. Use `WidgetBuilder::from_widget` to
construct a custom widget with empty bindings and default properties.

Headless hosts use `dispatch_mouse_move`, `dispatch_mouse_button`,
`dispatch_mouse_scroll`, and `dispatch_key`. Wheel dispatch includes the preventable
scroll-container default; see [Scrolling](scrolling.md). Call `refresh_pointer` after committed
layout/layer changes, and `cancel_pointer_capture` on native focus/capture loss.
The older `pointer_moved` and `pointer_pressed` helpers still work, but discard the
default-prevention response. `AppWindow` exposes native physical mouse adapters
and `current_cursor()` for integrations and programmatic window tests.

## Verification

```sh
cargo test --workspace
cargo test --doc -p voidui
cargo test --test events --no-default-features
cargo check --all-targets --no-default-features
cargo run --example events
cargo run --example events -- --smoke
cargo bench --bench events -- 100000
```

`--smoke` drives the actual window adapters programmatically and checks DPI conversion,
capture outside the node/window coordinates, cursor ownership and restoration, and
key/scroll routing. `--native-test` instead waits for real OS input: drag from the
handle to the crosshair panel, release, focus the input, press Tab then `a`, and scroll.
It checks the applied native cursor and requires editing to retain focus despite Tab.
Without the editing feature, press a key on the handle instead.

The headless suite covers sync/async adaptation, bubbling, default prevention,
input coexistence, handler reconciliation, capture, cancellation, hover transitions,
button/scroll data, and component identity. The benchmark asserts zero warmed
allocations for movement within one hovered/captured target. Boundary transitions
may allocate short ancestor paths. OS-level pointer automation and platform runtime
verification are distinct from headless/programmatic tests and cross-compilation.

### Local validation notes

The macOS/aarch64 programmatic native-window smoke passes, including a regression
where release previously applied `Auto` before layout even though the resolved
hover cursor was `Crosshair`. Cursor application now waits for valid hit geometry
and refreshes before `on_frame`. Real OS pointer automation could not be completed
in this run because the automation service repeatedly returned `noWindowsAvailable`;
`--native-test` remains available for manual OS-level validation.

A release run of 100,000 same-target samples reported 60.86 ns per mouse move and
114.77 ns per captured move, with zero warmed allocations in either loop. These
figures exclude native input delivery, rendering, boundary transitions, and user
callback work. They vary with the machine and compiler.
