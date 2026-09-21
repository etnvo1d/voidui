//! Finite work with separate admission and concurrency budgets for CPU and I/O.
use super::{TaskError, backend};

/// Offload CPU-intensive work. Do not access UI state in this closure.
/// Awaiting returns to the executor that called this function.
pub async fn compute<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, TaskError> {
    backend::current()?.work(true, work).await
}

/// Offload a finite blocking operation. Cancelling the await cannot interrupt an
/// operation already running. Persistent media loops need their own backend.
pub async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, TaskError> {
    backend::current()?.work(false, work).await
}
