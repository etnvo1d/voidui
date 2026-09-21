//! Deterministic task/lifecycle tests run without a window or a rendering loop.
use std::{
    cell::{Cell, RefCell},
    future::{Future, pending, poll_fn},
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    task::{Context, Poll, Waker},
    thread,
    time::{Duration, Instant},
};
use voidui::{
    AsyncCallback, Task, TaskError, TaskOptions, TaskRuntime, TaskScope, app_task_scope, component,
    core::widget_tree::WidgetTree,
    div, files, on_mount, state, task_scope,
    tasks::{time, workers},
    text, window_task_scope,
};

const TEST_TIMEOUT: Duration = Duration::from_secs(10);
struct Host {
    runtime: TaskRuntime,
    wake: mpsc::Receiver<()>,
}
impl Host {
    fn new() -> Self {
        Self::with_options(TaskOptions::default())
    }
    fn with_options(options: TaskOptions) -> Self {
        Self::with_runtime(TaskRuntime::new(options).unwrap())
    }
    fn with_runtime(runtime: TaskRuntime) -> Self {
        let (send, wake) = mpsc::sync_channel(1);
        runtime.set_waker(move || {
            let _ = send.try_send(());
        });
        Self { runtime, wake }
    }
    fn wait<T>(&self, task: Task<T>) -> Result<T, TaskError> {
        let deadline = Instant::now() + TEST_TIMEOUT;
        let mut task = std::pin::pin!(task);
        loop {
            self.runtime.tick();
            if let Poll::Ready(result) = task.as_mut().poll(&mut Context::from_waker(Waker::noop()))
            {
                return result;
            }
            self.wait_wake(deadline);
        }
    }
    fn until(&self, mut ready: impl FnMut() -> bool) {
        let deadline = Instant::now() + TEST_TIMEOUT;
        loop {
            self.runtime.tick();
            if ready() {
                return;
            }
            self.wait_wake(deadline);
        }
    }
    fn wait_wake(&self, deadline: Instant) {
        assert!(
            Instant::now() < deadline,
            "task host timed out: {:?}",
            self.runtime.stats()
        );
        if !self.runtime.has_ready_tasks() {
            self.wake
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("task host was not woken");
        }
    }
    fn tree(&self) -> WidgetTree {
        WidgetTree::with_task_runtime(self.runtime.clone())
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        self.runtime.shutdown();
    }
}
struct DropCount(Rc<Cell<usize>>);
impl Drop for DropCount {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

#[test]
fn local_futures_are_lazy_non_send_and_resume_on_the_ui_thread() {
    let host = Host::new();
    let tasks = host.runtime.scope();
    let seen = Rc::new(Cell::new(0));
    let output = seen.clone();
    let ui = thread::current().id();
    let task = tasks.spawn(async move {
        assert_eq!(thread::current().id(), ui);
        output.set(1);
        time::yield_now().await;
        assert_eq!(thread::current().id(), ui);
        output.set(2);
        output
    });
    assert_eq!(seen.get(), 0);
    assert_eq!(host.runtime.tick().polled, 1);
    assert_eq!(seen.get(), 1);
    assert_eq!(host.wait(task).unwrap().get(), 2);
    assert!(!host.runtime.stats().background_started);
}

#[test]
fn detached_handle_keeps_scope_ownership_and_completed_output_is_released() {
    let host = Host::new();
    let tasks = host.runtime.scope();
    let count = Rc::new(Cell::new(0));
    let value = DropCount(count.clone());
    drop(tasks.spawn(async move { value }));
    assert_eq!(count.get(), 0);
    host.runtime.tick();
    assert_eq!(count.get(), 1);
    assert_eq!(tasks.active_tasks(), 0);
}

#[test]
fn owner_drop_cancels_pending_and_unpolled_futures_and_rejects_stale_handles() {
    let host = Host::new();
    let owner = host.runtime.scope();
    let scope = owner.handle();
    let drops = Rc::new(Cell::new(0));
    let value = DropCount(drops.clone());
    let task = scope.spawn(async move {
        let _value = value;
        pending::<()>().await;
    });
    drop(owner);
    assert_eq!(drops.get(), 1);
    assert!(!scope.is_open());
    assert_eq!(host.wait(task), Err(TaskError::Cancelled));
    assert_eq!(
        host.wait(scope.spawn(async { 7 })),
        Err(TaskError::ScopeClosed)
    );
    assert_eq!(host.runtime.stats().active, 0);
}

#[test]
fn explicit_cancel_wakes_a_suspended_task_and_scope_remains_usable() {
    let host = Host::new();
    let tasks = host.runtime.scope();
    let task = tasks.spawn(pending::<()>());
    host.runtime.tick();
    assert!(!host.runtime.has_ready_tasks());
    task.cancel();
    assert_eq!(host.wait(task), Err(TaskError::Cancelled));
    assert!(task_scope_is_usable(&host, &tasks));
}
fn task_scope_is_usable(host: &Host, tasks: &TaskScope) -> bool {
    host.wait(tasks.spawn(async { true })).unwrap()
}

#[test]
fn nested_spawn_and_cancellation_during_poll_do_not_borrow_the_registry() {
    let host = Host::new();
    let tasks = host.runtime.scope();
    let scope = tasks.handle();
    let task = tasks.spawn(async move { scope.spawn(async { 21 }).await.unwrap() * 2 });
    assert_eq!(host.wait(task).unwrap(), 42);
    let scope = tasks.handle();
    let task = tasks.spawn(async move {
        scope.cancel_all();
        pending::<()>().await;
    });
    assert_eq!(host.wait(task), Err(TaskError::Cancelled));
    assert_eq!(tasks.active_tasks(), 0);
}

#[test]
fn cancellation_drops_user_captures_outside_internal_borrows() {
    struct Cleanup(TaskScope, Rc<Cell<usize>>);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let count = self.1.clone();
            self.0.spawn(async move {
                count.set(count.get() + 1);
            });
        }
    }
    let host = Host::new();
    let source = host.runtime.scope();
    let surviving = host.runtime.scope();
    let count = Rc::new(Cell::new(0));
    let cleanup = Cleanup(surviving.handle(), count.clone());
    source.spawn(async move {
        let _cleanup = cleanup;
        pending::<()>().await;
    });
    drop(source);
    host.until(|| count.get() == 1);
}

