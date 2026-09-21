//! Main-thread state slots with weak handles and render-time dependency tracking.
use super::reconcile::ComponentId;
use std::{
    any::Any,
    cell::{Cell, RefCell},
    collections::HashMap,
    marker::PhantomData,
    rc::{Rc, Weak},
};

#[derive(Default)]
pub(crate) struct UpdateQueue {
    pub pending: RefCell<Vec<ComponentId>>,
    pub contexts: RefCell<Vec<ComponentId>>,
    pub wake: RefCell<Option<Box<dyn Fn()>>>,
}
enum SignalTarget {
    Component {
        id: ComponentId,
        queue: Weak<UpdateQueue>,
    },
    Widget(super::updates::WidgetInvalidator),
}
pub(crate) struct Signal {
    target: SignalTarget,
    pub mounted: Cell<bool>,
    pub dirty: Cell<bool>,
    pub epoch: Cell<u64>,
}
impl Signal {
    pub(crate) fn component(id: ComponentId, queue: Weak<UpdateQueue>) -> Self {
        Self {
            target: SignalTarget::Component { id, queue },
            mounted: true.into(),
            dirty: false.into(),
            epoch: 0.into(),
        }
    }
    fn invalidate(&self, context: bool) {
        if !self.mounted.get() {
            return;
        }
        match &self.target {
            SignalTarget::Widget(invalidator) => invalidator.repaint(),
            SignalTarget::Component { id, queue } => {
                // A context update must reach this flush even if an unmount
                // destructor already queued the reader as ordinary next-batch
                // work. Duplicate context entries are skipped after rendering.
                if self.dirty.replace(true) && !context {
                    return;
                }
                if let Some(queue) = queue.upgrade() {
                    let wake =
                        queue.pending.borrow().is_empty() && queue.contexts.borrow().is_empty();
                    let mut pending = if context {
                        queue.contexts.borrow_mut()
                    } else {
                        queue.pending.borrow_mut()
                    };
                    pending.push(*id);
                    drop(pending);
                    if wake && let Some(wake) = queue.wake.borrow().as_ref() {
                        wake();
                    }
                }
            }
        }
    }
}
/// A retained widget's state subscription. Dropping it disconnects that reader;
/// subscribing a widget does not subscribe the component that constructs it.
pub struct StateSubscription {
    _signal: Rc<Signal>,
}

pub(crate) struct Subscription {
    signal: Weak<Signal>,
    epoch: u64,
}
impl Subscription {
    fn active(&self) -> Option<Rc<Signal>> {
        self.signal
            .upgrade()
            .filter(|s| s.mounted.get() && s.epoch.get() == self.epoch)
    }
}
// The usual one-reader state has no subscriber allocation. Shared state switches
// to an indexed table, avoiding quadratic registration for large lists of readers.
#[derive(Default)]
pub(crate) enum Subscribers {
    #[default]
    Empty,
    One(Subscription),
    // Box the uncommon map header to keep Empty/One slots compact.
    #[allow(clippy::box_collection)]
    Many(Box<HashMap<usize, Subscription>>),
}
impl Subscribers {
    /// Subscribe only the component currently reading this channel.
    pub(crate) fn track(&mut self) {
        ACTIVE.with(|active| {
            if let Some(frame) = active.borrow().last() {
                self.subscribe(&frame.signal);
            }
        });
    }
    pub(crate) fn is_active(&self) -> bool {
        match self {
            Self::Empty => false,
            Self::One(value) => value.active().is_some(),
            Self::Many(values) => values.values().any(|value| value.active().is_some()),
        }
    }
    fn subscribe(&mut self, signal: &Rc<Signal>) {
        let key = Rc::as_ptr(signal) as usize;
        let next = Subscription {
            signal: Rc::downgrade(signal),
            epoch: signal.epoch.get(),
        };
        match self {
            Self::Empty => *self = Self::One(next),
            Self::One(old) if old.signal.as_ptr() as usize == key || old.active().is_none() => {
                *old = next
            }
            Self::One(_) => {
                let Self::One(old) = std::mem::take(self) else {
                    unreachable!()
                };
                *self = Self::Many(Box::new(HashMap::from([
                    (old.signal.as_ptr() as usize, old),
                    (key, next),
                ])));
            }
            Self::Many(values) => {
                // Prune before growth, not on every read. Weak subscriptions from
                // removed or conditionally inactive readers cannot grow indefinitely.
                if values.len() == values.capacity() && !values.contains_key(&key) {
                    values.retain(|_, subscription| subscription.active().is_some());
                }
                values.insert(key, next);
            }
        }
    }
    pub(crate) fn notify(&mut self) {
        self.notify_change(false);
    }
    pub(crate) fn notify_context(&mut self) {
        self.notify_change(true);
    }
    fn notify_change(&mut self, context: bool) {
        match self {
            Self::Empty => {}
            Self::One(subscription) => {
                if let Some(signal) = subscription.active() {
                    signal.invalidate(context);
                } else {
                    *self = Self::Empty;
                }
            }
            Self::Many(values) => {
                values.retain(|_, subscription| {
                    if let Some(signal) = subscription.active() {
                        signal.invalidate(context);
                        true
                    } else {
                        false
                    }
                });
                if values.len() <= 1 {
                    let next = values
                        .drain()
                        .next()
                        .map(|(_, s)| Self::One(s))
                        .unwrap_or_default();
                    *self = next;
                }
            }
        }
    }
}
slotmap::new_key_type! { struct StateId; }
thread_local! {
    // Weak entries never own user data. Generations reject handles from unmounted
    // components even after the same storage slot has been reused.
    static VALUES: RefCell<slotmap::SlotMap<StateId, Weak<dyn Any>>> = RefCell::default();
}
struct Value<T> {
    id: Cell<Option<StateId>>,
    value: RefCell<T>,
    subscribers: RefCell<Subscribers>,
}
impl<T> Drop for Value<T> {
    fn drop(&mut self) {
        if let Some(id) = self.id.get() {
            // Thread-local destruction may already have destroyed the registry.
            let _ = VALUES.try_with(|values| values.borrow_mut().remove(id));
        }
    }
}

