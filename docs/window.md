# Native windows and frame scheduling

## Run

```sh
cargo run --example hello
cargo run --release --example hello
```

The basic entry point loads system fonts once. The hello example uses the bundled
IBM Plex Sans font for deterministic output without a system-font scan.

```rust,no_run
use voidui::{Application, WindowOptions, div, text};
use voidui::core::layout::{Rect, length};
use voidui::style::color::Rgba8;

fn main() -> anyhow::Result<()> {
    Application::new()
        .window(
            WindowOptions::default(),
            div()
                .background(Rgba8::from_rgb8(235, 240, 245))
                .padding(24.0)
                .child("Inherited text")
                .child(text("Custom label").font("Arial").font_size(24.0))
                .child(div().height(40)),
        )
        .run()
}
```

Call `window` more than once for multiple native windows. The runtime shares the
GPU device/queue and `TextSystem`; each window owns its surface, atlas, layout
cache, widget tree, and retained scene. GPU resources are dropped before native
window handles. `resumed`/`suspended` rebuild surface resources without discarding
the widget tree or font database.

`Application::text_system` accepts a preconfigured shared font system.
`on_window_event` handles native input before the built-in lifecycle handler.
Use `AppWindow::tree_mut()` inside that callback to update styles/content and
schedule layout. CSS hover/active/focus state is updated by the runtime when the
loaded stylesheets reference it; [CSS hot reload](css.md) is separately opt-in.
General pointer/key handlers run after the native observer and can prevent built-in
defaults. Text input clients then receive focused input before document-selection
shortcuts; clipboard and Winit IME use the text protocol. See
[Events and dragging](events.md) and [Inputs and reusable editing](input.md).

`on_frame` runs after successful presentation and can inspect `FrameStats`, call
`snapshot`, or close a window. The callback itself does not schedule another frame.
A snapshot is an explicit synchronous GPU readback; normal rendering never reads
pixels back to the CPU.

## When work happens

Component commits can enqueue another update (for example, restarting a resource
publishes its loading state). Native frames use `WidgetTree::prepare_frame` and
defer layout/presentation when it returns `None`. Paint invalidation survives that
deferral, and CPU follow-up work does not count as a failed GPU presentation.
Custom incremental hosts should use the same boundary before `layout_computed`
and `draw`; `update_styles` alone does not guarantee an empty update queue. Each
attempt processes one batch rather than draining arbitrary user effects in a loop.

- Content/tree edits: layout, rebuild the scene, present.
- Changed logical window size: layout, rebuild the scene, present.
- DPI change with identical logical dimensions: rebuild device-scaled primitives,
  reuse logical layout and shaped text, present.
- Exposure or `request_redraw`: re-present the retained scene without layout or
  scene construction.
- No changes: `ControlFlow::Wait`; no periodic timer, polling loop, or display link.
- Minimized, occluded, zero-sized, or suspended: park rendering until restored.
- Temporary presentation/GPU recovery failure: bounded exponential retry using
  `WaitUntil`. Default delays start at 16 ms, cap at one second, and stop after
  eight failures. A new external invalidation or restoration starts another attempt.

Native occlusion support varies by window system. Where it is unavailable, a
static window still has no periodic redraw. The runtime does not treat loss of
focus as occlusion: an unfocused but visible window may still need a redraw.

`Scene::clear()` keeps vector allocations. Text shaping, glyph rasterization,
and atlas resources are reused. Atlas/device reset explicitly invalidates cached
scene tile IDs. No extra device-wide blocking poll is performed before every
resize; WGPU owns synchronization and retirement of in-flight textures. Path/MSAA
intermediate targets are allocated only for scenes containing vector paths, so
ordinary div/text scenes avoid those full-window allocations.

GPU selection now prefers an integrated GPU over a discrete GPU when neither an
explicit `ZED_DEVICE_ID` override nor a compositor-device match applies. Surface
compatibility is still tested. The inherited override is retained for compatibility
with the renderer fork. VSync/FIFO remains the default; there is no uncapped loop.

`FrameStats` records layout passes, scene builds, presentations, failures, and
last-pass CPU durations. It does not measure GPU execution time or total system
power consumption. Layout's internal cache currently reuses measurements within
each reflow; the window skips that entire reflow when content is unchanged.

## Platform adaptation

The implementation follows selected GPUI policies while retaining Winit and WGPU
as the native window and graphics implementations. It does not copy the complete
GPUI application runtime or its native renderers.

| Platform | Adaptation |
| --- | --- |
| macOS | AppKit main-thread loop; disable unsupported automatic native tabbing; synchronous resize drawing with CAMetalLayer `presentsWithTransaction`; allow drawable acquisition timeout; park on occlusion. |
| Windows | Enable Winit's per-monitor DPI awareness before HWND creation; process physical resize/scale events; skip minimized windows; retain resources between exposure redraws. |
| Wayland | Supply app_id; notify Winit immediately before a successful presentation so compositor frame callbacks can pace requests; do not force an X11 backend. |
| X11 | Supply WM_CLASS; handle exposure/resize through Winit and use the same event-driven, bounded-retry scheduler. |