#[test]
fn only_woken_tasks_are_polled_and_notifications_are_coalesced() {
    let host = Host::with_options(TaskOptions {
        polls_per_tick: 1024,
        ..Default::default()
    });
    let tasks = host.runtime.scope();
    let wake = Arc::new(Mutex::new(None::<Waker>));
    let saved = wake.clone();
    let polls = Rc::new(Cell::new(0));
    let count = polls.clone();
    tasks.spawn(poll_fn(move |cx| {
        count.set(count.get() + 1);
        *saved.lock().unwrap() = Some(cx.waker().clone());
        Poll::<()>::Pending
    }));
    for _ in 0..100 {
        tasks.spawn(pending::<()>());
    }
    host.runtime.tick();
    let initial = host.runtime.stats();
    assert_eq!(initial.polls, 101);
    for _ in 0..100 {
        assert_eq!(host.runtime.tick().polled, 0);
    }
    assert_eq!(host.runtime.stats().polls, initial.polls);
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let wake = wake.lock().unwrap().as_ref().unwrap().clone();
            thread::spawn(move || {
                for _ in 0..1000 {
                    wake.wake_by_ref();
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(host.runtime.stats().queued, 1);
    assert_eq!(host.runtime.stats().wakeups, initial.wakeups + 1);
    assert_eq!(host.runtime.tick().polled, 1);
    assert_eq!(polls.get(), 2);
}

#[test]
fn self_wakes_are_fair_and_budgeted_without_polling_to_convergence() {
    let host = Host::with_options(TaskOptions {
        polls_per_tick: 1,
        ..Default::default()
    });
    let tasks = host.runtime.scope();
    let first = tasks.spawn(async {
        for _ in 0..3 {
            time::yield_now().await;
        }
        1
    });
    let second = tasks.spawn(async { 2 });
    assert_eq!(host.runtime.tick().polled, 1);
    assert!(!first.is_finished());
    assert_eq!(host.runtime.tick().polled, 1);
    assert!(second.is_finished());
    assert_eq!(host.wait(first), Ok(1));
    assert_eq!(host.wait(second), Ok(2));
}

#[test]
fn task_admission_also_bounds_cancelled_ids_waiting_for_a_tick() {
    let host = Host::with_options(TaskOptions {
        max_tasks: 2,
        ..Default::default()
    });
    let tasks = host.runtime.scope();
    tasks.spawn(pending::<()>());
    tasks.spawn(pending::<()>());
    let rejected = tasks.spawn(async { 3 });
    tasks.cancel_all();
    assert_eq!(host.runtime.stats().active, 0);
    assert_eq!(host.runtime.stats().queued, 2);
    let also_rejected = tasks.spawn(async { 4 });
    assert_eq!(host.wait(rejected), Err(TaskError::AtCapacity));
    assert_eq!(host.wait(also_rejected), Err(TaskError::AtCapacity));
    assert_eq!(host.wait(tasks.spawn(async { 5 })), Ok(5));
}

#[test]
fn stale_wakers_cannot_wake_a_reused_task_slot_or_a_dropped_runtime() {
    let host = Host::new();
    let tasks = host.runtime.scope();
    let saved = Arc::new(Mutex::new(None::<Waker>));
    let capture = saved.clone();
    let first = tasks.spawn(poll_fn(move |cx| {
        *capture.lock().unwrap() = Some(cx.waker().clone());
        Poll::<()>::Pending
    }));
    host.runtime.tick();
    first.cancel();
    host.runtime.tick();
    let next = tasks.spawn(pending::<()>());
    host.runtime.tick();
    let before = host.runtime.stats().polls;
    saved.lock().unwrap().as_ref().unwrap().wake_by_ref();
    assert_eq!(host.runtime.tick().polled, 0);
    assert_eq!(host.runtime.stats().polls, before);
    drop(next);
    drop(tasks);
    drop(host);
    saved.lock().unwrap().take().unwrap().wake();
}

#[test]
fn panics_are_reported_as_task_errors_and_other_tasks_continue() {
    let host = Host::new();
    let tasks = host.runtime.scope();
    let errors = Rc::new(RefCell::new(Vec::new()));
    let out = errors.clone();
    host.runtime
        .set_error_handler(move |error| out.borrow_mut().push(error));
    let failed = tasks.spawn(async {
        panic!("test panic");
    });
    assert_eq!(
        host.wait(failed),
        Err(TaskError::Panicked("test panic".into()))
    );
    assert_eq!(host.wait(tasks.spawn(async { 42 })), Ok(42));
    assert_eq!(errors.borrow().len(), 1);
    assert_eq!(host.runtime.stats().panicked, 1);
}

#[test]
fn spawn_is_rejected_during_render_but_mount_handlers_start_after_commit() {
    let host = Host::new();
    let mut tree = host.tree();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tree.build_root(component(|| {
                task_scope().spawn(async {});
                div()
            }));
        }))
        .is_err()
    );
    let count = Rc::new(Cell::new(0));
    let capture = count.clone();
    tree.build_root(component(move || {
        let count = capture.clone();
        on_mount(async move || {
            count.set(count.get() + 1);
        });
        div()
    }));
    assert_eq!(count.get(), 0);
    host.runtime.tick();
    assert_eq!(count.get(), 1);
}

