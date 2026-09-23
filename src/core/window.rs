//! A retained widget tree, scene, and renderer for one native desktop window.
use std::{
    cell::RefCell,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use voidui_gpui_wgpu::{
    DevicePixels, GpuContext, Painter, Result, Scene, TextLayoutCache, TextSystem, WgpuRenderer,
    WgpuSurfaceConfig, fill, point, px, size,
};
use winit::{
    dpi::{LogicalSize, PhysicalSize},
    event_loop::ActiveEventLoop,
    window::Window,
};

use crate::{
    core::{
        element::Element,
        frame::{FrameSchedule, RetryPolicy},
        geometry::Size,
        layout::{self, AvailableSpace},
        widget_tree::WidgetTree,
    },
    platform,
    style::color::{Color, Rgba8},
};

/// Native window configuration. Sizes are logical pixels and follow monitor DPI.
#[derive(Debug, Clone)]
pub struct WindowOptions {
    pub render_cache: crate::render::RenderCacheOptions,
    pub decorations: super::decoration::WindowDecorations,
    pub titlebar: super::decoration::TitlebarOptions,
    pub resizable: bool,
    pub minimizable: bool,
    pub title: String,
    pub app_id: String,
    pub size: Size<f64>,
    pub min_size: Option<Size<f64>>,
    pub transparent: bool,
    pub background: Color,
    pub retry_policy: RetryPolicy,
    pub selection: super::selection::SelectionOptions,
    pub scrolling: super::scroll::ScrollOptions,
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            render_cache: Default::default(),
            decorations: Default::default(),
            titlebar: Default::default(),
            resizable: true,
            minimizable: true,
            title: "voidui".into(),
            app_id: "org.voidui.app".into(),
            size: Size::new(960.0, 640.0),
            min_size: None,
            transparent: false,
            background: Rgba8::from_rgb8(255, 255, 255).into(),
            retry_policy: RetryPolicy::default(),
            selection: Default::default(),
            scrolling: Default::default(),
        }
    }
}

