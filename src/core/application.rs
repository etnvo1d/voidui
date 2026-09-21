//! Main-thread desktop runtime with shared GPU/text resources and demand-driven frames.
#![doc = include_str!("../../docs/window.md")]
use anyhow::Error;
use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc, time::Instant};

use voidui_gpui_wgpu::{GpuContext, ParleyTextSystem, Result, TextSystem};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::WindowId,
};

use crate::{
    core::{
        element::{Element, IntoElement},
        window::{AppWindow, WindowOptions},
    },
    platform,
    style::css::Stylesheet,
};

enum RuntimeEvent {
    Reload(crate::style::css::reload::ReloadEvent),
    TasksReady,
}

type EventCallback = Box<dyn FnMut(&WindowEvent, &mut AppWindow) -> Result<()>>;
type FrameCallback = Box<dyn FnMut(&mut AppWindow) -> Result<()>>;

/// Configure windows, then run the desktop event loop on the main thread.
#[derive(Default)]
pub struct Application {
    tasks: crate::tasks::TaskRuntime,
    windows: Vec<(WindowOptions, Element)>,
    text_system: Option<Arc<TextSystem>>,
    stylesheets: Vec<Stylesheet>,
    css_files: Vec<(usize, std::path::PathBuf)>,
    hot_reload: bool,
    on_frame: Option<FrameCallback>,
    on_event: Option<EventCallback>,
}

impl Application {
    pub fn new() -> Self {
        Self::default()
    }

    /// Supply task budgets, an error handler, or a host-owned Tokio backend.
    /// All windows share this executor; constructing it starts no worker threads.
    pub fn task_runtime(mut self, runtime: crate::tasks::TaskRuntime) -> Self {
        self.tasks = runtime;
        self
    }

    /// Share an existing font database, or supply bundled fonts without scanning
    /// system font directories. All windows use this same text system.
    pub fn text_system(mut self, text_system: Arc<TextSystem>) -> Self {
        self.text_system = Some(text_system);
        self
    }

    /// Add an independent native window. GPU device and font resources are shared.
    pub fn window(mut self, options: WindowOptions, root: impl IntoElement) -> Self {
        self.windows.push((options, root.into_element()));
        self
    }

    /// Inspect successful frames, capture a snapshot, or update a window's tree.
    /// This callback does not create a continuous frame loop by itself.
    pub fn on_frame(
        mut self,
        callback: impl FnMut(&mut AppWindow) -> Result<()> + 'static,
    ) -> Self {
        self.on_frame = Some(Box::new(callback));
        self
    }

    /// Handle native input/lifecycle events before the runtime processes them.
    /// Editing `window.tree_mut()` schedules a frame; unrelated input stays idle.
    pub fn on_window_event(
        mut self,
        callback: impl FnMut(&WindowEvent, &mut AppWindow) -> Result<()> + 'static,
    ) -> Self {
        self.on_event = Some(Box::new(callback));
        self
    }

    /// Add a compiled author stylesheet, in source order.
    pub fn stylesheet(mut self, sheet: Stylesheet) -> Self {
        self.stylesheets.push(sheet);
        self
    }

    /// Compile inline CSS once when configuring the application.
    pub fn css(self, source: &str) -> Result<Self> {
        Ok(self.stylesheet(Stylesheet::parse(source)?))
    }

