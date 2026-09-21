use super::{
    TaskError,
    backend::Backend,
    queue::{Control, ReadyQueue},
    scope::{OwnedTaskScope, ScopeState, Task, TaskScope},
};
use futures_channel::oneshot;
use futures_util::FutureExt;
use slotmap::{SlotMap, new_key_type};
use std::{
    cell::{Cell, OnceCell, RefCell},
    collections::VecDeque,
    future::Future,
    panic::AssertUnwindSafe,
    pin::Pin,
    rc::{Rc, Weak},
    sync::{Arc, atomic::Ordering},
    task::{Context, Waker},
    time::{Duration, Instant},
};

new_key_type! { pub(super) struct TaskId; }

/// Shared application budgets. Defaults create no threads until an operation
/// actually needs the background backend. Limits apply to work submitted here;
/// third-party code spawning directly onto Tokio manages its own concurrency.
#[derive(Clone, Debug)]
pub struct TaskOptions {
    /// Maximum registered tasks across all scopes sharing this runtime.
    pub max_tasks: usize,
    /// Maximum ready-queue visits per tick, including stale cancellation entries.
    pub polls_per_tick: usize,
    /// Cooperative time budget; a poll in progress cannot be preempted.
    pub poll_budget: Duration,
    /// Tokio async threads, created lazily and retained until backend shutdown.
    pub background_threads: usize,
    /// Maximum simultaneously running CPU jobs submitted through workers.
    pub compute_threads: usize,
    /// Maximum simultaneously running blocking jobs submitted through workers.
    pub blocking_threads: usize,
    /// Additional CPU jobs allowed to wait for execution; zero rejects when busy.
    pub compute_queue_capacity: usize,
    /// Additional blocking jobs allowed to wait; zero rejects when busy.
    pub blocking_queue_capacity: usize,
    /// Idle lifetime of the owned backend's blocking/compute worker threads.
    pub worker_keep_alive: Duration,
}
impl Default for TaskOptions {
    fn default() -> Self {
        Self {
            max_tasks: 4096,
            polls_per_tick: 256,
            poll_budget: Duration::from_millis(2),
            background_threads: 1,
            compute_threads: std::thread::available_parallelism()
                .map_or(1, |n| n.get().saturating_sub(1).max(1)),
            blocking_threads: 4,
            compute_queue_capacity: 256,
            blocking_queue_capacity: 256,
            worker_keep_alive: Duration::from_secs(10),
        }
    }
}
impl TaskOptions {
    fn validate(&self) -> Result<(), TaskError> {
        let invalid = |message: &str| TaskError::InvalidOptions(message.into());
        if self.max_tasks == 0 || self.polls_per_tick == 0 || self.poll_budget.is_zero() {
            return Err(invalid(
                "task capacity and polling budgets must be positive",
            ));
        }
        if self.background_threads == 0 || self.compute_threads == 0 || self.blocking_threads == 0 {
            return Err(invalid("thread limits must be positive"));
        }
        for (threads, queued) in [
            (self.compute_threads, self.compute_queue_capacity),
            (self.blocking_threads, self.blocking_queue_capacity),
        ] {
            if threads
                .checked_add(queued)
                .is_none_or(|n| n > tokio::sync::Semaphore::MAX_PERMITS)
            {
                return Err(invalid("worker capacity exceeds the semaphore limit"));
            }
        }
        if self
            .compute_threads
            .checked_add(self.blocking_threads)
            .is_none()
        {
            return Err(invalid("combined worker thread limit overflows"));
        }
        Ok(())
    }
}

/// Counters describe this executor, not process-wide CPU or memory usage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TaskStats {
    pub active: usize,
    /// Queued task IDs, including cancellations not yet drained by the host.
    pub queued: usize,
    pub spawned: u64,
    pub completed: u64,
    pub cancelled: u64,
    pub panicked: u64,
    pub polls: u64,
    pub wakeups: u64,
    pub background_started: bool,
    pub compute_in_flight: usize,
    pub blocking_in_flight: usize,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tick {
    pub polled: usize,
    pub has_ready: bool,
}

struct Entry {
    future: Pin<Box<dyn Future<Output = ()>>>,
    control: Arc<Control>,
    scope: Weak<ScopeState>,
    id: TaskId,
}
impl Drop for Entry {
    fn drop(&mut self) {
        self.control.finished.store(true, Ordering::Release);
        if let Some(scope) = self.scope.upgrade() {
            scope.ids.borrow_mut().remove(&self.id);
        }
    }
}
pub(super) struct Inner {
    options: TaskOptions,
    queue: Arc<ReadyQueue>,
    tasks: RefCell<SlotMap<TaskId, Option<Entry>>>,
    scratch: RefCell<VecDeque<TaskId>>,
    polling: Cell<bool>,
    pub(super) closed: Cell<bool>,
    stats: Cell<TaskStats>,
    error_handler: RefCell<Rc<dyn Fn(TaskError)>>,
    application: OnceCell<OwnedTaskScope>,
    pub backend: Arc<Backend>,
    pub(super) updates: Cell<usize>,
}
impl Drop for Inner {
    fn drop(&mut self) {
        self.queue.close();
        self.tasks.get_mut().clear();
    }
}