On macOS and Windows, layout and the first scene are prepared while the window is
hidden, then it is mapped **before** acquiring the presentation buffer. Waiting
for a successful present before mapping is invalid: some surfaces return Occluded
while hidden, which would leave the application permanently invisible.

The Metal hook borrows WGPU's HAL surface guard and changes only presentation
properties under the layer mutex. WGPU's Metal backend performs its existing
`waitUntilScheduled` plus drawable-present sequence for transactional frames.
Normal frames return to non-transactional presentation. There is no separate
CoreVideo callback thread or duplicate display-link lifecycle.

Backgrounds, solid borders, uniform rounded corners, and per-axis rectangular
overflow clipping are painted by the shared box painter. Border color defaults
to inherited foreground color. Generic scrolling and scrollbars share retained
geometry and demand-driven frames; see [Scrolling](scrolling.md). Rounded overflow clipping, native
accessibility content, native tabs,
drag-and-drop, animation scheduling, and browser/mobile entry points are not part
of this port. Input still requires an application callback or future widget dispatch.

Custom titlebars and platform window controls are opt-in; see [Custom titlebars](titlebar.md).

## Pointer and keyboard events

General handlers run before text/default actions. Drag capture routes movement and
release to the original owner, while the owner's computed cursor takes precedence
over the hovered element. Native cursor exit preserves capture; focus loss and
suspension cancel it. See [Events and dragging](events.md) for routing and test APIs.

## Asynchronous execution

Use `Application::task_runtime` to configure the shared executor, its budgets, or
an existing Tokio backend. Async task wakeups are handled independently of drawing;
minimized windows still commit component updates and task lifecycles. Ordinary
windows start no worker threads until background work or timers are used.
See [Asynchronous tasks](tasks.md) for scoped execution and async event handlers.

## References and provenance

Reference revision: `e2534d2357a80795d2c372d31268748e7ee992e5`, the same GPUI revision
as the vendored renderer. The platform policies were adapted to the project's
existing abstractions; no dependency source in the Cargo registry was modified.
The maintained renderer fork changed only where native presentation, recovery,
and allocation policy required it.

- [GPUI macOS window](https://github.com/zed-industries/zed/blob/e2534d2357a80795d2c372d31268748e7ee992e5/crates/gpui_macos/src/window.rs): synchronous transaction-bound layer display and resize.
- [GPUI display-link lifetime](https://github.com/zed-industries/zed/blob/e2534d2357a80795d2c372d31268748e7ee992e5/crates/gpui_macos/src/display_link.rs): callbacks and subscriptions have nontrivial teardown requirements; this runtime does not introduce a display link for static content.
- [GPUI Windows window](https://github.com/zed-industries/zed/blob/e2534d2357a80795d2c372d31268748e7ee992e5/crates/gpui_windows/src/window.rs): per-window DPI and recovery invalidation.
- [GPUI Wayland frame lifecycle](https://github.com/zed-industries/zed/blob/e2534d2357a80795d2c372d31268748e7ee992e5/crates/gpui_linux/src/linux/wayland/window.rs): compositor pacing, parked frames, and presentation retries.
- [GPUI X11 window](https://github.com/zed-industries/zed/blob/e2534d2357a80795d2c372d31268748e7ee992e5/crates/gpui_linux/src/linux/x11/window.rs): refresh and per-window callbacks.
- [Winit pre_present_notify](https://docs.rs/winit/0.30.13/winit/window/struct.Window.html#method.pre_present_notify): presentation notification placement and Wayland frame throttling.

The GPUI-derived renderer retains its existing Apache/MIT attribution and notices.

## Verification

```sh
cargo test --workspace
cargo check --all-targets --target x86_64-pc-windows-gnu
cargo check --all-targets --target x86_64-unknown-linux-gnu
cargo run --example hello -- --snapshot target/hello.png
cargo run --example hello -- --smoke target/hello-smoke.png
cargo run --example multi_window -- --smoke
```

`--smoke` performs a native resize, checks that layout reflows, then compares
counters over a two-second idle interval. One requested exposure must add one
presentation and no layout/scene rebuilds. Screenshot dimensions are physical
pixels and therefore reflect DPI.

Validated on macOS / Apple M3: native Metal presentation, 2× DPI screenshots,
window shrink/enlarge with text reflow, two independent windows sharing the GPU/font
context, and the idle smoke test. A two-second
process sample placed the main thread entirely in the OS event wait; a separate
idle `ps` sample reported 0.0% CPU. These observations are not a sustained power
benchmark. Windows GNU and Linux GNU targets pass `cargo check --all-targets`;
no Windows/Linux hardware or compositor testing has been performed.

For native macOS UI automation, `scripts/bundle-macos.sh` creates `target/Voidui.app`
without installing it. Override `VOIDUI_BUNDLE_PATH` and `VOIDUI_BUNDLE_ID` if needed.