    /// Read and compile a CSS file once. Watching requires css_hot_reload(true).
    pub fn css_file(mut self, path: impl AsRef<std::path::Path>) -> Result<Self> {
        let path = path.as_ref();
        let sheet = Stylesheet::from_file(path)?;
        self.css_files.push((
            self.stylesheets.len(),
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                // Remember the source location even if the caller changes cwd before run().
                std::env::current_dir()?.join(path)
            },
        ));
        self.stylesheets.push(sheet);
        Ok(self)
    }

    /// Opt in to native file notifications for css_file sources. Defaults to false.
    /// Disabled applications create no watcher, worker, wake channel, or debounce wait.
    pub fn css_hot_reload(mut self, enabled: bool) -> Self {
        self.hot_reload = enabled;
        self
    }

    pub fn run(self) -> Result<()> {
        // Close task scopes on every return path, including setup errors and an
        // application configured without windows. Existing runtime clones may live on.
        struct Shutdown(crate::tasks::TaskRuntime);
        impl Drop for Shutdown {
            fn drop(&mut self) {
                self.0.shutdown();
            }
        }
        let _shutdown = Shutdown(self.tasks.clone());
        if self.windows.is_empty() {
            return Ok(());
        }
        let mut builder = EventLoop::<RuntimeEvent>::with_user_event();
        platform::configure_event_loop(&mut builder);
        let event_loop = builder.build()?;
        event_loop.set_control_flow(ControlFlow::Wait);
        let task_proxy = event_loop.create_proxy();
        self.tasks.set_waker(move || {
            let _ = task_proxy.send_event(RuntimeEvent::TasksReady);
        });
        let _reload = if self.hot_reload && !self.css_files.is_empty() {
            let proxy = event_loop.create_proxy();
            Some(crate::style::css::reload::ReloadManager::start(
                self.css_files,
                move |event| {
                    let _ = proxy.send_event(RuntimeEvent::Reload(event));
                },
            )?)
        } else {
            None
        };
        let text_system = self.text_system.unwrap_or_else(|| {
            Arc::new(TextSystem::new(Arc::new(ParleyTextSystem::new(
                platform::system_font(),
            ))))
        });
        let clipboard = Rc::new(RefCell::new(platform::clipboard::Clipboard::new(
            event_loop.owned_display_handle(),
        )));
        // Components reach this same lazy backend through `crate::clipboard`.
        platform::clipboard::install(&clipboard);
        let mut runtime = Runtime {
            tasks: self.tasks,
            pending: self.windows,
            windows: HashMap::new(),
            clipboard,
            gpu: GpuContext::default(),
            text_system,
            stylesheets: self.stylesheets,
            on_frame: self.on_frame,
            on_event: self.on_event,
            error: None,
        };
        event_loop.run_app(&mut runtime)?;
        match runtime.error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

struct Runtime {
    tasks: crate::tasks::TaskRuntime,
    pending: Vec<(WindowOptions, Element)>,
    windows: HashMap<WindowId, AppWindow>,
    // Keep X11/Wayland clipboard ownership alive after the source window closes.
    // Clipboard retains a display lease independently of the event loop's lifetime.
    clipboard: Rc<RefCell<platform::clipboard::Clipboard>>,
    gpu: GpuContext,
    text_system: Arc<TextSystem>,
    stylesheets: Vec<Stylesheet>,
    on_frame: Option<FrameCallback>,
    on_event: Option<EventCallback>,
    error: Option<Error>,
}

impl Runtime {
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: Error) {
        self.error = Some(error);
        event_loop.exit();
    }

    fn draw(&mut self, event_loop: &ActiveEventLoop, id: WindowId, synchronous: bool) {
        let window = self.windows.get_mut(&id).unwrap();
        let result = window.render(synchronous).and_then(|presented| {
            if presented && let Some(callback) = &mut self.on_frame {
                callback(window)?;
            }
            Ok(())
        });
        if let Err(error) = result {
            self.fail(event_loop, error);
            return;
        }
        if window.close_requested {
            self.windows.remove(&id);
            if self.windows.is_empty() {
                event_loop.exit();
            }
        }
    }
}

impl Runtime {
    fn remove_closed_windows(&mut self, event_loop: &ActiveEventLoop) {
        self.windows.retain(|_, window| !window.close_requested);
        if self.windows.is_empty() {
            event_loop.exit();
        }
    }
}

