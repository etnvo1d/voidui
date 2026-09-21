//! Component-owned async queries with explicit inputs and latest-result commits.
use super::{
    component::AsyncCallback,
    data::Read,
    state::{self, State},
};
use crate::{OwnedTaskScope, Task, TaskError};
use futures_util::FutureExt;
use std::{
    cell::{Cell, RefCell},
    fmt,
    panic::AssertUnwindSafe,
    rc::Rc,
};

/// Preserve domain errors separately from admission, cancellation, and panics.
#[derive(Debug)]
pub enum ResourceError<E> {
    Load(E),
    Task(TaskError),
}
impl<E: fmt::Display> fmt::Display for ResourceError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Load(e) => e.fmt(f),
            Self::Task(e) => e.fmt(f),
        }
    }
}
impl<E: std::error::Error + 'static> std::error::Error for ResourceError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Load(e) => Some(e),
            Self::Task(e) => Some(e),
        }
    }
}
struct Value<T, E> {
    data: Option<Read<T>>,
    error: Option<Read<ResourceError<E>>>,
    loading: bool,
    reload: RefCell<Option<Rc<dyn Fn()>>>,
}
/// A Copy, component-owned query handle. A refresh retains the last successful
/// snapshot while loading. Data and errors need not implement Clone.
pub struct Resource<T, E> {
    value: State<Value<T, E>>,
}
impl<T, E> Copy for Resource<T, E> {}
impl<T, E> Clone for Resource<T, E> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T, E> PartialEq for Resource<T, E> {
    fn eq(&self, other: &Self) -> bool {
        self.value.same_state(&other.value)
    }
}
impl<T, E> Eq for Resource<T, E> {}
impl<T: 'static, E: 'static> Resource<T, E> {
    pub fn is_mounted(&self) -> bool {
        self.value.is_mounted()
    }
    pub fn is_loading(&self) -> bool {
        self.value.with(|value| value.loading)
    }
    pub fn data(&self) -> Option<Read<T>> {
        self.value.with(|value| value.data.clone())
    }
    pub fn error(&self) -> Option<Read<ResourceError<E>>> {
        self.value.with(|value| value.error.clone())
    }
    /// Borrow the last successful value without copying it.
    pub fn with<R>(&self, read: impl FnOnce(Option<&T>) -> R) -> R {
        self.value.with(|value| read(value.data.as_deref()))
    }
    /// Request another load with the last committed input and loader. Repeated
    /// calls replace the previous request; they do not create a write queue.
    pub fn reload(&self) {
        if !self.is_mounted() {
            return;
        }
        state::assert_not_rendering();
        let reload = self
            .value
            .with_untracked(|value| value.reload.borrow().clone());
        if let Some(reload) = reload {
            reload();
        }
    }
}

