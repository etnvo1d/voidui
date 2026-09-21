//! Winit and clipboard adapters. Text input is offered to the focused client
//! before document-selection shortcuts and default button/focus actions.
use super::*;
use crate::core::{
    event::EventResult,
    geometry::{Point, Rect},
    input::{InputEvent, KeyInput, PointerPhase},
};
use winit::{
    event::{ElementState, Ime, MouseScrollDelta},
    keyboard::{Key, NamedKey},
};

impl AppWindow {
    /// Send text input through the same client protocol used by native events.
    pub fn dispatch_input(
        &mut self,
        id: super::super::widget::WidgetId,
        event: &InputEvent,
    ) -> EventResult {
        self.tree.dispatch_input(id, event, &self.text_layout)
    }

    fn input_target(&self) -> Option<super::super::widget::WidgetId> {
        self.tree.focused().filter(|id| {
            self.native_focused
                && self.tree.input_enabled(*id)
                && self.tree.text_input(*id).is_some()
        })
    }
    pub(crate) fn text_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        let Some(id) = self.input_target() else {
            return false;
        };
        if event.state != ElementState::Pressed {
            return true;
        }
        let input = InputEvent::Key(KeyInput {
            key: event.logical_key.clone(),
            modifiers: self.modifiers,
            repeat: event.repeat,
        });
        if self.tree.dispatch_input(id, &input, &self.text_layout) == EventResult::Handled {
            self.sync_text_input();
            return true;
        }
        if self
            .tree
            .text_input(id)
            .is_some_and(|input| input.is_composing())
        {
            return true;
        }
        let command = if cfg!(target_os = "macos") {
            self.modifiers.super_key()
        } else {
            self.modifiers.control_key()
        };
        if command && !self.modifiers.alt_key() {
            if let Key::Character(key) = &event.logical_key {
                if key.eq_ignore_ascii_case("c") || key.eq_ignore_ascii_case("x") {
                    let text = self.tree.text_input(id).unwrap().selected_text();
                    if !text.is_empty() {
                        let copied = self.clipboard.borrow_mut().write(text);
                        match copied {
                            Ok(()) if key.eq_ignore_ascii_case("x") => {
                                self.tree
                                    .dispatch_input(id, &InputEvent::Cut, &self.text_layout);
                            }
                            Err(error) => log::warn!("{error}"),
                            _ => {}
                        }
                    }
                    self.sync_text_input();
                    return true;
                }
                if key.eq_ignore_ascii_case("v") {
                    let result = self.clipboard.borrow_mut().read();
                    match result {
                        Ok(text) => {
                            self.tree.dispatch_input(
                                id,
                                &InputEvent::Paste(text),
                                &self.text_layout,
                            );
                        }
                        Err(error) => log::warn!("{error}"),
                    }
                    self.sync_text_input();
                    return true;
                }
            }
        }
        if event.logical_key == Key::Named(NamedKey::Tab) {
            return false;
        }
        // Option/dead-key and AltGr text comes from Winit's committed text field.
        let shortcut = self.modifiers.super_key()
            || (self.modifiers.control_key() && !self.modifiers.alt_key());
        if !shortcut {
            if let Some(text) = &event.text {
                let text: String = text.chars().filter(|c| !c.is_control()).collect();
                if !text.is_empty() {
                    self.tree
                        .dispatch_input(id, &InputEvent::Text(text), &self.text_layout);
                    self.sync_text_input();
                    return true;
                }
            }
        }
        false
    }
    pub(crate) fn text_ime(&mut self, event: Ime) {
        let Some(id) = self.input_target() else {
            return;
        };
        if Some(id) != self.ime_target {
            return;
        }
        let event = match event {
            Ime::Enabled => return,
            Ime::Disabled => InputEvent::CancelComposition,
            Ime::Preedit(text, cursor) if !text.is_empty() => InputEvent::Preedit(text, cursor),
            Ime::Preedit(_, _) => InputEvent::CancelComposition,
            Ime::Commit(text) => InputEvent::Commit(text),
        };
        self.tree.dispatch_input(id, &event, &self.text_layout);
        self.sync_text_input();
    }
    pub(crate) fn text_pointer(
        &mut self,
        phase: PointerPhase,
        position: Point<f32>,
        clicks: u8,
    ) -> bool {
        let id = if phase == PointerPhase::Down {
            self.tree.input_at(position)
        } else {
            self.input_capture
        };
        let Some(id) = id else {
            return false;
        };
        if phase == PointerPhase::Down {
            self.tree.set_focused(Some(id));
            self.tree.clear_selection();
            let local = self.tree.window_to_layout(id, position).unwrap_or(position);
            let bounds = self.tree.bounds(id);
            if local.x < bounds.origin.x
                || local.x > bounds.origin.x + bounds.size.width
                || local.y < bounds.origin.y
                || local.y > bounds.origin.y + bounds.size.height
            {
                self.sync_text_input();
                return true;
            }
            self.input_capture = Some(id);
        }
        let handled = self.tree.dispatch_input(
            id,
            &InputEvent::Pointer {
                phase,
                position,
                modifiers: self.modifiers,
                clicks,
            },
            &self.text_layout,
        ) == EventResult::Handled;
        if matches!(phase, PointerPhase::Up | PointerPhase::Cancel) {
            self.input_capture = None;
        }
        self.sync_text_input();
        handled
    }
    pub(crate) fn text_scroll(&mut self, delta: MouseScrollDelta) -> bool {
        if self.tree.input_handlers == 0 {
            return false;
        }
        let Some(id) = self
            .tree
            .pointer_position()
            .and_then(|p| self.tree.input_at(p))
        else {
            return false;
        };
        // Controls exposing retained scroll metrics use the shared default,
        // including boundary chaining and draggable CSS scrollbars.
        if self.tree.scroll_metrics(id).is_some() {
            return false;
        }
        let step = self
            .tree
            .text_style(id)
            .line_height
            .resolve(self.tree.text_style(id).font_size);
        let delta = match delta {
            MouseScrollDelta::LineDelta(x, y) => Point::new(-x * step, -y * step),
            MouseScrollDelta::PixelDelta(p) => Point::new(
                (-p.x / self.viewport.scale) as f32,
                (-p.y / self.viewport.scale) as f32,
            ),
        };
        self.tree
            .dispatch_input(id, &InputEvent::Scroll(delta), &self.text_layout);
        self.sync_text_input();
        true
    }
    pub(crate) fn text_focus(&mut self, focused: bool) {
        self.native_focused = focused;
        if let Some(id) = self.tree.focused() {
            self.tree.input_focus_changed(id, focused);
        }
        if !focused {
            self.text_pointer(
                PointerPhase::Cancel,
                self.tree.pointer_position().unwrap_or_default(),
                0,
            );
        }
        self.sync_text_input();
        self.request_redraw();
    }
    pub(crate) fn next_text_frame(&self, now: Instant) -> Option<Instant> {
        self.native_focused
            .then(|| self.tree.next_input_frame(now))
            .flatten()
    }
    pub(crate) fn sync_text_input(&mut self) {
        if self
            .input_capture
            .is_some_and(|id| !self.tree.input_enabled(id) || self.tree.text_input(id).is_none())
        {
            self.input_capture = None;
        }
        let target = self
            .input_target()
            .filter(|id| self.tree.attribute(*id, "readonly").is_none());
        if target != self.ime_target {
            if let Some(old) = self.ime_target {
                self.tree.input_focus_changed(old, false);
                self.native.set_ime_allowed(false);
            }
            self.ime_target = target;
            self.ime_bounds = None;
            if let Some(id) = target {
                self.tree.input_focus_changed(id, true);
                self.native.set_ime_allowed(true);
            }
        }
        if self.tree.requires_layout() {
            return;
        }
        if let Some(id) = target {
            let range = self.tree.text_input(id).unwrap().selection_range();
            if let Some(bounds) = self
                .tree
                .input_bounds_for_range(id, range, &self.text_layout)
            {
                // Winit accepts logical coordinates and converts them for each platform.
                let bounds = Rect::from_xywh(
                    bounds.origin.x.max(0.0),
                    bounds.origin.y.max(0.0),
                    bounds.size.width.max(1.0),
                    bounds.size.height.max(1.0),
                );
                if self.ime_bounds != Some(bounds) {
                    self.native.set_ime_cursor_area(
                        winit::dpi::LogicalPosition::new(
                            bounds.origin.x as f64,
                            bounds.origin.y as f64,
                        ),
                        LogicalSize::new(bounds.size.width as f64, bounds.size.height as f64),
                    );
                    self.ime_bounds = Some(bounds);
                }
            }
        }
    }
}
