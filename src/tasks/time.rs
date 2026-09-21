//! Event-driven timers and cooperative yielding. No periodic UI timer is installed.
use super::{TaskError, backend};
use futures_util::future::{Either, select};
use std::{
    future::{Future, poll_fn},
    task::Poll,
    time::Duration,
};

/// Sleep using the shared I/O driver. A UI task resumes on the UI thread.
pub async fn sleep(duration: Duration) -> Result<(), TaskError> {
    let backend = backend::current()?;
    let handle = backend.handle()?;
    let timer = {
        let _entered = handle.enter();
        tokio::time::sleep(duration)
    };
    timer.await;
    Ok(())
}

/// Drop the supplied future if its deadline wins. This does not undo external
/// effects or stop a blocking call that already started.
pub async fn timeout<F: Future>(duration: Duration, future: F) -> Result<F::Output, TaskError> {
    let future = std::pin::pin!(future);
    let timer = std::pin::pin!(sleep(duration));
    match select(future, timer).await {
        Either::Left((result, _)) => Ok(result),
        Either::Right((result, _)) => {
            result?;
            Err(TaskError::TimedOut)
        }
    }
}

/// Yield until a later executor turn. Use this for small cooperative UI work;
/// move expensive computation to workers rather than relying on repeated yields.
pub async fn yield_now() {
    let mut yielded = false;
    poll_fn(move |cx| {
        if yielded {
            Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    })
    .await
}
