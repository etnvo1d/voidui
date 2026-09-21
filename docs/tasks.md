# Asynchronous tasks

Write ordinary Rust `async` code. Local tasks run on the UI thread, may hold `State`
and `Rc` values, and resume on that same thread after an await. File I/O, timers,
background futures, and worker jobs have explicit execution boundaries.

## Async queries

Use resource for loading data. Pass an explicit input value and an async loader;
read State inputs with get() so the component subscribes to their changes.

```rust
use voidui::{component, div, files, resource, state};
let view = component(|| {
    let path = state(|| String::from("README.md"));
    let content = resource(path.get(), async |path| files::read_text(path).await);
    div()
        .child(if content.is_loading() { "Loading" } else { "Ready" })
        .child(div().tag("button").child("Reload").on_click(move || content.reload()))
});
```

Call resource unconditionally in a stable hook order. It starts after the tree
commits, reloads when its input compares unequal, and cancels the previous request.
Unrelated renders do not reload. A generation check prevents old results from
committing. Rendering failures discard staged input/loader changes; removing the
component cancels pending work, including work that has never been polled.

Resource<T, E> is Copy. data() returns Option<Read<T>>; with() borrows the last
successful result; error() returns a shared ResourceError<E> distinguishing domain
and execution failures. Neither T nor E needs Clone. is_loading() covers the initial
load and refreshes. A refresh keeps the last successful data and clears the old
error. Failed refreshes retain that data and expose the new error. The previous
snapshot can therefore be visible while a changed input is loading; check
is_loading() when your UI should hide it. Input changes take effect at commit,
so the render that declares a changed input still sees the previous snapshot.

reload() uses the latest committed input and callback. Admission failure, panic,
and cancellation are observable errors, not an indefinitely loading status. Stale
handles make reload a no-op. Retaining a data/error snapshot intentionally keeps
that snapshot alive after unmount; retaining Resource does not keep it alive.

Resource is a query primitive, with no implicit retries, global cache, timer,
debounce interval, or cross-component deduplication. Share one resource from a
surviving owner when several components need the same query. Use input-driven
resources for searches and loads; use async events or explicit tasks for writes.
Cancellation cannot undo an external side effect or stop a blocking call that has
already started. Existing backend capacity limits continue to apply.

Run `cargo run --example resources -- README.md` for editable file loading.

## Async events

```rust
use std::path::PathBuf;
use voidui::{component, div, files, state, text, IntoElement};

#[component]
fn reader(path: PathBuf) -> impl IntoElement {
    let content = state(|| "Click to read".into());
    div()
        .child(text(content.get()))
        .child(
            div().tag("button").child("Read")
                .on_click(async move || -> anyhow::Result<()> {
                    content.set(files::read_text(&path).await?);
                    Ok(())
                }),
        )
}
```

Event registration preserves the builder type, so `child`/`children` and widget
options can appear before or after `on_click`. See [Events and dragging](events.md).
Each activation starts an independent task. Mouse, keyboard, disabled, and inert
behavior use the same activation path as synchronous callbacks. The callback must
implement `AsyncFn`: captured state can use interior mutability, but the callback
cannot consume its captures on each click or require an exclusive borrow of itself.

The handler and each active invocation retain their owned captures across await.
Reconciliation replaces the handler for **future** activations; in-flight calls
keep the captures from their original activation. Removing the node cancels those
calls. Changing an async handler to a synchronous handler cancels its old calls too.

Return `()` when handling results yourself, or `Result<(), E>` with a displayable
error. Handler errors and task panics go to `TaskRuntime::set_error_handler`; the
default uses `log::error!`. Install a logger or supply a handler to display failures.
A return annotation such as `-> anyhow::Result<()>` also makes `?` inference clear.

## General execution and task results

```rust
use voidui::{component, div, task_scope};
use voidui::tasks::time;

let view = component(|| {
    let tasks = task_scope();
    div().tag("button").child("Start")
        .on_click(move || {
            tasks.spawn(async {
                for _ in 0..3 {
                    // Give native input and other tasks a turn between small steps.
                    time::yield_now().await;
                }
            });
        })
});
```

`spawn` starts exactly one task per call. It does not infer dependencies, rerun your
code, cache results, retry errors, or discard older successful results. Use ordinary
functions, loops, channels, joins, and application state to express your workflow.

A `Task<T>` is a single-consumer future returning `Result<T, TaskError>`. A task
whose own output is `Result<T, E>` therefore returns two result layers: execution
failure and business failure. Preserve that distinction or convert it at the
application boundary.

```rust
use voidui::TaskRuntime;
let runtime = TaskRuntime::default();
let owner = runtime.scope();
let child = owner.spawn(async { 21 });
let parent = owner.spawn(async move {
    let answer = child.await? * 2;
    Ok::<_, voidui::TaskError>(answer)
});
runtime.tick();
runtime.tick();
assert!(parent.is_finished());
runtime.shutdown();
```