#[test]
fn mounts_are_once_per_identity_and_failed_tree_updates_do_not_launch_work() {
    #[component]
    fn mounted(count: Rc<Cell<usize>>, version: usize) -> impl voidui::IntoElement {
        on_mount(async move || {
            count.set(count.get() + 1);
        });
        text(version.to_string())
    }
    let host = Host::new();
    let mut tree = host.tree();
    let count = Rc::new(Cell::new(0));
    tree.build_root(mounted(count.clone(), 1).key("a"));
    host.runtime.tick();
    tree.reconcile_root(mounted(count.clone(), 2).key("a"));
    host.runtime.tick();
    assert_eq!(count.get(), 1);
    tree.reconcile_root(mounted(count.clone(), 3).key("b"));
    host.runtime.tick();
    assert_eq!(count.get(), 2);
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tree.build_root(div().child(mounted(count.clone(), 4)).child(component(
            || -> voidui::core::element::Element { panic!("render failed") },
        )));
    }));
    assert!(failed.is_err());
    assert_eq!(host.runtime.stats().active, 0);
    host.runtime.tick();
    assert_eq!(count.get(), 2);
    tree.build_root(div());
}

#[test]
fn unmount_before_first_poll_drops_the_mount_callback_without_running_it() {
    let host = Host::new();
    let mut tree = host.tree();
    let calls = Rc::new(Cell::new(0));
    let capture = calls.clone();
    tree.build_root(component(move || {
        let calls = capture.clone();
        on_mount(async move || {
            calls.set(1);
        });
        div()
    }));
    tree.build_root(div());
    host.runtime.tick();
    assert_eq!(calls.get(), 0);
    assert_eq!(host.runtime.stats().active, 0);
}