/// A Copy, main-thread handle. The owning component retains the value; copying a
/// handle never extends its lifetime. Stale handles cannot address a reused slot.
///
/// ```compile_fail
/// use voidui::{component, div, state};
/// let view = component(|| {
///     let count = state(|| 0);
///     std::thread::spawn(move || count.set(1)); // UI state cannot cross threads.
///     div()
/// });
/// ```
pub struct State<T> {
    id: StateId,
    // Invariance prevents changing the registered value type through coercion.
    marker: PhantomData<fn(T) -> T>,
    thread: PhantomData<Rc<()>>,
}
impl<T> Copy for State<T> {}
impl<T> Clone for State<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> PartialEq for State<T> {
    fn eq(&self, other: &Self) -> bool {
        self.same_state(other)
    }
}
impl<T> Eq for State<T> {}
impl<T> State<T> {
    /// Compare state-slot identity without reading or subscribing.
    pub fn same_state(&self, other: &Self) -> bool {
        self.id == other.id
    }
    /// Check whether the owning component still retains this value.
    pub fn is_mounted(&self) -> bool {
        VALUES.with(|values| values.borrow().contains_key(self.id))
    }
}
impl<T: 'static> State<T> {
    fn value(&self) -> Option<Rc<Value<T>>> {
        let value = VALUES.with(|values| values.borrow().get(self.id).and_then(Weak::upgrade))?;
        Some(value.downcast::<Value<T>>().expect("state type mismatch"))
    }
    /// Retained widgets can invalidate themselves directly. Keep the guard until
    /// the widget unmounts, and read with with_untracked when synchronizing it.
    pub fn subscribe_widget(
        &self,
        invalidator: super::updates::WidgetInvalidator,
    ) -> StateSubscription {
        let value = self.value().expect("cannot subscribe to unmounted state");
        let signal = Rc::new(Signal {
            target: SignalTarget::Widget(invalidator),
            mounted: true.into(),
            dirty: false.into(),
            epoch: 0.into(),
        });
        value.subscribers.borrow_mut().subscribe(&signal);
        StateSubscription { _signal: signal }
    }
    /// Borrow without creating a component dependency. Custom retained widgets
    /// must arrange their own subscription before using this method.
    pub fn with_untracked<R>(&self, read: impl FnOnce(&T) -> R) -> R {
        let value = self.value().expect("cannot read state after unmount");
        read(&value.value.borrow())
    }
    /// Borrow without cloning. During rendering, subscribe only the current
    /// component; event-time reads do not create dependencies.
    pub fn with<R>(&self, read: impl FnOnce(&T) -> R) -> R {
        let value = self.value().expect("cannot read state after unmount");
        value.subscribers.borrow_mut().track();
        read(&value.value.borrow())
    }
    /// Clone the current value and track a render-time dependency.
    pub fn get(&self) -> T
    where
        T: Clone,
    {
        self.with(Clone::clone)
    }

    /// Replace the value and schedule its current readers. Repeated writes are
    /// batched until the tree flushes. Use set_if_changed to suppress equal writes.
    pub fn set(&self, next: T) {
        self.update(|value| *value = next);
    }

    /// Mutate in place without cloning. Writes during rendering are rejected to
    /// prevent partially rendered trees and self-sustaining redraw loops.
    pub fn update(&self, update: impl FnOnce(&mut T)) {
        let Some(value) = self.value() else {
            return;
        };
        assert_not_rendering();
        // Even a panicking update may have changed T. Notify after releasing the
        // value borrow so a subsequent caught panic cannot leave readers stale.
        let _notify = Notify(&value);
        update(&mut value.value.borrow_mut());
    }
    /// Return false without scheduling when the value is equal or has unmounted.
    pub fn set_if_changed(&self, next: T) -> bool
    where
        T: PartialEq,
    {
        let Some(value) = self.value() else {
            return false;
        };
        assert_not_rendering();
        let mut current = value.value.borrow_mut();
        if *current == next {
            return false;
        }
        *current = next;
        drop(current);
        notify(&value);
        true
    }
}
pub(crate) fn assert_not_rendering() {
    ACTIVE.with(|active| {
        assert!(
            active.borrow().is_empty(),
            "state writes and task execution are not allowed while rendering a component; use an event handler or on_mount"
        )
    });
}
pub(crate) fn is_rendering() -> bool {
    ACTIVE.with(|active| !active.borrow().is_empty())
}
struct Notify<'a, T>(&'a Value<T>);
impl<T> Drop for Notify<'_, T> {
    fn drop(&mut self) {
        notify(self.0);
    }
}
fn notify<T>(value: &Value<T>) {
    value.subscribers.borrow_mut().notify();
}