/// Counters distinguish content work from simply re-presenting an unchanged scene.
#[derive(Debug, Default, Clone, Copy)]
pub struct FrameStats {
    pub layout_passes: u64,
    pub scene_builds: u64,
    pub presented_frames: u64,
    pub failed_frames: u64,
    pub gpu_recoveries: u64,
    pub last_layout_time: Duration,
    pub last_scene_time: Duration,
    pub last_present_time: Duration,
    pub renderer: crate::render::RenderStats,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Viewport {
    pub(crate) physical: PhysicalSize<u32>,
    pub(crate) scale: f64,
}

impl Viewport {
    fn read(window: &Window) -> Self {
        Self {
            physical: window.inner_size(),
            scale: window.scale_factor(),
        }
    }
    pub(crate) fn logical(self) -> Size<f32> {
        Size::new(
            (f64::from(self.physical.width) / self.scale) as f32,
            (f64::from(self.physical.height) / self.scale) as f32,
        )
    }
    pub(crate) fn drawable(self) -> bool {
        self.physical.width > 0 && self.physical.height > 0
    }
    fn device_size(self) -> voidui_gpui_wgpu::Size<DevicePixels> {
        size(
            DevicePixels(self.physical.width.min(i32::MAX as u32) as i32),
            DevicePixels(self.physical.height.min(i32::MAX as u32) as i32),
        )
    }
}

mod decoration;
mod input;
pub struct AppWindow {
    decoration: platform::decoration::Decoration,
    titlebar_clicks: super::selection::SelectionInput,
    input_capture: Option<super::widget::WidgetId>,
    ime_target: Option<super::widget::WidgetId>,
    ime_bounds: Option<super::geometry::Rect<f32>>,
    native_focused: bool,
    clipboard: Rc<RefCell<crate::platform::clipboard::Clipboard>>,
    selection_input: super::selection::SelectionInput,
    cursor: crate::style::selection::Cursor,
    // Drop surface resources before releasing our final native-window handle.
    renderer: Option<WgpuRenderer>,
    native: Arc<Window>,
    gpu: GpuContext,
    text_system: Arc<TextSystem>,
    text_layout: TextLayoutCache,
    tree: WidgetTree,
    pub(crate) modifiers: winit::keyboard::ModifiersState,
    scene: Scene,
    options: WindowOptions,
    viewport: Viewport,
    layout_dirty: bool,
    scene_dirty: bool,
    occluded: bool,
    suspended: bool,
    shown: bool,
    pub(crate) close_requested: bool,
    pub(crate) schedule: FrameSchedule,
    stats: FrameStats,
}

impl AppWindow {
    pub(crate) fn create(
        event_loop: &ActiveEventLoop,
        options: WindowOptions,
        root: Element,
        gpu: GpuContext,
        text_system: Arc<TextSystem>,
        clipboard: Rc<RefCell<crate::platform::clipboard::Clipboard>>,
        tasks: crate::tasks::TaskRuntime,
    ) -> Result<Self> {
        options.titlebar.validate()?;
        if !options.size.width.is_finite()
            || options.size.width <= 0.0
            || !options.size.height.is_finite()
            || options.size.height <= 0.0
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "window size must be finite and positive",
            )
            .into());
        }
        let mut attributes = Window::default_attributes()
            .with_title(&options.title)
            .with_inner_size(LogicalSize::new(options.size.width, options.size.height))
            .with_transparent(options.transparent)
            .with_visible(!platform::HIDE_UNTIL_SCENE_READY);
        if let Some(minimum) = options.min_size {
            attributes =
                attributes.with_min_inner_size(LogicalSize::new(minimum.width, minimum.height));
        }
        let native = Arc::new(event_loop.create_window(platform::window_attributes(
            platform::decoration::attributes(attributes, &options),
            &options.app_id,
        ))?);
        let mut decoration = platform::decoration::Decoration::new(&native, &options)?;
        let viewport = Viewport::read(&native);
        let schedule = FrameSchedule::new(options.retry_policy);
        let mut tree = WidgetTree::with_task_runtime(tasks);
        tree.set_selection_colors(options.selection.colors);
        tree.set_scroll_options(options.scrolling);
        let weak_window = Arc::downgrade(&native);
        tree.set_update_waker(move || {
            if let Some(window) = weak_window.upgrade() {
                window.request_redraw();
            }
        });
        tree.set_window_state(super::decoration::WindowState {
            decorations: options.decorations,
            titlebar: options.titlebar.clone(),
            button_layout: options.titlebar.button_layout.clone().unwrap_or_default(),
            native_controls: decoration.native_controls(&native),
            resizable: options.resizable,
            minimizable: options.minimizable,
            ..Default::default()
        });
        #[cfg(target_os = "linux")]
        if options.decorations == super::decoration::WindowDecorations::Custom
            && options.titlebar.button_layout.is_none()
        {
            platform::watch_button_layout(&tree);
        }
        tree.build_root(root);
        Ok(Self {
            decoration,
            titlebar_clicks: Default::default(),
            input_capture: None,
            ime_target: None,
            ime_bounds: None,
            native_focused: true,
            clipboard,
            selection_input: Default::default(),
            cursor: Default::default(),
            renderer: None,
            native,
            gpu,
            text_layout: TextLayoutCache::new(text_system.clone()),
            text_system,
            tree,
            modifiers: Default::default(),
            scene: Scene::default(),
            options,
            viewport,
            layout_dirty: true,
            scene_dirty: true,
            occluded: false,
            suspended: false,
            shown: !platform::HIDE_UNTIL_SCENE_READY,
            close_requested: false,
            schedule,
            stats: FrameStats::default(),
        })
    }

    /// Share the native handle for window operations or an external wakeup.
    pub fn native_window(&self) -> &Arc<Window> {
        &self.native
    }
    /// Window-owned work survives component unmounts and stops when this window closes.
    pub fn task_scope(&self) -> crate::tasks::TaskScope {
        self.tree.task_scope()
    }

    pub fn tree(&self) -> &WidgetTree {
        &self.tree
    }

    /// Edit content or styles, then allow the runtime to coalesce one new frame.
    pub fn tree_mut(&mut self) -> &mut WidgetTree {
        // Tree construction invalidates geometry itself; style edits are classified
        // by update_styles. A color/class change should not force text measurement.
        self.decoration.invalidate();
        self.request_redraw();
        &mut self.tree
    }

    /// Open an ordinary element as a modal top-layer entry. Its appearance comes
    /// entirely from CSS; this method supplies only lifecycle and modality.
    pub fn show_modal(&mut self, id: crate::core::widget::WidgetId) -> Result<bool> {
        let changed = self.tree.show_modal(id)?;
        if changed {
            self.request_redraw();
        }
        Ok(changed)
    }
    pub fn show_popover(&mut self, id: crate::core::widget::WidgetId) -> Result<bool> {
        let changed = self.tree.show_popover(id)?;
        if changed {
            self.request_redraw();
        }
        Ok(changed)
    }
    pub fn close_top_layer(&mut self, id: crate::core::widget::WidgetId) -> bool {
        let changed = self.tree.close_top_layer(id);
        if changed {
            self.request_redraw();
        }
        changed
    }
    pub(crate) fn focus_next(&mut self, reverse: bool) {
        if self.tree.focus_next(reverse) {
            self.request_redraw();
        }
    }

    /// Replace author stylesheets without touching inline declarations.
    pub fn set_stylesheets(&mut self, sheets: Vec<crate::style::css::Stylesheet>) {
        if self.tree.set_stylesheets(sheets) {
            self.request_redraw();
        }
    }

    /// Feed physical window coordinates from a native adapter; None means cursor exit.
    pub fn pointer_moved(&mut self, position: Option<winit::dpi::PhysicalPosition<f64>>) {
        let point = position.map(|p| {
            crate::core::geometry::Point::new(
                (p.x / self.viewport.scale) as f32,
                (p.y / self.viewport.scale) as f32,
            )
        });
        let event = self.tree.dispatch_mouse_move(point, self.modifiers);
        if event.changed {
            self.request_redraw();
        }
        if !event.response.prevent_default
            && self.tree.pointer_capture().is_none()
            && let Some(point) = point
            && !self.text_pointer(super::input::PointerPhase::Move, point, 0)
            && self.tree.selection_pointer_move(point)
        {
            self.invalidate_paint();
        }
        self.update_cursor();
    }
    pub(crate) fn cancel_pointer(&mut self) {
        if self.tree.cancel_pointer_capture() {
            self.request_redraw();
        }
        // CursorLeft carries no coordinates. Cancellation still has to release
        // the text client's capture instead of manufacturing a release/click.
        self.text_pointer(
            super::input::PointerPhase::Cancel,
            self.tree.pointer_position().unwrap_or_default(),
            0,
        );
        self.update_cursor();
    }
    pub(crate) fn activation_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        use winit::{
            event::ElementState,
            keyboard::{Key, NamedKey},
        };
        if event.state != ElementState::Pressed
            || event.repeat
            || self.modifiers.control_key()
            || self.modifiers.alt_key()
            || self.modifiers.super_key()
            || !matches!(
                event.logical_key,
                Key::Named(NamedKey::Enter | NamedKey::Space)
            )
        {
            return false;
        }
        self.tree.focused().is_some_and(|id| {
            self.tree
                .click_with_source(id, super::event::ClickSource::Keyboard)
                .is_some()
        })
    }
    /// Dispatch a native button through general handlers, capture, and text defaults.
    pub fn mouse_button(&mut self, button: super::event::MouseButton, pressed: bool) {
        let event = self
            .tree
            .dispatch_mouse_button(button, pressed, self.modifiers);
        if event.changed {
            self.request_redraw();
        }
        if self.decoration_pointer(button, pressed, event.response.prevent_default) {
            self.apply_window_actions();
            self.update_cursor();
            return;
        }
        if button == super::event::MouseButton::Left {
            if pressed {
                if !event.response.prevent_default
                    && self.tree.pointer_capture().is_none()
                    && let Some(point) = self.tree.pointer_position()
                {
                    let clicks =
                        self.selection_input
                            .clicks(Instant::now(), point, self.options.selection);
                    if !self.text_pointer(super::input::PointerPhase::Down, point, clicks)
                        && self.tree.selection_pointer_down(
                            point,
                            self.modifiers.shift_key(),
                            clicks,
                        )
                    {
                        self.invalidate_paint();
                    }
                }
            } else {
                self.text_pointer(
                    super::input::PointerPhase::Up,
                    self.tree.pointer_position().unwrap_or_default(),
                    0,
                );
                self.tree.end_selection_drag();
            }
            self.sync_text_input();
        }
        self.apply_window_actions();
        self.update_cursor();
    }
    fn update_cursor(&mut self) {
        // A release can invalidate layout through its callback. Keep the current
        // cursor until hit geometry is ready instead of flashing Auto, then resolve
        // it again after layout/presentation. Capture itself never needs a hit test.
        if self.tree.requires_layout()
            && self.tree.pointer_capture().is_none()
            && self.input_capture.is_none()
            && self.tree.pointer_position().is_some()
        {
            return;
        }
        let cursor = self
            .tree
            .pointer_cursor_at(self.tree.pointer_position(), self.input_capture);
        #[cfg(target_os = "linux")]
        let cursor = if self.options.decorations == crate::WindowDecorations::Custom
            && self.native.is_resizable()
            && !self.native.is_maximized()
            && self.native.fullscreen().is_none()
            && self.tree.pointer_capture().is_none()
            && self.input_capture.is_none()
        {
            use winit::window::{CursorIcon, ResizeDirection as Edge};
            self.tree
                .pointer_position()
                .and_then(|p| {
                    platform::decoration::resize_edge(
                        p,
                        self.logical_size(),
                        self.options.titlebar.resize_border,
                    )
                })
                .map(|edge| {
                    crate::style::selection::Cursor::Icon(match edge {
                        Edge::North => CursorIcon::NResize,
                        Edge::South => CursorIcon::SResize,
                        Edge::East => CursorIcon::EResize,
                        Edge::West => CursorIcon::WResize,
                        Edge::NorthEast => CursorIcon::NeResize,
                        Edge::NorthWest => CursorIcon::NwResize,
                        Edge::SouthEast => CursorIcon::SeResize,
                        Edge::SouthWest => CursorIcon::SwResize,
                    })
                })
                .unwrap_or(cursor)
        } else {
            cursor
        };
        if cursor != self.cursor {
            use crate::style::selection::Cursor;
            self.native.set_cursor_visible(cursor != Cursor::None);
            self.native.set_cursor(match cursor {
                Cursor::Icon(icon) => icon,
                _ => winit::window::CursorIcon::Default,
            });
            self.cursor = cursor;
        }
    }
    /// The cursor last applied to the native window, including drag capture.
    pub fn current_cursor(&self) -> crate::style::selection::Cursor {
        self.cursor
    }

    /// Dispatch native wheel units through handlers before editable scrolling.
    pub fn mouse_scroll(&mut self, delta: winit::event::MouseScrollDelta) {
        use super::event::{MouseWheel, WheelUnit};
        let wheel = match delta {
            winit::event::MouseScrollDelta::LineDelta(x, y) => MouseWheel {
                x,
                y,
                unit: WheelUnit::Lines,
            },
            winit::event::MouseScrollDelta::PixelDelta(point) => MouseWheel {
                x: (point.x / self.viewport.scale) as f32,
                y: (point.y / self.viewport.scale) as f32,
                unit: WheelUnit::Pixels,
            },
        };
        let response = self.tree.dispatch_mouse_scroll(wheel, self.modifiers);
        if !response.prevent_default && self.tree.pointer_capture().is_none() {
            // Legacy text clients receive only axes not consumed by a hosted
            // scrollport. Retained controls already use the tree's default path.
            let remaining = response.remaining_scroll(wheel);
            let delta = match remaining.unit {
                WheelUnit::Lines => {
                    winit::event::MouseScrollDelta::LineDelta(remaining.x, remaining.y)
                }
                WheelUnit::Pixels => {
                    winit::event::MouseScrollDelta::PixelDelta(winit::dpi::PhysicalPosition::new(
                        remaining.x as f64 * self.viewport.scale,
                        remaining.y as f64 * self.viewport.scale,
                    ))
                }
            };
            self.text_scroll(delta);
        }
    }
    pub(crate) fn keyboard_input(&mut self, event: &winit::event::KeyEvent) {
        use super::event::{Key, KeyEventType, NamedKey};
        let state = match event.state {
            winit::event::ElementState::Pressed => KeyEventType::Pressed,
            winit::event::ElementState::Released => KeyEventType::Released,
        };
        let response = self.tree.dispatch_key(
            event.logical_key.clone(),
            state,
            self.modifiers,
            event.repeat,
        );
        if response.prevent_default
            || self.text_key(event)
            || self.selection_key(event)
            || self.activation_key(event)
            || (state == KeyEventType::Pressed
                && self.tree.scroll_key(&event.logical_key, self.modifiers))
        {
            return;
        }
        if state == KeyEventType::Pressed
            && event.logical_key == Key::Named(NamedKey::Tab)
            && !self.modifiers.control_key()
            && !self.modifiers.alt_key()
            && !self.modifiers.super_key()
        {
            self.focus_next(self.modifiers.shift_key());
        }
    }
    /// Copy selected rendered text without modifying it. A collapsed/empty range
    /// leaves the system clipboard untouched and does not initialize a backend.
    pub fn copy_selection(&mut self) -> Result<bool> {
        let text = self.tree.selected_text();
        if text.is_empty() {
            return Ok(false);
        }
        self.clipboard.borrow_mut().write(text)?;
        Ok(true)
    }
    pub(crate) fn selection_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        if event.state != winit::event::ElementState::Pressed {
            return false;
        }
        let command = if cfg!(target_os = "macos") {
            self.modifiers.super_key()
        } else {
            self.modifiers.control_key()
        };
        if command
            && !self.modifiers.alt_key()
            && let winit::keyboard::Key::Character(key) = &event.logical_key
        {
            if key.eq_ignore_ascii_case("a") {
                if self.tree.select_all() {
                    self.invalidate_paint();
                }
                return true;
            }
            if key.eq_ignore_ascii_case("c") {
                if let Err(error) = self.copy_selection() {
                    log::warn!("{error}");
                }
                return true;
            }
        }
        if self.modifiers.shift_key() {
            use super::selection::SelectionMove as M;
            use winit::keyboard::{Key, NamedKey};
            let word = if cfg!(target_os = "macos") {
                self.modifiers.alt_key()
            } else {
                self.modifiers.control_key()
            };
            let movement = match &event.logical_key {
                Key::Named(NamedKey::ArrowLeft) => Some(if command && cfg!(target_os = "macos") {
                    M::LineStart
                } else if word {
                    M::WordLeft
                } else {
                    M::Left
                }),
                Key::Named(NamedKey::ArrowRight) => Some(if command && cfg!(target_os = "macos") {
                    M::LineEnd
                } else if word {
                    M::WordRight
                } else {
                    M::Right
                }),
                Key::Named(NamedKey::ArrowUp) => Some(if command && cfg!(target_os = "macos") {
                    M::DocumentStart
                } else {
                    M::Up
                }),
                Key::Named(NamedKey::ArrowDown) => Some(if command && cfg!(target_os = "macos") {
                    M::DocumentEnd
                } else {
                    M::Down
                }),
                Key::Named(NamedKey::Home) => Some(if command {
                    M::DocumentStart
                } else {
                    M::LineStart
                }),
                Key::Named(NamedKey::End) => {
                    Some(if command { M::DocumentEnd } else { M::LineEnd })
                }
                _ => None,
            };
            if let Some(movement) = movement {
                if self.tree.extend_selection(movement) {
                    self.invalidate_paint();
                }
                return true;
            }
        }
        false
    }

    pub fn stats(&self) -> FrameStats {
        self.stats
    }
    pub fn logical_size(&self) -> Size<f32> {
        self.viewport.logical()
    }

    /// Schedule layout and scene construction after content/typography changes.
    pub fn invalidate_layout(&mut self) {
        self.layout_dirty = true;
        self.invalidate_paint();
    }

    /// Rebuild the scene without reflowing unchanged logical geometry.
    pub fn invalidate_paint(&mut self) {
        self.scene_dirty = true;
        self.schedule.invalidate();
    }

    /// Re-present the retained scene without layout or text shaping work.
    pub fn request_redraw(&mut self) {
        self.schedule.invalidate();
    }
    pub fn close(&mut self) {
        self.close_requested = true;
        self.native.request_redraw();
    }

    /// Explicit, synchronous GPU readback for diagnostics. Never used by normal frames.
    /// Returns physical dimensions and tightly packed RGBA8 pixels.
    pub fn snapshot(&mut self) -> Result<(Size<u32>, Vec<u8>)> {
        if self.scene_dirty || self.layout_dirty || self.tree.has_pending_updates() {
            return Err(
                std::io::Error::other("present a current frame before taking a snapshot").into(),
            );
        }
        let renderer = self
            .renderer
            .as_mut()
            .ok_or_else(|| std::io::Error::other("renderer is not initialized"))?;
        let pixels = renderer.render_to_rgba(&self.scene)?;
        let size = renderer.viewport_size();
        Ok((Size::new(size.width.0 as u32, size.height.0 as u32), pixels))
    }

    pub(crate) fn update_viewport(&mut self) {
        let viewport = Viewport::read(&self.native);
        if viewport != self.viewport {
            self.decoration.invalidate();
            if viewport.logical() != self.viewport.logical() {
                self.layout_dirty = true;
            }
            self.viewport = viewport;
            self.invalidate_paint();
        }
        self.update_availability();
    }

    fn update_availability(&mut self) {
        let minimized = self.native.is_minimized() == Some(true);
        self.schedule.set_available(
            !minimized
                && !self.suspended
                && self.viewport.drawable()
                && (!self.occluded || !self.shown),
        );
    }

    pub(crate) fn occluded(&mut self, occluded: bool) {
        self.occluded = occluded;
        self.update_availability();
    }

    pub(crate) fn suspend(&mut self) {
        self.cancel_pointer();
        self.suspended = true;
        // A suspended native surface may be invalid. Retain the shared device,
        // widget tree, and text cache; recreate surface resources on resume.
        self.renderer.take();
        self.scene_dirty = true;
        self.update_availability();
    }

    pub(crate) fn resume(&mut self) {
        self.suspended = false;
        self.update_viewport();
        self.schedule.invalidate();
    }

    pub(crate) fn request_if_needed(&mut self, now: Instant) {
        self.apply_window_actions();
        if self.native_focused {
            self.tree.tick_input(now);
        }
        if self.tree.widget_updates.caret_visibility.get().is_some() {
            self.schedule.invalidate();
        }
        if self.tree.has_pending_updates() {
            self.schedule.invalidate();
            // Commit component lifecycles without requiring a drawable surface.
            // Style/layout/paint still wait for the next actual presentation.
            self.tree.flush_updates();
        }
        if self.tree.refresh_pointer() {
            self.schedule.invalidate();
        }
        if self.tree.requires_layout()
            || self.tree.has_pending_updates()
            || self.tree.styles_pending()
        {
            self.decoration.invalidate();
        }
        self.update_cursor();
        self.sync_text_input();
        self.schedule.animate_at(
            self.tree
                .next_animation_frame(now)
                .into_iter()
                .chain(self.next_text_frame(now))
                .min(),
        );
        if self.schedule.request(now) {
            self.native.request_redraw();
        }
    }

    /// Returns true only when a buffer was submitted for presentation.
    pub(crate) fn render(&mut self, synchronous: bool) -> Result<bool> {
        self.sync_window_state();
        self.update_viewport();
        if !self.schedule.begin(Instant::now()) {
            return Ok(false);
        }
        // Consume CPU updates before trying GPU recovery. Leaving a state batch
        // pending after a GPU failure would repeatedly restart redraw backoff.
        if self.native_focused {
            self.tree.tick_input(Instant::now());
        }
        let logical = self.viewport.logical();
        self.tree.set_viewport_size(layout::Size {
            width: logical.width,
            height: logical.height,
        });
        if let Some(visible) = self.tree.widget_updates.caret_visibility.take() {
            self.scene.caret_visible = visible;
        }
        let Some(changes) = self.tree.prepare_frame(Instant::now()) else {
            // Reconciliation may publish resource/loading or destructor updates
            // at commit. Resume on the next turn, without treating CPU work as
            // a failed GPU presentation or laying out an uncommitted tree.
            self.schedule.invalidate();
            return Ok(false);
        };
        if changes.paint {
            self.tree.refresh_scroll_content(&self.text_layout);
        }
        self.update_cursor();
        self.layout_dirty |= changes.layout || self.tree.requires_layout();
        let selection_dirty = self.tree.take_selection_paint_dirty();
        self.scene_dirty |= changes.paint || selection_dirty;
        if self.renderer.is_none() {
            if self
                .gpu
                .borrow()
                .as_ref()
                .is_some_and(|gpu| gpu.device_lost())
            {
                self.gpu.borrow_mut().take();
            }
            self.renderer = Some(WgpuRenderer::new(
                self.gpu.clone(),
                &self.native,
                WgpuSurfaceConfig {
                    size: self.viewport.device_size(),
                    transparent: self.options.transparent,
                    preferred_present_mode: None,
                },
                None,
            )?);
            self.renderer
                .as_mut()
                .unwrap()
                .set_cache_options(self.options.render_cache);
            self.scene_dirty = true;
        }
        let renderer = self.renderer.as_mut().unwrap();
        if renderer.device_lost() {
            if let Err(error) = renderer.recover(&self.native) {
                self.stats.failed_frames += 1;
                self.schedule.failed(Instant::now());
                log::warn!("GPU recovery deferred: {error:#}");
                return Ok(false);
            }
            self.stats.gpu_recoveries += 1;
            self.scene_dirty = true;
        }
        // Atlas reset invalidates scene tile IDs even when the widget tree is clean.
        self.scene_dirty |= renderer.needs_redraw();
        renderer.update_drawable_size(self.viewport.device_size());
        let logical = self.viewport.logical();
        if self.layout_dirty {
            let started = Instant::now();
            self.tree.layout_computed(
                layout::Size {
                    width: AvailableSpace::Definite(logical.width),
                    height: AvailableSpace::Definite(logical.height),
                },
                &self.text_layout,
            );
            self.stats.last_layout_time = started.elapsed();
            self.stats.layout_passes += 1;
            self.layout_dirty = false;
            self.scene_dirty = true;
        }
        self.decoration
            .publish(&self.tree, &self.tree.window_host.state(), self.scene_dirty);
        if self.scene_dirty {
            let started = Instant::now();
            self.scene.clear(); // Retain CPU allocations across changed frames.
            let atlas = renderer.sprite_atlas().clone();
            let viewport = size(px(logical.width), px(logical.height));
            let mut painter = Painter::new(
                &mut self.scene,
                atlas.as_ref(),
                self.text_system.clone(),
                viewport,
                self.viewport.scale as f32,
            )?;
            let current_color = self
                .tree
                .root()
                .map(|id| self.tree.text_style(id).color)
                .unwrap_or_else(|| crate::style::text::TextStyle::default().color);
            let background: voidui_gpui_wgpu::Hsla =
                self.options.background.resolve(current_color).into();
            if background.a > 0.0 {
                painter.paint_quad(fill(
                    voidui_gpui_wgpu::Bounds::new(point(px(0.), px(0.)), viewport),
                    background,
                ));
            }
            self.tree.draw(&mut painter)?;
            drop(painter);
            self.scene.finish();
            self.text_layout.finish_frame();
            self.stats.last_scene_time = started.elapsed();
            self.stats.scene_builds += 1;
            self.scene_dirty = false;
        }
        // Some surfaces return Occluded until the native window is mapped. Do
        // not wait for a successful present to show it: that creates a startup
        // deadlock. Delay showing only until layout, glyphs, and the scene exist.
        if !self.shown {
            self.native.set_visible(true);
            self.shown = true;
            self.occluded = false;
        }
        let started = Instant::now();
        platform::prepare_present(renderer, synchronous);
        let presented =
            renderer.draw_with_present_callback(&self.scene, || self.native.pre_present_notify());
        platform::prepare_present(renderer, false);
        self.stats.last_present_time = started.elapsed();
        self.stats.renderer = renderer.stats();
        self.sync_text_input();
        if presented {
            self.stats.presented_frames += 1;
            self.schedule.presented();
            self.schedule.animate_at(
                self.tree
                    .next_animation_frame(Instant::now())
                    .into_iter()
                    .chain(self.next_text_frame(Instant::now()))
                    .min(),
            );
        } else {
            self.stats.failed_frames += 1;
            self.schedule.failed(Instant::now());
            if self.schedule.retry_exhausted() {
                log::warn!(
                    "Presentation retries exhausted for {:?}; parked until a new window event",
                    self.native.id()
                );
            }
        }
        // Layout can move the pointer's hit target even without a CursorMoved
        // event. Apply its final cursor before exposing this frame to on_frame.
        self.update_cursor();
        Ok(presented)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dpi_is_applied_once_and_zero_sizes_are_not_drawable() {
        let normal = Viewport {
            physical: PhysicalSize::new(800, 600),
            scale: 1.0,
        };
        let retina = Viewport {
            physical: PhysicalSize::new(1600, 1200),
            scale: 2.0,
        };
        assert_eq!(normal.logical(), retina.logical());
        assert_eq!(retina.device_size().width.0, 1600);
        assert!(
            !Viewport {
                physical: PhysicalSize::new(0, 600),
                ..normal
            }
            .drawable()
        );
    }
    #[test]
    fn fractional_dpi_does_not_round_logical_layout() {
        let viewport = Viewport {
            physical: PhysicalSize::new(1001, 751),
            scale: 1.25,
        };
        assert_eq!(viewport.logical(), Size::new(800.8, 600.8));
    }
}