/// A cloneable UI-thread executor. Native applications share one across windows.
/// Headless hosts call tick() after committed tree updates and when woken.
#[derive(Clone)]
pub struct TaskRuntime(pub(super) Rc<Inner>);
impl Default for TaskRuntime {
    fn default() -> Self {
        Self::new(TaskOptions::default()).expect("default task options are valid")
    }
}
impl TaskRuntime {
    pub fn new(options: TaskOptions) -> Result<Self, TaskError> {
        Self::build(options, None)
    }
    /// Reuse a host-owned Tokio runtime. Its I/O/time drivers must be enabled and
    /// continuously driven. Voidui never shuts down an externally supplied runtime.
    pub fn with_tokio_handle(
        options: TaskOptions,
        handle: tokio::runtime::Handle,
    ) -> Result<Self, TaskError> {
        Self::build(options, Some(handle))
    }
    fn build(
        options: TaskOptions,
        handle: Option<tokio::runtime::Handle>,
    ) -> Result<Self, TaskError> {
        options.validate()?;
        let backend = Arc::new(Backend::new(options.clone(), handle));
        Ok(Self(Rc::new(Inner {
            options,
            backend,
            queue: Arc::default(),
            tasks: RefCell::new(SlotMap::with_key()),
            scratch: RefCell::default(),
            polling: Cell::new(false),
            closed: Cell::new(false),
            stats: Cell::default(),
            error_handler: RefCell::new(Rc::new(|error| log::error!("{error}"))),
            application: OnceCell::new(),
            updates: Cell::new(0),
        })))
    }
    /// Stop accepting work and cancel all scopes, even when callers retain runtime
    /// clones. Native applications call this at exit. Started blocking work may
    /// finish later; a supplied external Tokio runtime remains owned by its host.
    pub fn shutdown(&self) {
        if self.0.closed.replace(true) {
            return;
        }
        self.0.queue.close();
        let tasks = std::mem::take(&mut *self.0.tasks.borrow_mut());
        self.count(|stats| {
            stats.cancelled += tasks.values().filter(|entry| entry.is_some()).count() as u64
        });
        drop(tasks);
        if let Some(scope) = self.0.application.get() {
            scope.close();
        }
        self.0.backend.close();
    }
    pub fn options(&self) -> &TaskOptions {
        &self.0.options
    }
    pub fn scope(&self) -> OwnedTaskScope {
        OwnedTaskScope::new(self)
    }
    pub fn application_scope(&self) -> TaskScope {
        self.0.application.get_or_init(|| self.scope()).handle()
    }
    /// Install a thread-safe notification callback. It must only request host
    /// work, never synchronously poll the executor. Existing ready work wakes it.
    pub fn set_waker(&self, wake: impl Fn() + Send + Sync + 'static) {
        self.0.queue.set_waker(Arc::new(wake));
    }
    /// Called on the UI thread for panics, rejected event tasks, and handler errors.
    /// A typed result from ordinary spawn is returned only through its Task handle.
    pub fn set_error_handler(&self, handler: impl Fn(TaskError) + 'static) {
        *self.0.error_handler.borrow_mut() = Rc::new(handler);
    }
    pub(crate) fn report(&self, error: TaskError) {
        let handler = self.0.error_handler.borrow().clone();
        if let Err(panic) = std::panic::catch_unwind(AssertUnwindSafe(|| handler(error))) {
            log::error!("task error handler panicked: {}", super::panic_error(panic));
        }
    }
    pub fn has_ready_tasks(&self) -> bool {
        self.0.queue.stats().0 != 0
    }
    pub fn stats(&self) -> TaskStats {
        let mut stats = self.0.stats.get();
        (stats.queued, stats.wakeups) = self.0.queue.stats();
        stats.active = self.0.tasks.borrow().len();
        stats.background_started = self.0.backend.started();
        (stats.compute_in_flight, stats.blocking_in_flight) = self.0.backend.in_flight();
        stats
    }
    fn count(&self, update: impl FnOnce(&mut TaskStats)) {
        let mut stats = self.0.stats.get();
        update(&mut stats);
        self.0.stats.set(stats);
    }
    pub(super) fn spawn<T: 'static>(
        &self,
        scope: &Rc<ScopeState>,
        future: impl Future<Output = Result<T, TaskError>> + 'static,
    ) -> Task<T> {
        if self.0.closed.get() {
            return Task::failed(TaskError::ScopeClosed);
        }
        if self.0.tasks.borrow().len() >= self.0.options.max_tasks
            || self.0.queue.stats().0 >= self.0.options.max_tasks
        {
            return Task::failed(TaskError::AtCapacity);
        }
        let (sender, receiver) = oneshot::channel();
        let runtime = Rc::downgrade(&self.0);
        let future = async move {
            let result = AssertUnwindSafe(future).catch_unwind().await;
            let result = result.unwrap_or_else(|payload| Err(super::panic_error(payload)));
            if let Err(error @ TaskError::Panicked(_)) = &result
                && let Some(inner) = runtime.upgrade()
            {
                let runtime = TaskRuntime(inner);
                runtime.count(|stats| stats.panicked += 1);
                runtime.report(error.clone());
            }
            let _ = sender.send(result);
        };
        let mut control = None;
        let id = self.0.tasks.borrow_mut().insert_with_key(|id| {
            let wake = Control::new(id, &self.0.queue);
            control = Some(wake.clone());
            Some(Entry {
                id,
                control: wake,
                scope: Rc::downgrade(scope),
                future: Box::pin(future),
            })
        });
        scope.ids.borrow_mut().insert(id);
        self.count(|stats| stats.spawned += 1);
        let control = control.unwrap();
        control.schedule();
        Task {
            receiver,
            control: Some(control),
            admission_error: None,
        }
    }
    pub(super) fn cancel_now(&self, id: TaskId) {
        let entry = self.0.tasks.borrow_mut().remove(id);
        if let Some(Some(entry)) = entry {
            self.count(|stats| stats.cancelled += 1);
            // Drop user futures only after releasing registry borrows. Destructors
            // may close other scopes or submit new work to a surviving scope.
            drop(entry);
        }
    }
    /// Advance one bounded snapshot of ready tasks. A self-waking task runs at
    /// most once per tick. This never scans suspended tasks or waits for a frame.
    pub fn tick(&self) -> Tick {
        crate::core::state::assert_not_rendering();
        assert_eq!(
            self.0.updates.get(),
            0,
            "tasks cannot be polled during tree reconciliation"
        );
        assert!(
            !self.0.polling.replace(true),
            "task polling cannot reenter itself"
        );
        struct Guard<'a>(&'a Cell<bool>);
        impl Drop for Guard<'_> {
            fn drop(&mut self) {
                self.0.set(false);
            }
        }
        let _guard = Guard(&self.0.polling);
        if !self.has_ready_tasks() {
            return Tick::default();
        }
        let mut batch = std::mem::take(&mut *self.0.scratch.borrow_mut());
        self.0.queue.take_batch(&mut batch);
        let start = Instant::now();
        let mut polled = 0;
        let mut visited = 0;
        while visited < self.0.options.polls_per_tick {
            if visited > 0 && start.elapsed() >= self.0.options.poll_budget {
                break;
            }
            let Some(id) = batch.pop_front() else {
                break;
            };
            visited += 1;
            let entry = self.0.tasks.borrow_mut().get_mut(id).and_then(Option::take);
            let Some(mut entry) = entry else {
                continue;
            };
            entry.control.before_poll();
            if entry.control.cancelled.load(Ordering::Acquire)
                || entry.scope.upgrade().is_none_or(|scope| scope.closed.get())
            {
                self.0.tasks.borrow_mut().remove(id);
                self.count(|stats| stats.cancelled += 1);
                drop(entry);
                continue;
            }
            let waker = Waker::from(entry.control.clone());
            let mut cx = Context::from_waker(&waker);
            let before = Instant::now();
            let result = super::backend::with_local_backend(self.0.backend.clone(), || {
                entry.future.as_mut().poll(&mut cx)
            });
            polled += 1;
            self.count(|stats| stats.polls += 1);
            if before.elapsed() > self.0.options.poll_budget {
                log::warn!(
                    "UI task poll exceeded {:?}; move blocking or CPU work to workers",
                    self.0.options.poll_budget
                );
            }
            if result.is_ready() {
                self.0.tasks.borrow_mut().remove(id);
                self.count(|stats| stats.completed += 1);
                drop(entry);
            } else if entry.control.cancelled.load(Ordering::Acquire)
                || entry.scope.upgrade().is_none_or(|scope| scope.closed.get())
                || !self.0.tasks.borrow().contains_key(id)
            {
                self.0.tasks.borrow_mut().remove(id);
                self.count(|stats| stats.cancelled += 1);
                drop(entry);
            } else {
                *self.0.tasks.borrow_mut().get_mut(id).unwrap() = Some(entry);
            }
        }
        self.0.queue.finish_batch(&mut batch);
        *self.0.scratch.borrow_mut() = batch;
        Tick {
            polled,
            has_ready: self.has_ready_tasks(),
        }
    }
}