#[derive(Default)]
pub(crate) struct Hooks {
    values: Vec<Rc<dyn Any>>,
    initialized: bool,
    tasks: Option<crate::tasks::OwnedTaskScope>,
}
impl Drop for Hooks {
    fn drop(&mut self) {
        // Cancel suspended work while the component's values still exist.
        if let Some(tasks) = self.tasks.take() {
            tasks.close();
        }
    }
}
impl Hooks {
    pub fn len(&self) -> usize {
        self.values.len()
    }
}
struct Frame {
    context: Rc<super::environment::ContextScope>,
    window: super::decoration::WindowContext,
    hooks: Hooks,
    signal: Rc<Signal>,
    cursor: usize,
    initializing: bool,
    task_host: crate::tasks::host::TaskHost,
}
thread_local! { static ACTIVE: RefCell<Vec<Frame>> = const { RefCell::new(Vec::new()) }; }

/// Create a slot once per mounted instance. Call state unconditionally and in a
/// stable order; count and type changes are errors, including in release builds.
pub fn state<T: 'static>(init: impl FnOnce() -> T) -> State<T> {
    let value = use_hook(|| Value {
        id: Cell::new(None),
        value: RefCell::new(init()),
        subscribers: RefCell::default(),
    });
    let id = value.id.get().unwrap_or_else(|| {
        let erased: Rc<dyn Any> = value.clone();
        let id = VALUES.with(|values| values.borrow_mut().insert(Rc::downgrade(&erased)));
        value.id.set(Some(id));
        id
    });
    State {
        id,
        marker: PhantomData,
        thread: PhantomData,
    }
}