Dropping a `Task` handle leaves its task owned by the scope; a discarded output is
released on completion. `task.cancel()` requests cancellation and wakes a suspended
task. Await the handle to observe completion or cancellation. Cancellation takes
effect when the executor can drop the future; it cannot preempt a poll already
running. `scope.cancel_all()` cancels current work without closing the scope.

Read shared state after an await when updating it, or use `State::update` for the
final synchronous mutation. An expression such as `count.set(count.get() +
work.await)` reads `count` before waiting and can overwrite intervening changes.

Repeated requests can finish out of order. For a latest-request-wins UI, explicitly
cancel the previous task and/or check an application request version before writing
results. For writes, choose a serialization, idempotency, or commit policy rather
than assuming cancellation reverses an external effect.

## Component, window, and application ownership

`task_scope()` returns the current component's weak scope handle. It allocates the
component's scope on first use, shares it across repeated calls, and preserves it
across rerenders. This accessor consumes no ordered hook slot and may be conditional.
Removing the component or changing its key/type cancels its scoped work. Keeping
old handles does not keep the component or its tasks alive.

`window_task_scope()` returns the containing tree's scope. It survives component
unmounts and `build_root` resets, and closes when the tree/window is dropped.
`app_task_scope()` survives individual window closure and closes at application
exit. The default application exits when its final window closes; app scope does
not keep a windowless event loop running.

Outside component rendering, use `AppWindow::task_scope()`,
`WidgetTree::task_scope()`, or `TaskRuntime::scope()`. The last method returns an
`OwnedTaskScope`: keep the owner alive while work should run and clone its weak
`handle()` into callbacks. Dropping the owner closes it even when handles survive.

### Run once after mount

```rust
use voidui::{component, div, on_mount, state, text};
use voidui::tasks::time;
use std::time::Duration;

let view = component(|| {
    let ready = state(|| false);
    let result = ready;
    on_mount(async move || -> Result<(), voidui::TaskError> {
        time::sleep(Duration::from_millis(10)).await?;
        result.set(true);
        Ok(())
    });
    div().child(text(if ready.get() { "Ready" } else { "Starting" }))
});
```

Call `on_mount` unconditionally and in the same hook order as `state`. Its first
callback runs once for a mounted identity, using the inputs captured on that mount.
Later renders do not update or repeat it. Mount callbacks start only after the
whole tree update commits; failed rendering/reconciliation drops staged callbacks.
Reset a tree after a caught rendering panic, as described in the state guide.

Starting tasks or polling an executor directly during component rendering panics.
This prevents render loops from accidentally launching duplicate external work.
State initializers cannot acquire component scopes or call lifecycle hooks.

## Background futures, CPU work, and blocking I/O

```rust
use voidui::TaskRuntime;
use voidui::tasks::{time, workers};
use std::time::Duration;

let runtime = TaskRuntime::default();
let owner = runtime.scope();
let background = owner.spawn_background(async {
    // Tokio-dependent networking and timers are valid inside this async block.
    time::sleep(Duration::from_millis(1)).await?;
    Ok::<_, voidui::TaskError>(42)
});
owner.spawn(async move {
    let value = background.await??;
    let result = workers::compute(move || value * 2).await?;
    // This continuation is on the UI thread again.
    Ok::<_, voidui::TaskError>(result)
});
runtime.shutdown();
```

`spawn_background` requires `Send + 'static` for its future and output. It runs on
a lazily created Tokio backend. Create runtime-dependent operations inside the
async block, not before submitting it. HTTP clients and other Tokio-based libraries
can be used there without blocking the UI executor. Local tasks do not implicitly
enter Tokio or start its threads; use a background task for such library calls.

`workers::compute` handles finite CPU work. `workers::blocking` handles finite
synchronous calls. Their closures and outputs must be `Send + 'static`. Both are
awaitable from local or voidui background tasks. They use separate admission and
concurrency limits over a shared bounded blocking pool. They do not automatically
inherit into tasks independently spawned with `tokio::spawn`.

`files::read`, `files::read_text`, and `files::write` use the blocking lane and return
`std::io::Result`. `read_text` returns a shared string for inexpensive widget clones.
These are explicit **whole-file** helpers; the entire input/output is retained.
Use streaming I/O and bounded buffers for large files or response bodies.

`time::sleep` and `time::timeout` use event-driven backend timers, without a periodic
UI timer. `time::yield_now` needs no backend threads. Helpers called outside a
voidui task report `TaskError::NoRuntime` (file helpers wrap it in an I/O error).

```compile_fail
use std::rc::Rc;
use voidui::TaskRuntime;
let runtime = TaskRuntime::default();
let owner = runtime.scope();
let local = Rc::new(42);
owner.spawn_background(async move { *local }); // Rc cannot cross threads.
```

```compile_fail
use voidui::TaskRuntime;
let runtime = TaskRuntime::default();
let owner = runtime.scope();
let value = String::from("temporary");
let borrowed = &value;
owner.spawn(async move { borrowed.len() }); // Spawned work cannot borrow this stack.
```

## Async callbacks for component authors