#[test]
fn component_scope_is_stable_and_cancelled_on_unmount() {
    let host = Host::new();
    let mut tree = host.tree();
    let out = Rc::new(RefCell::new(None::<TaskScope>));
    let capture = out.clone();
    tree.build_root(component(move || {
        let first = task_scope();
        let second = task_scope();
        *capture.borrow_mut() = Some(first.clone());
        let _ = second;
        div()
    }));
    let tasks = out.borrow().as_ref().unwrap().clone();
    let task = tasks.spawn(pending::<()>());
    host.runtime.tick();
    assert_eq!(tasks.active_tasks(), 1);
    tree.build_root(div());
    assert!(!tasks.is_open());
    assert_eq!(host.wait(task), Err(TaskError::Cancelled));
}

#[test]
fn async_events_can_borrow_captures_and_update_state_after_await() {
    let host = Host::new();
    let mut tree = host.tree();
    tree.build_root(component(|| {
        let count = state(|| 0);
        div()
            .child(text(count.get().to_string()).id("count"))
            .child(div().tag("button").id("button").on_click(async move || {
                time::yield_now().await;
                count.update(|value| *value += 1);
            }))
    }));
    let button = tree.find_by_id("button").unwrap();
    assert!(tree.click(button));
    assert!(tree.click(button));
    host.until(|| host.runtime.stats().active == 0);
    assert_eq!(tree.flush_updates(), 1);
    assert_eq!(
        tree.text_content(tree.find_by_id("count").unwrap()),
        Some("2")
    );
}

#[test]
fn callback_replacement_preserves_in_flight_calls_but_node_removal_cancels_them() {
    #[component]
    fn button(label: String, output: Rc<RefCell<Vec<String>>>) -> impl voidui::IntoElement {
        div().id("button").on_click(async move || {
            time::yield_now().await;
            output.borrow_mut().push(label.clone());
        })
    }
    let host = Host::new();
    let mut tree = host.tree();
    let output = Rc::new(RefCell::new(Vec::new()));
    tree.build_root(button("old".into(), output.clone()));
    let id = tree.find_by_id("button").unwrap();
    tree.click(id);
    host.runtime.tick();
    tree.reconcile_root(button("new".into(), output.clone()));
    host.runtime.tick();
    assert_eq!(*output.borrow(), ["old"]);
    assert_eq!(tree.find_by_id("button"), Some(id));
    tree.click(id);
    host.until(|| host.runtime.stats().active == 0);
    assert_eq!(*output.borrow(), ["old", "new"]);
    tree.click(id);
    host.runtime.tick();
    tree.remove_subtree(id);
    host.runtime.tick();
    assert_eq!(*output.borrow(), ["old", "new"]);
    assert_eq!(host.runtime.stats().active, 0);
}