/// Ordered storage shared by state and lifecycle hooks. Initializers run only on
/// first mount; nested hooks and type/count changes retain the same diagnostics.
pub(crate) fn use_hook<T: 'static>(init: impl FnOnce() -> T) -> Rc<T> {
    let (index, existing) = ACTIVE.with(|active| {
        let mut active = active.borrow_mut();
        let frame = active
            .last_mut()
            .expect("hooks require an active component");
        assert!(
            !frame.initializing,
            "hooks cannot be called from a state initializer"
        );
        let index = frame.cursor;
        frame.cursor += 1;
        if frame.hooks.initialized {
            assert!(
                index < frame.hooks.values.len(),
                "state/hook call count changed; hooks must be unconditional"
            );
        }
        let existing = frame.hooks.values.get(index).cloned();
        frame.initializing = existing.is_none();
        (index, existing)
    });
    match existing {
        Some(value) => value
            .downcast::<T>()
            .unwrap_or_else(|_| panic!("state/hook type changed at slot {index}")),
        None => {
            let value = Rc::new(init());
            ACTIVE.with(|active| {
                let mut active = active.borrow_mut();
                let frame = active.last_mut().unwrap();
                frame.initializing = false;
                // Keep the one-slot case compact, growing only when another hook
                // is actually used by this component.
                if frame.hooks.values.len() == frame.hooks.values.capacity() {
                    let additional = frame.hooks.values.capacity().max(1);
                    frame.hooks.values.reserve_exact(additional);
                }
                frame.hooks.values.push(value.clone());
            });
            value
        }
    }
}

pub(crate) fn task_host() -> crate::tasks::host::TaskHost {
    ACTIVE.with(|active| {
        let active = active.borrow();
        let frame = active
            .last()
            .expect("task scope access requires an active component");
        assert!(
            !frame.initializing,
            "task scopes cannot be accessed from a state initializer"
        );
        frame.task_host.clone()
    })
}
pub(crate) fn component_task_scope() -> crate::tasks::TaskScope {
    ACTIVE.with(|active| {
        let mut active = active.borrow_mut();
        let frame = active
            .last_mut()
            .expect("task_scope() requires an active component");
        assert!(
            !frame.initializing,
            "task scopes cannot be accessed from a state initializer"
        );
        frame
            .hooks
            .tasks
            .get_or_insert_with(|| frame.task_host.runtime().scope())
            .handle()
    })
}

pub(crate) fn render<R>(
    hooks: &mut Hooks,
    signal: Rc<Signal>,
    task_host: crate::tasks::host::TaskHost,
    window: super::decoration::WindowContext,
    context: Rc<super::environment::ContextScope>,
    render: impl FnOnce() -> R,
) -> R {
    // Moving the slot vector into TLS reuses its allocation. No raw pointers,
    // per-hook hash keys, state-value clones, or per-render scope allocations.
    struct Scope<'a>(&'a mut Hooks);
    impl Drop for Scope<'_> {
        fn drop(&mut self) {
            *self.0 = ACTIVE.with(|active| active.borrow_mut().pop().unwrap().hooks);
        }
    }
    signal.dirty.set(false);
    signal.epoch.set(
        signal
            .epoch
            .get()
            .checked_add(1)
            .expect("component render epoch exhausted"),
    );
    ACTIVE.with(|active| {
        active.borrow_mut().push(Frame {
            context,
            window,
            hooks: std::mem::take(hooks),
            signal,
            cursor: 0,
            initializing: false,
            task_host,
        })
    });
    let _scope = Scope(hooks);
    let output = render();
    ACTIVE.with(|active| {
        let mut active = active.borrow_mut();
        let frame = active.last_mut().unwrap();
        if frame.hooks.initialized {
            assert_eq!(
                frame.hooks.values.len(),
                frame.cursor,
                "state() call count changed; hooks must be unconditional"
            );
        }
        frame.hooks.initialized = true;
    });
    output
}

pub(crate) fn window_context() -> super::decoration::WindowContext {
    ACTIVE.with(|frames| {
        frames
            .borrow()
            .last()
            .expect("window_context must be called inside a component")
            .window
            .clone()
    })
}

/// Accessors do not consume ordered hook slots. Providers can be conditional.
pub(crate) fn context_scope() -> Rc<super::environment::ContextScope> {
    ACTIVE.with(|active| {
        let active = active.borrow();
        let frame = active
            .last()
            .expect("context access requires an active component");
        assert!(
            !frame.initializing,
            "context cannot be accessed from a state initializer"
        );
        frame.context.clone()
    })
}

#[cfg(test)]
mod context_queue_tests {
    use super::*;

    #[test]
    fn context_changes_promote_already_dirty_readers_into_the_current_wave() {
        let queue = Rc::new(UpdateQueue::default());
        let id = slotmap::SlotMap::<ComponentId, ()>::with_key().insert(());
        let signal = Signal::component(id, Rc::downgrade(&queue));
        signal.invalidate(false);
        assert_eq!(&*queue.pending.borrow(), &[id]);
        signal.invalidate(true);
        assert_eq!(&*queue.contexts.borrow(), &[id]);
    }
}
