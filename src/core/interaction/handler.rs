//! Reusable sync/async adaptation, independent of any particular widget or event.
use crate::{
    core::event::{EventResponse, EventResult},
    tasks::{HandlerOutput, OwnedTaskScope, TaskError, TaskRuntime, TaskScope},
};
use std::{panic::AssertUnwindSafe, rc::Rc};

mod sealed {
    pub trait Output {}
    impl Output for () {}
    impl Output for super::EventResponse {}
    impl Output for super::EventResult {}
    impl<R: super::EventOutput, E: std::fmt::Display> Output for Result<R, E> {}
}
/// Synchronous callbacks may return (), EventResponse, EventResult, or Result of
/// these. Errors use the same application error handler as asynchronous tasks.
pub trait EventOutput: sealed::Output + 'static {
    #[doc(hidden)]
    fn into_response(self) -> Result<EventResponse, TaskError>;
}
impl EventOutput for () {
    fn into_response(self) -> Result<EventResponse, TaskError> {
        Ok(EventResponse::CONTINUE)
    }
}
impl EventOutput for EventResponse {
    fn into_response(self) -> Result<EventResponse, TaskError> {
        Ok(self)
    }
}
impl EventOutput for EventResult {
    fn into_response(self) -> Result<EventResponse, TaskError> {
        Ok(self.into())
    }
}
impl<R: EventOutput, E: std::fmt::Display + 'static> EventOutput for Result<R, E> {
    fn into_response(self) -> Result<EventResponse, TaskError> {
        self.map_err(|e| TaskError::Handler(e.to_string()))?
            .into_response()
    }
}

/// Inferred implementation markers keep synchronous and asynchronous blanket
/// implementations disjoint. Applications never need to specify these types.
#[doc(hidden)]
pub mod mode {
    pub struct Sync;
    pub struct Async;
    pub struct SyncNoArgs;
    pub struct AsyncNoArgs;
}

/// Converts a repeatable callback to a reusable event handler. An argument-taking
/// closure may need an explicit event parameter type with Rust's trait inference.
pub trait IntoEventHandler<E, M>: 'static {
    #[doc(hidden)]
    fn into_event_handler(self) -> EventHandler<E>;
}
impl<E: 'static, F: Fn(E) -> R + 'static, R: EventOutput> IntoEventHandler<E, mode::Sync> for F {
    fn into_event_handler(self) -> EventHandler<E> {
        EventHandler(Callback::Sync(Rc::new(move |e| self(e).into_response())))
    }
}
impl<E: 'static, F: Fn() -> R + 'static, R: EventOutput> IntoEventHandler<E, mode::SyncNoArgs>
    for F
{
    fn into_event_handler(self) -> EventHandler<E> {
        EventHandler(Callback::Sync(Rc::new(move |_| self().into_response())))
    }
}
impl<E: 'static, F: AsyncFn(E) -> R + 'static, R: HandlerOutput> IntoEventHandler<E, mode::Async>
    for F
{
    fn into_event_handler(self) -> EventHandler<E> {
        let callback = Rc::new(self);
        EventHandler(Callback::Async(Box::new(AsyncHandler {
            callback: Rc::new(move |event, scope| {
                let callback = callback.clone();
                // Keep the callable alive while its lending future borrows captures.
                scope.spawn_handler(async move { callback(event).await });
            }),
            scope: None,
        })))
    }
}
impl<E: 'static, F: AsyncFn() -> R + 'static, R: HandlerOutput>
    IntoEventHandler<E, mode::AsyncNoArgs> for F
{
    fn into_event_handler(self) -> EventHandler<E> {
        let callback = Rc::new(self);
        EventHandler(Callback::Async(Box::new(AsyncHandler {
            callback: Rc::new(move |_, scope| {
                let callback = callback.clone();
                scope.spawn_handler(async move { callback().await });
            }),
            scope: None,
        })))
    }
}

type SyncCallback<E> = dyn Fn(E) -> Result<EventResponse, TaskError>;
type AsyncCallback<E> = dyn Fn(E, &TaskScope);
enum Callback<E> {
    Sync(Rc<SyncCallback<E>>),
    Async(Box<AsyncHandler<E>>),
}
struct AsyncHandler<E> {
    callback: Rc<AsyncCallback<E>>,
    scope: Option<OwnedTaskScope>,
}

/// Store this alongside a custom component's event slot, then call dispatch for
/// each event. Reconcile callback descriptions with replace; drop to cancel work.
/// Cloning creates an unmounted description, not shared live task ownership.
pub struct EventHandler<E>(Callback<E>);
impl<E> Clone for EventHandler<E> {
    fn clone(&self) -> Self {
        Self(match &self.0 {
            Callback::Sync(callback) => Callback::Sync(callback.clone()),
            Callback::Async(handler) => Callback::Async(Box::new(AsyncHandler {
                callback: handler.callback.clone(),
                scope: None,
            })),
        })
    }
}
impl<E: 'static> EventHandler<E> {
    pub fn new<M>(callback: impl IntoEventHandler<E, M>) -> Self {
        callback.into_event_handler()
    }
    /// Executes sync work now, or schedules an independently scoped async call.
    pub fn dispatch(&mut self, event: E, runtime: &TaskRuntime) -> EventResponse {
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| match &mut self.0 {
            Callback::Sync(callback) => callback(event),
            Callback::Async(handler) => {
                (handler.callback)(event, handler.scope.get_or_insert_with(|| runtime.scope()));
                Ok(EventResponse::CONTINUE)
            }
        }))
        .unwrap_or_else(|payload| Err(crate::tasks::panic_error(payload)));
        result.unwrap_or_else(|error| {
            runtime.report(error);
            EventResponse::CONTINUE
        })
    }
    /// Preserve in-flight calls when replacing an async callback. Switching to a
    /// sync callback or dropping this slot cancels its previous async calls.
    pub fn replace(&mut self, mut next: Self) {
        if let (Callback::Async(old), Callback::Async(new)) = (&mut self.0, &mut next.0) {
            new.scope = old.scope.take();
        }
        *self = next;
    }
    pub fn cancel(&mut self) {
        if let Callback::Async(handler) = &mut self.0 {
            handler.scope.take();
        }
    }
    pub(crate) fn map_input<A: 'static>(self, map: fn(A) -> E) -> EventHandler<A> {
        match self.0 {
            Callback::Sync(callback) => {
                EventHandler(Callback::Sync(Rc::new(move |value| callback(map(value)))))
            }
            Callback::Async(handler) => EventHandler(Callback::Async(Box::new(AsyncHandler {
                callback: Rc::new(move |value, scope| (handler.callback)(map(value), scope)),
                scope: None,
            }))),
        }
    }
}