#[test]
fn disabled_async_handlers_do_not_spawn_and_handler_errors_are_observable() {
    let host = Host::new();
    let mut tree = host.tree();
    let errors = Rc::new(RefCell::new(Vec::new()));
    let output = errors.clone();
    host.runtime
        .set_error_handler(move |error| output.borrow_mut().push(error));
    let make = || div().on_click(async || -> Result<(), &'static str> { Err("rejected") });
    tree.build_root(make().attr("disabled", ""));
    assert!(!tree.click(tree.root().unwrap()));
    assert_eq!(host.runtime.stats().spawned, 0);
    tree.reconcile_root(make());
    tree.click(tree.root().unwrap());
    host.runtime.tick();
    assert_eq!(*errors.borrow(), [TaskError::Handler("rejected".into())]);
}

#[test]
fn async_callback_values_work_as_repeatable_component_inputs() {
    let host = Host::new();
    let tasks = host.runtime.scope();
    let values = Rc::new(RefCell::new(Vec::new()));
    let out = values.clone();
    let callback = AsyncCallback::new(async move |value: usize| {
        time::yield_now().await;
        out.borrow_mut().push(value);
        value * 2
    });
    let first = tasks.spawn(callback.call(1));
    let second = tasks.spawn(callback.clone().call(2));
    drop(callback);
    assert_eq!(host.wait(first), Ok(2));
    assert_eq!(host.wait(second), Ok(4));
    assert_eq!(*values.borrow(), [1, 2]);
}

#[test]
fn component_window_and_application_scopes_have_distinct_lifetimes() {
    let host = Host::new();
    let mut tree = host.tree();
    let scopes = Rc::new(RefCell::new(Vec::new()));
    let out = scopes.clone();
    tree.build_root(component(move || {
        *out.borrow_mut() = vec![task_scope(), window_task_scope(), app_task_scope()];
        div()
    }));
    let handles: Vec<_> = scopes
        .borrow()
        .iter()
        .map(|scope| scope.spawn(pending::<()>()))
        .collect();
    host.runtime.tick();
    tree.build_root(div());
    assert!(!scopes.borrow()[0].is_open());
    assert!(scopes.borrow()[1].is_open());
    assert_eq!(host.runtime.stats().active, 2);
    drop(tree);
    assert!(!scopes.borrow()[1].is_open());
    assert!(scopes.borrow()[2].is_open());
    assert_eq!(host.runtime.stats().active, 1);
    host.runtime.shutdown();
    assert!(!scopes.borrow()[2].is_open());
    for handle in handles {
        assert_eq!(host.wait(handle), Err(TaskError::Cancelled));
    }
}

#[test]
fn background_and_workers_run_off_thread_and_resume_local_state_on_ui() {
    let host = Host::new();
    let tasks = host.runtime.scope();
    let ui = thread::current().id();
    let background = tasks.spawn_background(async move {
        assert_ne!(thread::current().id(), ui);
        tokio::time::sleep(Duration::from_millis(1)).await;
        workers::compute(move || {
            assert_ne!(thread::current().id(), ui);
            20
        })
        .await
        .unwrap()
    });
    let task = tasks.spawn(async move {
        let value = background.await.unwrap();
        assert_eq!(thread::current().id(), ui);
        let extra = workers::blocking(move || {
            assert_ne!(thread::current().id(), ui);
            22
        })
        .await
        .unwrap();
        assert_eq!(thread::current().id(), ui);
        value + extra
    });
    assert_eq!(host.wait(task), Ok(42));
    assert!(host.runtime.stats().background_started);
}

#[test]
fn cancelled_background_tasks_drop_their_future_and_prestart_cancellation_is_lazy() {
    let host = Host::new();
    let tasks = host.runtime.scope();
    let never = tasks.spawn_background(async {
        tokio::time::sleep(Duration::from_secs(60)).await;
    });
    never.cancel();
    assert_eq!(host.wait(never), Err(TaskError::Cancelled));
    assert!(!host.runtime.stats().background_started);
    struct Notify(mpsc::Sender<()>);
    impl Drop for Notify {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }
    let (dropped, wait_drop) = mpsc::channel();
    let (started, wait_start) = mpsc::channel();
    let task = tasks.spawn_background(async move {
        let _notify = Notify(dropped);
        started.send(()).unwrap();
        pending::<()>().await;
    });
    host.runtime.tick();
    wait_start.recv_timeout(TEST_TIMEOUT).unwrap();
    task.cancel();
    assert_eq!(host.wait(task), Err(TaskError::Cancelled));
    wait_drop.recv_timeout(TEST_TIMEOUT).unwrap();
}

#[test]
fn worker_capacity_is_held_until_cancelled_running_work_actually_finishes() {
    let host = Host::with_options(TaskOptions {
        compute_threads: 1,
        compute_queue_capacity: 0,
        blocking_threads: 1,
        ..Default::default()
    });
    let tasks = host.runtime.scope();
    let (started, wait_start) = mpsc::channel();
    let (finish, wait_finish) = mpsc::channel();
    let first = tasks.spawn(async move {
        workers::compute(move || {
            started.send(()).unwrap();
            wait_finish.recv_timeout(TEST_TIMEOUT).unwrap();
            1
        })
        .await
    });
    host.runtime.tick();
    wait_start.recv_timeout(TEST_TIMEOUT).unwrap();
    first.cancel();
    assert_eq!(host.wait(first), Err(TaskError::Cancelled));
    assert_eq!(host.runtime.stats().compute_in_flight, 1);
    let rejected = tasks.spawn(async { workers::compute(|| 2).await });
    assert_eq!(host.wait(rejected), Ok(Err(TaskError::WorkerQueueFull)));
    // A blocked CPU job does not occupy the blocking lane's admission permit.
    assert_eq!(
        host.wait(tasks.spawn(async { workers::blocking(|| 3).await })),
        Ok(Ok(3))
    );
    finish.send(()).unwrap();
}

#[test]
fn cancellation_of_a_worker_waiter_releases_its_admission_without_running_it() {
    let host = Host::with_options(TaskOptions {
        compute_threads: 1,
        compute_queue_capacity: 1,
        ..Default::default()
    });
    let tasks = host.runtime.scope();
    let (started, wait_start) = mpsc::channel();
    let (finish, wait_finish) = mpsc::channel();
    let first = tasks.spawn(async move {
        workers::compute(move || {
            started.send(()).unwrap();
            wait_finish.recv_timeout(TEST_TIMEOUT).unwrap();
        })
        .await
    });
    host.runtime.tick();
    wait_start.recv_timeout(TEST_TIMEOUT).unwrap();
    let ran = Arc::new(AtomicUsize::new(0));
    let output = ran.clone();
    let queued = tasks.spawn(async move {
        workers::compute(move || {
            output.fetch_add(1, Ordering::SeqCst);
        })
        .await
    });
    host.runtime.tick();
    assert_eq!(host.runtime.stats().compute_in_flight, 2);
    queued.cancel();
    assert_eq!(host.wait(queued), Err(TaskError::Cancelled));
    assert_eq!(host.runtime.stats().compute_in_flight, 1);
    finish.send(()).unwrap();
    assert_eq!(host.wait(first), Ok(Ok(())));
    assert_eq!(ran.load(Ordering::SeqCst), 0);
}

#[test]
fn timers_and_timeouts_wake_without_a_render_loop() {
    let host = Host::new();
    let tasks = host.runtime.scope();
    let task = tasks.spawn(async {
        time::sleep(Duration::from_millis(1)).await.unwrap();
        time::timeout(Duration::from_millis(1), pending::<()>()).await
    });
    assert_eq!(host.wait(task), Ok(Err(TaskError::TimedOut)));
    assert_eq!(
        host.wait(tasks.spawn(async { time::timeout(Duration::from_secs(1), async { 7 }).await })),
        Ok(Ok(7))
    );
}

#[test]
fn whole_file_helpers_round_trip_and_report_missing_files() {
    struct Temp(std::path::PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let path = std::env::temp_dir().join(format!("voidui-task-test-{}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    let temp = Temp(path.clone());
    let host = Host::new();
    let tasks = host.runtime.scope();
    let task = tasks.spawn(async move {
        let path = path.join("sample.txt");
        files::write(&path, b"hello async".to_vec()).await.unwrap();
        assert_eq!(files::read(&path).await.unwrap(), b"hello async");
        assert_eq!(
            files::read_text(&path).await.unwrap().as_str(),
            "hello async"
        );
        files::read(path.with_extension("missing"))
            .await
            .unwrap_err()
            .kind()
    });
    assert_eq!(host.wait(task), Ok(std::io::ErrorKind::NotFound));
    drop(temp);
}

#[test]
fn real_network_io_uses_the_background_driver() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(TEST_TIMEOUT)).unwrap();
        let mut input = [0; 4];
        stream.read_exact(&mut input).unwrap();
        assert_eq!(&input, b"ping");
        stream.write_all(b"pong").unwrap();
    });
    let host = Host::new();
    let tasks = host.runtime.scope();
    let task = tasks.spawn_background(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        stream.write_all(b"ping").await.unwrap();
        let mut output = [0; 4];
        stream.read_exact(&mut output).await.unwrap();
        output
    });
    assert_eq!(host.wait(task), Ok(*b"pong"));
    server.join().unwrap();
}