Use `AsyncCallback<A, R>` for a cheap cloneable async input. Its calls own the
callback until completion, so the future does not borrow a temporary component
parameter. The caller chooses whether to await it in an existing task or submit it
to a scope. Domain errors and result types remain under the component's control.

```rust
use voidui::{AsyncCallback, component, div, IntoElement};

#[component]
fn confirm(on_confirm: AsyncCallback<(), anyhow::Result<()>>) -> impl IntoElement {
    div().tag("button").child("Confirm")
        .on_click(async move || -> anyhow::Result<()> {
            on_confirm.call(()).await
        })
}
```

## Budgets, errors, and host integration

```rust
use voidui::{Application, TaskOptions, TaskRuntime};
use std::time::Duration;
let runtime = TaskRuntime::new(TaskOptions {
    max_tasks: 2048,
    polls_per_tick: 128,
    poll_budget: Duration::from_millis(2),
    compute_threads: 2,
    compute_queue_capacity: 64,
    ..Default::default()
}).unwrap();
runtime.set_error_handler(|error| eprintln!("Async operation failed: {error}"));
let application = Application::new().task_runtime(runtime);
```

Defaults permit 4096 active tasks, up to 256 queue visits or 2 ms per UI tick,
one background async worker, CPU concurrency based on available parallelism,
four blocking operations, and 256 waiting jobs per worker lane. Blocking threads
expire after ten seconds idle. All budgets are configurable through `TaskOptions`.
A poll already in progress can exceed its time budget; it is logged, not preempted.

Task admission returns `AtCapacity`. Queued cancelled IDs also count toward queue
admission until drained, preventing unbounded spawn/cancel churn between ticks.
Worker admission returns `WorkerQueueFull` before allocating a backend job beyond
the lane's capacity. Waiting permits release on cancellation; running blocking
operations retain their permits until the **actual operation** finishes.

Limits cover submissions through this API. They cannot bound arbitrary allocations,
third-party internal queues, or work independently spawned by another library.
Continuous audio/video processing should use a media backend and bounded data
buffers; control its startup and lifecycle with tasks. Media frames and real-time
audio callbacks should not go through component state or this UI task queue.

The native application shares a runtime across windows and advances one bounded
ready batch before sleeping. Component updates commit even for minimized or
occluded windows. Layout and painting still wait for a drawable surface. Wakers
only enqueue generational task IDs; local futures never cross threads. Notifications
coalesce, stale wakes cannot reactivate completed tasks, and idle checks do not
scan suspended tasks or allocate.

Headless hosts can construct `WidgetTree::with_task_runtime(runtime.clone())`.
Install a thread-safe `runtime.set_waker(...)` that only notifies the host, then
call `runtime.tick()` and `tree.flush_updates()` on the UI thread when notified.
Use `tree.set_update_waker(...)` too when state can be changed independently of
tasks. A remaining ready batch requests another wake; never wait for a drawing
callback to advance tasks. `TaskStats` exposes task counts, polls, notifications,
backend activation, and admitted worker jobs.

Reuse an existing Tokio backend with `TaskRuntime::with_tokio_handle`. Enable its
I/O/time drivers and keep it driven; a current-thread runtime needs its own active
`block_on` loop. Voidui does not shut down that externally owned backend.

Call `runtime.shutdown()` when a custom host exits, even if runtime clones survive.
It closes admission and cancels scopes, including tasks capturing a runtime clone.
Prefer weak `TaskScope` captures to avoid retaining a runtime in its own tasks.
The native application shuts down automatically. Started blocking calls may finish
later with their owned inputs; shutdown never waits for them on the UI thread.
Registry and queue capacities are retained for reuse until runtime destruction.

## Reproduce validation

```sh
cargo test --workspace
cargo test --doc -p voidui
cargo run --example async_tasks -- README.md
cargo run --example async_tasks -- --smoke README.md
cargo bench --bench tasks -- 10000 50000
cargo check --all-targets --target x86_64-pc-windows-gnu
cargo check --all-targets --target x86_64-unknown-linux-gnu
```

Tests cover local ownership, cross-thread wakes, cancellation, stale IDs, bounded
scheduling and workers, render failures, native lifecycle integration, typed async
callbacks, file I/O, real loopback network I/O, timers, and external runtime reuse.
The native smoke verifies updates while minimized and an idle interval with no
layout/scene rebuild. The task benchmark measures requested heap bytes including
registry reservations, excludes allocator metadata, and asserts zero idle
allocations and no background threads for local-only tasks. Windows/Linux cross
compilation does not establish platform runtime behavior.

### Measured local overhead

A macOS/aarch64 run with Rust 1.98.1 in release mode, 10,000 empty suspended
futures, and 50,000 idle checks reported:

- 282.5 additional requested heap bytes per suspended task, including queue and registry reservations.
- 0.173 microseconds per spawn plus first poll.
- 6.88 nanoseconds per idle check, zero idle allocations, and no backend startup.

These measurements exclude captured application data, allocator metadata, native
rendering, and background I/O. They are not a process power/RSS benchmark. The
existing state benchmark also retained zero allocations for warmed local updates.
