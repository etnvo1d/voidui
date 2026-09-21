//! Per-tree task ownership and deferred mount work. Tree updates commit starts as
//! one batch, so a render/reconciliation panic cannot launch a partial mount.
use super::{OwnedTaskScope, TaskRuntime, TaskScope};
use std::{
    cell::{Cell, OnceCell, RefCell},
    rc::{Rc, Weak},
};

#[derive(Default)]
pub(crate) struct MountHook {
    pub scheduled: Cell<bool>,
}
struct Deferred {
    hook: Option<Weak<MountHook>>,
    start: Option<Box<dyn FnOnce()>>,
}
impl Deferred {
    fn commit(mut self) {
        if self
            .hook
            .as_ref()
            .is_none_or(|hook| hook.upgrade().is_some())
        {
            (self.start.take().unwrap())();
        }
        // Committed hooks stay marked, even when admission was rejected. Such
        // failures go through the error handler instead of causing a render loop.
        self.start = None;
    }
}
impl Drop for Deferred {
    fn drop(&mut self) {
        if self.start.is_some()
            && let Some(hook) = self.hook.as_ref().and_then(Weak::upgrade)
        {
            hook.scheduled.set(false);
        }
    }
}
struct Host {
    runtime: TaskRuntime,
    window: OnceCell<OwnedTaskScope>,
    depth: Cell<usize>,
    failed: Cell<bool>,
    deferred: RefCell<Vec<Deferred>>,
}
#[derive(Clone)]
pub(crate) struct TaskHost(Rc<Host>);
impl TaskHost {
    pub fn new(runtime: TaskRuntime) -> Self {
        Self(Rc::new(Host {
            runtime,
            window: OnceCell::new(),
            depth: Cell::new(0),
            failed: Cell::new(false),
            deferred: RefCell::default(),
        }))
    }
    pub fn runtime(&self) -> &TaskRuntime {
        &self.0.runtime
    }
    pub fn window_scope(&self) -> TaskScope {
        self.0
            .window
            .get_or_init(|| self.runtime().scope())
            .handle()
    }
    pub fn update(&self) -> Update {
        if self.0.depth.get() == 0 {
            self.0.failed.set(false);
        }
        self.0.depth.set(self.0.depth.get() + 1);
        let runtime = &self.runtime().0;
        runtime.updates.set(runtime.updates.get() + 1);
        Update(self.clone())
    }
    pub fn defer(&self, hook: &Rc<MountHook>, start: impl FnOnce() + 'static) {
        assert!(
            self.0.depth.get() > 0,
            "mount handlers require a tree update"
        );
        self.0.deferred.borrow_mut().push(Deferred {
            hook: Some(Rc::downgrade(hook)),
            start: Some(Box::new(start)),
        });
    }
    /// Stage a resource input change until the entire tree update succeeds.
    /// The closure must hold weak ownership so an unmount discards pending work.
    pub fn defer_effect(&self, start: impl FnOnce() + 'static) {
        assert!(self.0.depth.get() > 0, "effects require a tree update");
        self.0.deferred.borrow_mut().push(Deferred {
            hook: None,
            start: Some(Box::new(start)),
        });
    }
}
pub(crate) struct Update(TaskHost);
impl Drop for Update {
    fn drop(&mut self) {
        let host = &self.0.0;
        if std::thread::panicking() {
            host.failed.set(true);
        }
        host.depth.set(host.depth.get() - 1);
        host.runtime.0.updates.set(host.runtime.0.updates.get() - 1);
        if host.depth.get() == 0 {
            let pending = std::mem::take(&mut *host.deferred.borrow_mut());
            if !host.failed.get() {
                for deferred in pending {
                    deferred.commit();
                }
            }
        }
    }
}
