//! Lazy native clipboard ownership. Merely displaying or selecting text creates
//! no clipboard connection/thread; the backend is opened only on an actual copy.
use anyhow::Result;
use copypasta::ClipboardProvider;
use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};
use winit::event_loop::OwnedDisplayHandle;

#[derive(Default)]
pub(crate) struct Clipboard {
    // Field order drops the backend before releasing its native display lease.
    provider: Option<Box<dyn ClipboardProvider>>,
    display: Option<OwnedDisplayHandle>,
}
impl Clipboard {
    pub fn new(display: OwnedDisplayHandle) -> Self {
        Self {
            display: Some(display),
            ..Default::default()
        }
    }
    pub fn read(&mut self) -> anyhow::Result<String> {
        if self.provider.is_none() {
            let display = self
                .display
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("clipboard has no display"))?;
            self.provider = Some(create(display)?);
        }
        self.provider
            .as_mut()
            .unwrap()
            .get_contents()
            .map_err(|e| anyhow::anyhow!("clipboard paste failed: {e}"))
    }
    pub fn write(&mut self, text: String) -> anyhow::Result<()> {
        let display = self
            .display
            .clone()
            .ok_or_else(|| anyhow::anyhow!("clipboard has no display"))?;
        self.write_with(text, || create(&display))
    }
    fn write_with(
        &mut self,
        text: String,
        create: impl FnOnce() -> anyhow::Result<Box<dyn ClipboardProvider>>,
    ) -> anyhow::Result<()> {
        if self.provider.is_none() {
            self.provider = Some(create()?);
        }
        self.provider
            .as_mut()
            .unwrap()
            .set_contents(text)
            .map_err(|e| anyhow::anyhow!("clipboard copy failed: {e}"))
    }
}
thread_local! {
    /// The running application's clipboard, shared by its windows. Only the UI
    /// thread that owns it can reach it; headless trees and tests have none.
    /// A weak reference keeps the native backend's lifetime with the application
    /// rather than with the thread.
    static CURRENT: RefCell<Weak<RefCell<Clipboard>>> = const { RefCell::new(Weak::new()) };
}

/// Publish the application's clipboard to its own UI thread.
pub(crate) fn install(clipboard: &Rc<RefCell<Clipboard>>) {
    CURRENT.with(|current| *current.borrow_mut() = Rc::downgrade(clipboard));
}

fn with_current<R>(use_clipboard: impl FnOnce(&mut Clipboard) -> Result<R>) -> Result<R> {
    let clipboard = CURRENT
        .with(|current| current.borrow().upgrade())
        .ok_or_else(|| anyhow::anyhow!("no application clipboard on this thread"))?;
    // A native shortcut releases its borrow before dispatching, so an event
    // callback never finds the clipboard in use; report it instead of panicking.
    let mut clipboard = clipboard
        .try_borrow_mut()
        .map_err(|_| anyhow::anyhow!("clipboard is already in use"))?;
    use_clipboard(&mut clipboard)
}

/// Read the system clipboard as text, opening the native backend on first use.
/// Call it from the UI thread of a running application.
pub fn read_text() -> Result<String> {
    with_current(|clipboard| clipboard.read())
}

/// Replace the system clipboard's text.
pub fn write_text(text: impl Into<String>) -> Result<()> {
    let text = text.into();
    with_current(|clipboard| clipboard.write(text))
}

fn create(display: &OwnedDisplayHandle) -> anyhow::Result<Box<dyn ClipboardProvider>> {
    #[cfg(target_os = "linux")]
    {
        use raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
        if let RawDisplayHandle::Wayland(display) = display.display_handle()?.as_raw() {
            // The owning Clipboard retains Winit's display lease until after the
            // provider is destroyed, including during event-loop error unwinding.
            let (_, clipboard) = unsafe {
                copypasta::wayland_clipboard::create_clipboards_from_external(
                    display.display.as_ptr(),
                )
            };
            return Ok(Box::new(clipboard));
        }
    }
    let _ = display;
    Ok(Box::new(copypasta::ClipboardContext::new().map_err(
        |e| anyhow::anyhow!("clipboard unavailable: {e}"),
    )?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    struct MemoryClipboard(Arc<Mutex<String>>);
    impl ClipboardProvider for MemoryClipboard {
        fn get_contents(
            &mut self,
        ) -> std::result::Result<String, Box<dyn std::error::Error + Send + Sync>> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn set_contents(
            &mut self,
            value: String,
        ) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
            *self.0.lock().unwrap() = value;
            Ok(())
        }
    }
    #[test]
    fn shared_access_needs_a_running_application() {
        // No application installed its clipboard on this test thread.
        assert!(read_text().is_err());
        assert!(write_text("text").is_err());
        let clipboard = Rc::new(RefCell::new(Clipboard::default()));
        install(&clipboard);
        let _borrowed = clipboard.borrow_mut();
        assert!(write_text("text").is_err());
    }

    #[test]
    fn backend_is_lazy_reused_and_can_retry_creation_failure() {
        // Test lifetime and writes without reading or changing the OS clipboard.
        let mut clipboard = Clipboard::default();
        assert!(clipboard.provider.is_none());
        assert!(
            clipboard
                .write_with("first".into(), || anyhow::bail!("unavailable"))
                .is_err()
        );
        assert!(clipboard.provider.is_none());
        let value = Arc::new(Mutex::new(String::new()));
        let creations = AtomicUsize::new(0);
        for text in ["first", "中文\nsecond"] {
            clipboard
                .write_with(text.into(), || {
                    creations.fetch_add(1, Ordering::Relaxed);
                    Ok(Box::new(MemoryClipboard(value.clone())))
                })
                .unwrap();
            assert_eq!(*value.lock().unwrap(), text);
        }
        assert_eq!(creations.load(Ordering::Relaxed), 1);
    }
}
