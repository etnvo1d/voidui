use super::{
    TaskError,
    queue::Control,
    runtime::{Inner, TaskId, TaskRuntime},
};
use futures_channel::oneshot;
use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    future::Future,
    ops::Deref,
    pin::Pin,
    rc::{Rc, Weak},
    sync::{Arc, atomic::Ordering},
    task::{Context, Poll},
};

pub(super) struct ScopeState {
    pub runtime: Weak<Inner>,
    pub ids: RefCell<HashSet<TaskId>>,
    pub closed: Cell<bool>,
}
impl ScopeState {
    fn close(&self) {
        if self.closed.replace(true) {
            return;
        }
        let ids = std::mem::take(&mut *self.ids.borrow_mut());
        if let Some(inner) = self.runtime.upgrade() {
            let runtime = TaskRuntime(inner);
            for id in ids {
                runtime.cancel_now(id);
            }
        }
    }
}

/// A weak UI-thread handle. Retaining it does not keep its owner or tasks alive.
#[derive(Clone)]
pub struct TaskScope(pub(super) Weak<ScopeState>);
impl TaskScope {
    pub fn is_open(&self) -> bool {
        self.0.upgrade().is_some_and(|scope| {
            !scope.closed.get()
                && scope
                    .runtime
                    .upgrade()
                    .is_some_and(|runtime| !runtime.closed.get())
        })
    }
    /// Start one local future. Calls during component rendering panic; use on_mount.
    /// Dropping the returned handle leaves the task owned by this scope.
    pub fn spawn<T: 'static>(&self, future: impl Future<Output = T> + 'static) -> Task<T> {
        self.spawn_result(async move { Ok(future.await) })
    }
    pub(super) fn spawn_result<T: 'static>(
        &self,
        future: impl Future<Output = Result<T, TaskError>> + 'static,
    ) -> Task<T> {
        crate::core::state::assert_not_rendering();
        let Some(scope) = self.0.upgrade().filter(|scope| !scope.closed.get()) else {
            return Task::failed(TaskError::ScopeClosed);
        };
        let Some(inner) = scope.runtime.upgrade() else {
            return Task::failed(TaskError::ScopeClosed);
        };
        TaskRuntime(inner).spawn(&scope, future)
    }
    /// Run a Send future on the shared Tokio backend. Construct runtime-dependent
    /// operations inside the async block so they see the backend's runtime context.
    pub fn spawn_background<T: Send + 'static>(
        &self,
        future: impl Future<Output = T> + Send + 'static,
    ) -> Task<T> {
        let backend = self
            .0
            .upgrade()
            .and_then(|s| s.runtime.upgrade())
            .map(|r| r.backend.clone());
        self.spawn_result(async move {
            let backend = backend.ok_or(TaskError::ScopeClosed)?;
            backend.spawn(future).await
        })
    }
    /// Cancel current tasks while leaving the scope open for later submissions.
    pub fn cancel_all(&self) {
        if let Some(scope) = self.0.upgrade()
            && let Some(inner) = scope.runtime.upgrade()
        {
            let ids: Vec<_> = scope.ids.borrow().iter().copied().collect();
            let runtime = TaskRuntime(inner);
            for id in ids {
                runtime.cancel_now(id);
            }
        }
    }
    pub fn active_tasks(&self) -> usize {
        self.0.upgrade().map_or(0, |scope| scope.ids.borrow().len())
    }
    pub(crate) fn report(&self, error: TaskError) {
        if let Some(inner) = self.0.upgrade().and_then(|s| s.runtime.upgrade()) {
            TaskRuntime(inner).report(error);
        }
    }
    pub(crate) fn spawn_handler<R: super::HandlerOutput>(
        &self,
        future: impl Future<Output = R> + 'static,
    ) {
        let scope = self.clone();
        let task = self.spawn(async move {
            if let Some(error) = future.await.into_task_error() {
                scope.report(error);
            }
        });
        // Admission failures must remain visible even when an event has no caller
        // awaiting its task handle. Ordinary user task results remain typed.
        if let Some(error) = task.admission_error.as_ref() {
            self.report(error.clone());
        }
    }
}

/// Keep this owner alive while tasks should run. Its handles are weak; dropping
/// the owner cancels tasks even if callbacks or the tasks retain cloned handles.
pub struct OwnedTaskScope {
    state: Rc<ScopeState>,
    handle: TaskScope,
}
impl OwnedTaskScope {
    pub(super) fn new(runtime: &TaskRuntime) -> Self {
        let state = Rc::new(ScopeState {
            runtime: Rc::downgrade(&runtime.0),
            ids: RefCell::default(),
            closed: Cell::new(false),
        });
        Self {
            handle: TaskScope(Rc::downgrade(&state)),
            state,
        }
    }
    pub fn handle(&self) -> TaskScope {
        self.handle.clone()
    }
    /// Permanently close this scope, immediately dropping suspended local futures.
    pub fn close(&self) {
        self.state.close();
    }
}
impl Deref for OwnedTaskScope {
    type Target = TaskScope;
    fn deref(&self) -> &Self::Target {
        &self.handle
    }
}
impl Drop for OwnedTaskScope {
    fn drop(&mut self) {
        self.state.close();
    }
}

/// A single-consumer result handle. Await it to observe completion or cancellation.
/// Dropping it does not cancel; use cancel() or close the owning scope.
pub struct Task<T> {
    pub(super) receiver: oneshot::Receiver<Result<T, TaskError>>,
    pub(super) control: Option<Arc<Control>>,
    pub(super) admission_error: Option<TaskError>,
}
impl<T> Task<T> {
    pub(super) fn failed(error: TaskError) -> Self {
        let (sender, receiver) = oneshot::channel();
        let _ = sender.send(Err(error.clone()));
        Self {
            receiver,
            control: None,
            admission_error: Some(error),
        }
    }
    /// Request cancellation. A currently executing poll must return before its
    /// future can be dropped; external side effects are not rolled back.
    pub fn cancel(&self) {
        if let Some(control) = &self.control {
            control.cancel();
        }
    }
    pub fn is_finished(&self) -> bool {
        self.control
            .as_ref()
            .is_none_or(|control| control.finished.load(Ordering::Acquire))
    }
}
impl<T> Future for Task<T> {
    type Output = Result<T, TaskError>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.get_mut().receiver)
            .poll(cx)
            .map(|result| result.unwrap_or(Err(TaskError::Cancelled)))
    }
}