#[test]
fn external_tokio_runtime_is_reused_and_not_shutdown_by_the_ui_host() {
    let external = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let host = Host::with_runtime(
        TaskRuntime::with_tokio_handle(TaskOptions::default(), external.handle().clone()).unwrap(),
    );
    let tasks = host.runtime.scope();
    assert_eq!(
        host.wait(tasks.spawn_background(async {
            tokio::time::sleep(Duration::from_millis(1)).await;
            42
        })),
        Ok(42)
    );
    host.runtime.shutdown();
    drop(host);
    assert_eq!(
        external.block_on(async { tokio::task::spawn(async { 7 }).await.unwrap() }),
        7
    );
}

#[test]
fn shutdown_cancels_capturing_tasks_even_when_runtime_clones_survive() {
    let host = Host::new();
    let runtime = host.runtime.clone();
    let tasks = runtime.application_scope();
    let captured_runtime = runtime.clone();
    let task = tasks.spawn(async move {
        let _retained = captured_runtime;
        pending::<()>().await;
    });
    runtime.tick();
    runtime.shutdown();
    assert_eq!(host.wait(task), Err(TaskError::Cancelled));
    assert_eq!(
        host.wait(tasks.spawn(async {})),
        Err(TaskError::ScopeClosed)
    );
    assert_eq!(runtime.stats().active, 0);
}

