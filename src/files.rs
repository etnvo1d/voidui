//! Whole-file helpers backed by the bounded blocking lane. These retain the entire
//! file in memory; use a streaming I/O library in a background task for large files.
use crate::tasks::workers;
use std::{io, path::Path};
use voidui_gpui_wgpu::SharedString;

pub async fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
    let path = path.as_ref().to_owned();
    workers::blocking(move || std::fs::read(path))
        .await
        .map_err(io::Error::other)?
}

/// Read UTF-8 text into shared storage, so cloning it for text widgets is cheap.
pub async fn read_text(path: impl AsRef<Path>) -> io::Result<SharedString> {
    let path = path.as_ref().to_owned();
    workers::blocking(move || std::fs::read_to_string(path))
        .await
        .map_err(io::Error::other)?
        .map(SharedString::from)
}

/// Replace the file's contents. Cancellation does not undo a write already started.
/// Atomic replacement or durability requires an application-specific save protocol.
pub async fn write(path: impl AsRef<Path>, contents: impl Into<Vec<u8>>) -> io::Result<()> {
    let path = path.as_ref().to_owned();
    let contents = contents.into();
    workers::blocking(move || std::fs::write(path, contents))
        .await
        .map_err(io::Error::other)?
}
