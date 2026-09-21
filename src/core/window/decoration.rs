//! Native actions run only after ordinary handlers have had a chance to prevent
//! defaults. The host updates one reactive snapshot for all titlebar components.
use super::*;
use crate::{WindowAction, WindowControlArea, WindowDecorations};

impl AppWindow {
    pub fn window_context(&self) -> crate::WindowContext {
        self.tree.window_context()
    }
    pub fn window_state(&self) -> crate::WindowState {
        self.tree.window_host.state()
    }
    pub(crate) fn sync_window_state(&mut self) {
        let mut state = self.tree.window_host.state();
        state.maximized = self.native.is_maximized();
        state.fullscreen = self.native.fullscreen().is_some();
        state.focused = self.native.has_focus();
        state.resizable = self.native.is_resizable();
        // Linux Winit reports all enabled buttons even when the compositor
        // cannot change them. Keep the application's explicit restriction.
        state.minimizable = self.options.minimizable
            && self
                .native
                .enabled_buttons()
                .contains(winit::window::WindowButtons::MINIMIZE);
        state.native_controls = self.decoration.native_controls(&self.native);
        self.tree.window_host.update(state);
    }
    pub(crate) fn apply_window_actions(&mut self) {
        let actions = self.tree.take_window_actions();
        if actions.is_empty() {
            return;
        }
        for action in actions {
            match action {
                WindowAction::Close => self.close(),
                WindowAction::Minimize if self.tree.window_host.state().minimizable => {
                    self.native.set_minimized(true)
                }
                WindowAction::ToggleMaximize
                    if self.native.is_resizable() && self.native.fullscreen().is_none() =>
                {
                    self.native.set_maximized(!self.native.is_maximized());
                }
                _ => {}
            }
        }
        self.sync_window_state();
    }
    pub(super) fn decoration_pointer(
        &mut self,
        button: super::super::event::MouseButton,
        pressed: bool,
        prevented: bool,
    ) -> bool {
        if self.options.decorations != WindowDecorations::Custom
            || prevented
            || self.tree.pointer_capture().is_some()
        {
            return false;
        }
        let Some(point) = self.tree.pointer_position() else {
            return false;
        };
        let area = self.tree.window_control_at(point);
        if self.native.fullscreen().is_some() {
            return false;
        }
        #[cfg(target_os = "linux")]
        if button == super::super::event::MouseButton::Left
            && pressed
            && self.native.is_resizable()
            && !self.native.is_maximized()
        {
            if let Some(edge) = platform::decoration::resize_edge(
                point,
                self.logical_size(),
                self.options.titlebar.resize_border,
            ) {
                self.cancel_pointer();
                if let Err(error) = self.native.drag_resize_window(edge) {
                    log::debug!("window resize unavailable: {error}");
                }
                return true;
            }
        }
        if area == WindowControlArea::Drag {
            if button == super::super::event::MouseButton::Right && pressed {
                self.native
                    .show_window_menu(winit::dpi::LogicalPosition::new(point.x, point.y));
                return true;
            }
            if button == super::super::event::MouseButton::Left && pressed {
                let clicks =
                    self.titlebar_clicks
                        .clicks(Instant::now(), point, self.options.selection);
                self.cancel_pointer();
                if clicks == 2 {
                    platform::decoration::double_click(&self.native);
                    self.titlebar_clicks = Default::default();
                } else if let Err(error) = self.native.drag_window() {
                    log::debug!("window drag unavailable: {error}");
                }
                return true;
            }
        }
        area.action().is_some()
    }
}