impl ApplicationHandler<RuntimeEvent> for Runtime {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        platform::resumed(event_loop);
        for (options, root) in std::mem::take(&mut self.pending) {
            match AppWindow::create(
                event_loop,
                options,
                root,
                self.gpu.clone(),
                self.text_system.clone(),
                self.clipboard.clone(),
                self.tasks.clone(),
            ) {
                Ok(mut window) => {
                    window.set_stylesheets(self.stylesheets.clone());
                    self.windows.insert(window.native_window().id(), window);
                }
                Err(error) => {
                    self.fail(event_loop, error);
                    return;
                }
            }
        }
        for window in self.windows.values_mut() {
            window.resume();
        }
        // Hidden AppKit/Win32 windows may not receive RedrawRequested. Bootstrap
        // their scene synchronously, then map them before acquiring the first buffer.
        if platform::HIDE_UNTIL_SCENE_READY {
            let ids: Vec<_> = self.windows.keys().copied().collect();
            for id in ids {
                self.draw(event_loop, id, true);
            }
        }
        for window in self.windows.values_mut() {
            window.request_if_needed(Instant::now());
        }
    }

    fn user_event(&mut self, _: &ActiveEventLoop, event: RuntimeEvent) {
        let RuntimeEvent::Reload(event) = event else {
            return;
        };
        let sheet = match event.source {
            Ok(source) => match Stylesheet::parse(&source) {
                Ok(sheet) => sheet,
                Err(error) => {
                    eprintln!(
                        "CSS reload rejected (keeping previous stylesheet): {}: {error}",
                        event.path.display()
                    );
                    return;
                }
            },
            Err(error) => {
                eprintln!(
                    "CSS reload read failed (keeping previous stylesheet): {}: {error}",
                    event.path.display()
                );
                return;
            }
        };
        if self.stylesheets[event.index].same_rules(&sheet) {
            return;
        }
        self.stylesheets[event.index] = sheet;
        for window in self.windows.values_mut() {
            window.set_stylesheets(self.stylesheets.clone());
        }
    }

    fn suspended(&mut self, _: &ActiveEventLoop) {
        for window in self.windows.values_mut() {
            window.suspend();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(window) = self.windows.get_mut(&id) else {
            return;
        };
        if let Some(callback) = &mut self.on_event
            && let Err(error) = callback(&event, window)
        {
            self.fail(event_loop, error);
            return;
        }
        if window.close_requested {
            self.windows.remove(&id);
            if self.windows.is_empty() {
                event_loop.exit();
            }
            return;
        }
        let state_changed = matches!(
            event,
            WindowEvent::Focused(_)
                | WindowEvent::Resized(_)
                | WindowEvent::ScaleFactorChanged { .. }
                | WindowEvent::Occluded(_)
        );
        match event {
            WindowEvent::CloseRequested | WindowEvent::Destroyed => {
                self.windows.remove(&id);
                if self.windows.is_empty() {
                    event_loop.exit();
                }
            }
            WindowEvent::Resized(_) => {
                window.update_viewport();
                if platform::SYNCHRONOUS_RESIZE {
                    self.draw(event_loop, id, true);
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                // Read final physical size at redraw, after Winit applies its DPI
                // size negotiation. A single pending request coalesces the events.
                window.invalidate_paint();
            }
            WindowEvent::Occluded(occluded) => window.occluded(occluded),
            WindowEvent::CursorMoved { position, .. } => window.pointer_moved(Some(position)),
            WindowEvent::CursorLeft { .. } => window.pointer_moved(None),
            WindowEvent::ModifiersChanged(modifiers) => window.modifiers = modifiers.state(),
            WindowEvent::Ime(event) => window.text_ime(event),
            WindowEvent::MouseWheel { delta, .. } => window.mouse_scroll(delta),
            WindowEvent::KeyboardInput { ref event, .. } => window.keyboard_input(event),
            WindowEvent::MouseInput { state, button, .. } => {
                window.mouse_button(button.into(), state == winit::event::ElementState::Pressed)
            }
            WindowEvent::Focused(focused) => {
                if !focused {
                    window.cancel_pointer();
                }
                window.text_focus(focused);
            }
            WindowEvent::RedrawRequested => self.draw(event_loop, id, false),
            _ => {}
        }
        // Window actions queued by pointer/keyboard callbacks are committed only
        // after dispatch. Closing a minimized window must not require a new frame.
        if let Some(window) = self.windows.get_mut(&id) {
            window.apply_window_actions();
            if state_changed {
                window.sync_window_state();
            }
        }
        self.remove_closed_windows(event_loop);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Futures make progress even while every window is occluded or suspended.
        // Poll one bounded batch after input; wake notifications schedule the next.
        self.tasks.tick();
        let now = Instant::now();
        for window in self.windows.values_mut() {
            window.request_if_needed(now);
        }
        self.remove_closed_windows(event_loop);
        // No polling, display link, or refresh timer for unchanged content.
        // Delayed transitions and transient failures install explicit deadlines.
        let next_retry = self
            .windows
            .values()
            .filter_map(|window| window.schedule.deadline())
            .min();
        event_loop.set_control_flow(
            next_retry
                .map(ControlFlow::WaitUntil)
                .unwrap_or(ControlFlow::Wait),
        );
    }

    fn exiting(&mut self, _: &ActiveEventLoop) {
        self.windows.clear();
        self.tasks.shutdown();
    }
}

#[cfg(test)]
mod css_reload_tests {
    #[test]
    fn hot_reload_is_disabled_by_default() {
        let app = super::Application::new();
        assert!(!app.hot_reload);
        assert!(app.css_files.is_empty());
    }
    #[test]
    fn hot_reload_has_one_runtime_switch() {
        let app = super::Application::new().css_hot_reload(true);
        assert!(app.hot_reload);
        let app = app.css_hot_reload(false);
        assert!(!app.hot_reload);
    }
}
