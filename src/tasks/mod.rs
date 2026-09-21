//! Scoped, event-driven execution of ordinary Rust futures.
#![doc = include_str!("../../docs/tasks.md")]

mod backend;
pub(crate) mod host;
mod queue;
mod runtime;
mod scope;
pub mod time;
pub mod workers;

pub use runtime::{TaskOptions, TaskRuntime, TaskStats, Tick};
pub use scope::{OwnedTaskScope, Task, TaskScope};

use std::{any::Any, fmt};

/// Execution failures are separate from the value returned by your future.
/// A task returning Result<T, E> is awaited as Result<Result<T, E>, TaskError>.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TaskError {
    Cancelled,
    ScopeClosed,
    AtCapacity,
    WorkerQueueFull,
    NoRuntime,
    TimedOut,
    Panicked(String),
    Handler(String),
    Runtime(String),
    InvalidOptions(String),
}
impl fmt::Display for TaskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => f.write_str("task was cancelled"),
            Self::ScopeClosed => f.write_str("task scope is closed"),
            Self::AtCapacity => f.write_str("task capacity was reached"),
            Self::WorkerQueueFull => f.write_str("worker queue capacity was reached"),
            Self::NoRuntime => f.write_str("this operation requires a voidui task"),
            Self::TimedOut => f.write_str("operation timed out"),
            Self::Panicked(message) => write!(f, "task panicked: {message}"),
            Self::Handler(message) => write!(f, "async handler failed: {message}"),
            Self::Runtime(message) => write!(f, "background runtime failed: {message}"),
            Self::InvalidOptions(message) => write!(f, "invalid task options: {message}"),
        }
    }
}
impl std::error::Error for TaskError {}

pub(crate) fn panic_error(payload: Box<dyn Any + Send>) -> TaskError {
    let message = if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else {
        "non-string panic payload".into()
    };
    TaskError::Panicked(message)
}

mod sealed {
    pub trait Output {}
    impl Output for () {}
    impl<E: std::fmt::Display> Output for Result<(), E> {}
}

/// Async event and mount handlers may return () or Result<(), E>.
/// Errors go to the runtime's error handler; the default logs them.
pub trait HandlerOutput: sealed::Output + 'static {
    #[doc(hidden)]
    fn into_task_error(self) -> Option<TaskError>;
}
impl HandlerOutput for () {
    fn into_task_error(self) -> Option<TaskError> {
        None
    }
}
impl<E: fmt::Display + 'static> HandlerOutput for Result<(), E> {
    fn into_task_error(self) -> Option<TaskError> {
        self.err()
            .map(|error| TaskError::Handler(error.to_string()))
    }
}