#[test]
fn background_panics_are_reported_and_budget_options_are_validated() {
    let host = Host::new();
    let tasks = host.runtime.scope();
    let errors = Rc::new(RefCell::new(Vec::new()));
    let output = errors.clone();
    host.runtime
        .set_error_handler(move |error| output.borrow_mut().push(error));
    assert_eq!(
        host.wait(tasks.spawn_background(async {
            panic!("background panic");
        })),
        Err(TaskError::Panicked("background panic".into()))
    );
    assert_eq!(errors.borrow().len(), 1);
    assert!(
        TaskRuntime::new(TaskOptions {
            max_tasks: 0,
            ..Default::default()
        })
        .is_err()
    );
    assert!(
        TaskRuntime::new(TaskOptions {
            blocking_threads: 0,
            ..Default::default()
        })
        .is_err()
    );
    assert!(
        TaskRuntime::new(TaskOptions {
            compute_queue_capacity: usize::MAX,
            ..Default::default()
        })
        .is_err()
    );
}

#[test]
fn late_waker_installation_notifies_already_ready_work() {
    let runtime = TaskRuntime::default();
    let tasks = runtime.scope();
    let task = tasks.spawn(async { 42 });
    let (send, receive) = mpsc::channel();
    runtime.set_waker(move || {
        let _ = send.send(());
    });
    receive.recv_timeout(TEST_TIMEOUT).unwrap();
    runtime.tick();
    assert!(task.is_finished());
}