struct Job<I, T, E> {
    input: RefCell<Option<I>>,
    loader: RefCell<Option<AsyncCallback<I, Result<T, E>>>>,
    output: Resource<T, E>,
    scope: OwnedTaskScope,
    task: RefCell<Option<Task<()>>>,
    generation: Cell<u64>,
}
struct Completion<I: Clone + 'static, T: 'static, E: 'static> {
    job: std::rc::Weak<Job<I, T, E>>,
    generation: u64,
    finished: bool,
}
impl<I: Clone + 'static, T: 'static, E: 'static> Drop for Completion<I, T, E> {
    fn drop(&mut self) {
        if !self.finished
            && let Some(job) = self.job.upgrade()
        {
            job.finish(
                self.generation,
                Err(ResourceError::Task(TaskError::Cancelled)),
            );
        }
    }
}
impl<I: Clone + 'static, T: 'static, E: 'static> Job<I, T, E> {
    fn start(self: &Rc<Self>) {
        if !self.output.is_mounted() {
            return;
        }
        let generation = self
            .generation
            .get()
            .checked_add(1)
            .expect("resource generation exhausted");
        self.generation.set(generation);
        // Free the previous task before admission. A started blocking operation
        // still holds its backend permit until its actual work has finished.
        self.scope.cancel_all();
        self.task.borrow_mut().take();
        if self.generation.get() != generation {
            return;
        }
        let input = self.input.borrow().as_ref().unwrap().clone();
        let loader = self.loader.borrow().as_ref().unwrap().clone();
        if self
            .output
            .value
            .with_untracked(|value| !value.loading || value.error.is_some())
        {
            self.output.value.update(|value| {
                value.loading = true;
                value.error = None;
            });
        }
        let mut completion = Completion {
            job: Rc::downgrade(self),
            generation,
            finished: false,
        };
        let mut task = self.scope.spawn(async move {
            let result = AssertUnwindSafe(async move { loader.call(input).await })
                .catch_unwind()
                .await;
            let result = match result {
                Ok(result) => result.map_err(ResourceError::Load),
                Err(panic) => Err(ResourceError::Task(crate::tasks::panic_error(panic))),
            };
            completion.finished = true;
            if let Some(job) = completion.job.upgrade() {
                job.finish(generation, result);
            }
        });
        // Admission failures are already-completed tasks. Publish them as a
        // resource error so a rejected query cannot remain loading forever.
        if task.is_finished() {
            if let Some(Err(error)) = (&mut task).now_or_never() {
                self.finish(generation, Err(ResourceError::Task(error)));
            }
        } else {
            *self.task.borrow_mut() = Some(task);
        }
    }
    fn finish(&self, generation: u64, result: Result<T, ResourceError<E>>) {
        if self.generation.get() != generation {
            return;
        }
        // The scope owns execution independently of this result handle. Release
        // the oneshot storage after completion instead of keeping it until reload.
        self.task.borrow_mut().take();
        self.output.value.update(|value| {
            value.loading = false;
            match result {
                Ok(data) => {
                    value.data = Some(Read::new(data));
                    value.error = None;
                }
                Err(error) => value.error = Some(Read::new(error)),
            }
        });
    }
}

/// Load on mount, input changes, and reload(). Work starts only after a successful
/// tree commit and is cancelled on unmount. Unrelated renders do not reload.
///
/// Use for queries, not external writes: cancellation cannot undo a side effect.
/// This hook does not create a global cache, retry failures, or poll while idle.
///
/// ```
/// use voidui::{component, div, resource, state};
/// let view = component(|| {
///     let query = state(|| String::from("hello"));
///     let result = resource(query.get(), async |query| Ok::<_, String>(query.len()));
///     div().child(if result.is_loading() { "Loading" } else { "Ready" })
/// });
/// ```
pub fn resource<I, T, E>(
    input: I,
    load: impl AsyncFn(I) -> Result<T, E> + 'static,
) -> Resource<T, E>
where
    I: Clone + PartialEq + 'static,
    T: 'static,
    E: 'static,
{
    let output = Resource {
        value: state::state(|| Value {
            data: None,
            error: None,
            loading: true,
            reload: RefCell::default(),
        }),
    };
    let host = state::task_host();
    let job = state::use_hook(|| Job {
        input: RefCell::default(),
        loader: RefCell::default(),
        output,
        scope: host.runtime().scope(),
        task: RefCell::default(),
        generation: Cell::new(0),
    });
    output.value.with_untracked(|value| {
        if value.reload.borrow().is_none() {
            let weak = Rc::downgrade(&job);
            *value.reload.borrow_mut() = Some(Rc::new(move || {
                if let Some(job) = weak.upgrade() {
                    job.start();
                }
            }));
        }
    });
    let weak = Rc::downgrade(&job);
    host.defer_effect(move || {
        let Some(job) = weak.upgrade() else {
            return;
        };
        let changed = job.input.borrow().as_ref() != Some(&input);
        // Refresh the loader even when input equality suppresses a new request,
        // so a later manual reload uses the latest committed callback captures.
        let old_loader = job.loader.replace(Some(AsyncCallback::new(load)));
        let old_input = job.input.replace(Some(input));
        drop((old_loader, old_input));
        if changed {
            job.start();
        }
    });
    output
}
