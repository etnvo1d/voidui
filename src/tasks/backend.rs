//! A lazy shared I/O runtime with admission-controlled compute and blocking lanes.
use super::{TaskError, TaskOptions};
use std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
};
use tokio::{
    runtime::{Handle, Runtime},
    sync::Semaphore,
    task::JoinHandle,
};

thread_local! { static LOCAL_BACKEND: RefCell<Option<Arc<Backend>>> = const { RefCell::new(None) }; }
tokio::task_local! { static BACKEND: Arc<Backend>; }

pub(super) fn with_local_backend<T>(backend: Arc<Backend>, body: impl FnOnce() -> T) -> T {
    struct Restore(Option<Arc<Backend>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            LOCAL_BACKEND.with(|slot| *slot.borrow_mut() = self.0.take());
        }
    }
    let _guard = Restore(LOCAL_BACKEND.with(|slot| slot.replace(Some(backend))));
    body()
}
pub(super) fn current() -> Result<Arc<Backend>, TaskError> {
    BACKEND
        .try_with(Arc::clone)
        .ok()
        .or_else(|| LOCAL_BACKEND.with(|slot| slot.borrow().clone()))
        .ok_or(TaskError::NoRuntime)
}

struct Lane {
    running: Arc<Semaphore>,
    admitted: Arc<Semaphore>,
    capacity: usize,
}
impl Lane {
    fn new(threads: usize, queued: usize) -> Self {
        Self {
            running: Arc::new(Semaphore::new(threads)),
            admitted: Arc::new(Semaphore::new(threads + queued)),
            capacity: threads + queued,
        }
    }
    fn in_flight(&self) -> usize {
        self.capacity - self.admitted.available_permits()
    }
}

pub(super) struct Backend {
    options: TaskOptions,
    external: Option<Handle>,
    owned: Mutex<Option<Result<Runtime, String>>>,
    closed: AtomicBool,
    started: AtomicBool,
    compute: Lane,
    blocking: Lane,
}
impl Backend {
    pub fn new(options: TaskOptions, external: Option<Handle>) -> Self {
        Self {
            compute: Lane::new(options.compute_threads, options.compute_queue_capacity),
            blocking: Lane::new(options.blocking_threads, options.blocking_queue_capacity),
            started: AtomicBool::new(external.is_some()),
            options,
            external,
            owned: Mutex::new(None),
            closed: AtomicBool::new(false),
        }
    }
    pub fn handle(&self) -> Result<Handle, TaskError> {
        let mut owned = self.owned.lock().unwrap();
        if self.closed.load(Ordering::Acquire) {
            return Err(TaskError::ScopeClosed);
        }
        if let Some(handle) = &self.external {
            return Ok(handle.clone());
        }
        owned
            .get_or_insert_with(|| {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(self.options.background_threads)
                    .max_blocking_threads(
                        self.options.compute_threads + self.options.blocking_threads,
                    )
                    .thread_keep_alive(self.options.worker_keep_alive)
                    .thread_name("voidui-worker")
                    .enable_all()
                    .build()
                    .map_err(|error| error.to_string());
                self.started.store(runtime.is_ok(), Ordering::Release);
                runtime
            })
            .as_ref()
            .map(|runtime| runtime.handle().clone())
            .map_err(|error| TaskError::Runtime(error.clone()))
    }
    pub fn started(&self) -> bool {
        self.started.load(Ordering::Acquire)
    }
    pub fn close(&self) {
        let runtime = {
            let mut owned = self.owned.lock().unwrap();
            self.closed.store(true, Ordering::Release);
            owned.take()
        };
        if let Some(Ok(runtime)) = runtime {
            runtime.shutdown_background();
        }
    }
    pub fn in_flight(&self) -> (usize, usize) {
        (self.compute.in_flight(), self.blocking.in_flight())
    }
    pub async fn spawn<T: Send + 'static>(
        self: &Arc<Self>,
        future: impl Future<Output = T> + Send + 'static,
    ) -> Result<T, TaskError> {
        let handle = self.handle()?;
        let task = handle.spawn(BACKEND.scope(self.clone(), future));
        AbortOnDrop(task).await
    }
    pub async fn work<T: Send + 'static>(
        self: &Arc<Self>,
        compute: bool,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, TaskError> {
        let lane = if compute {
            &self.compute
        } else {
            &self.blocking
        };
        // Reserve total capacity before waiting or creating a Tokio job. Admission
        // bounds both the running work and closures retained by permit waiters.
        let admitted = lane
            .admitted
            .clone()
            .try_acquire_owned()
            .map_err(|_| TaskError::WorkerQueueFull)?;
        let running = lane
            .running
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| TaskError::Cancelled)?;
        let handle = self.handle()?;
        let cancelled = Arc::new(AtomicBool::new(false));
        struct Cancel(Arc<AtomicBool>);
        impl Drop for Cancel {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }
        let _cancel = Cancel(cancelled.clone());
        let task = handle.spawn_blocking(move || {
            // Permits stay with the actual operation after its caller is cancelled.
            // Otherwise repeated cancellation could exceed the concurrency budget.
            let (_admitted, _running) = (admitted, running);
            if cancelled.load(Ordering::Acquire) {
                return Err(TaskError::Cancelled);
            }
            Ok(work())
        });
        AbortOnDrop(task).await?
    }
}
impl Drop for Backend {
    fn drop(&mut self) {
        if let Some(Ok(runtime)) = self.owned.get_mut().unwrap().take() {
            // Started blocking calls cannot be interrupted. Never join them on the
            // UI thread; their owned inputs/permits remain alive until they finish.
            runtime.shutdown_background();
        }
    }
}

struct AbortOnDrop<T>(JoinHandle<T>);
impl<T> Future for AbortOnDrop<T> {
    type Output = Result<T, TaskError>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.get_mut().0).poll(cx).map(|result| {
            result.map_err(|error| {
                if error.is_panic() {
                    super::panic_error(error.into_panic())
                } else {
                    TaskError::Cancelled
                }
            })
        })
    }
}
impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}