#[test]
fn empty_application_shutdown_closes_its_supplied_task_runtime() {
    let runtime = TaskRuntime::default();
    let scope = runtime.application_scope();
    let task = scope.spawn(pending::<()>());
    voidui::Application::new()
        .task_runtime(runtime.clone())
        .run()
        .unwrap();
    assert!(task.is_finished());
    assert!(!scope.is_open());
    assert_eq!(runtime.stats().active, 0);
}

#[test]
fn reentrant_poll_and_panicking_error_handler_do_not_poison_the_executor() {
    let host = Host::new();
    let tasks = host.runtime.scope();
    host.runtime
        .set_error_handler(|_| panic!("error handler failed"));
    let nested = host.runtime.clone();
    let task = tasks.spawn(async move {
        nested.tick();
    });
    assert!(matches!(host.wait(task), Err(TaskError::Panicked(_))));
    assert_eq!(host.wait(tasks.spawn(async { 42 })), Ok(42));
}

#[test]
fn concurrent_background_completion_does_not_lose_host_wakeups() {
    let host = Host::with_options(TaskOptions {
        background_threads: 4,
        polls_per_tick: 7,
        ..Default::default()
    });
    let tasks = host.runtime.scope();
    let output = Rc::new(Cell::new(0));
    let count = 512;
    for value in 0..count {
        let background = tasks.spawn_background(async move {
            tokio::task::yield_now().await;
            value
        });
        let output = output.clone();
        tasks.spawn(async move {
            let value = background.await.unwrap();
            // Read the current total after await; the left operand of an addition
            // is otherwise evaluated before suspension and becomes a stale snapshot.
            output.set(output.get() + value);
        });
    }
    host.until(|| host.runtime.stats().active == 0);
    assert_eq!(output.get(), (0..count).sum::<u64>());
    assert_eq!(host.runtime.stats().completed, 2 * count);
}

#[test]
fn switching_async_to_sync_cancels_existing_node_work() {
    let host = Host::new();
    let mut tree = host.tree();
    tree.build_root(div().on_click(async || {
        pending::<()>().await;
    }));
    let id = tree.root().unwrap();
    tree.click(id);
    host.runtime.tick();
    assert_eq!(host.runtime.stats().active, 1);
    tree.reconcile_root(div().on_click(|| {}));
    assert_eq!(tree.root(), Some(id));
    assert_eq!(host.runtime.stats().active, 0);
}

#[test]
fn file_and_worker_helpers_outside_task_context_return_errors() {
    use futures_util::FutureExt;
    assert_eq!(
        workers::compute(|| 1).now_or_never(),
        Some(Err(TaskError::NoRuntime))
    );
    assert_eq!(
        time::sleep(Duration::ZERO).now_or_never(),
        Some(Err(TaskError::NoRuntime))
    );
    let error = files::read("unused-without-a-runtime")
        .now_or_never()
        .unwrap()
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::Other);
}
