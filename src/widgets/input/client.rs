//! Native text input, pointer gestures, and committed-value notifications.
use super::*;

impl TextInputClient for TextEdit {
    fn pointer_cursor(&self, position: Option<Point<f32>>, bounds: Rect<f32>) -> Option<Cursor> {
        self.embedded_cursor(position, bounds)
    }
    fn mouse_scroll(
        &mut self,
        event: &crate::core::event::MouseEvent,
        bounds: Rect<f32>,
    ) -> crate::core::event::EventResponse {
        use crate::core::event::EventResponse;
        let system = self.view.borrow().system.clone();
        let Some(system) = system else {
            return EventResponse::CONTINUE;
        };
        if let Err(error) = self
            .editor()
            .with(|s| self.prepare(s, system, bounds, &self.style))
        {
            log::error!("editor wheel layout failed: {error:#}");
            return EventResponse::CONTINUE;
        }
        let view = self.view.borrow();
        let origin = Point::new(
            bounds.origin.x - view.scroll.x,
            bounds.origin.y - view.scroll.y,
        );
        let point = Point::new(event.position.x - origin.x, event.position.y - origin.y);
        let target = view
            .text
            .object_at(
                point,
                bounds.size.width,
                self.style.align.resolve(self.style.direction),
            )
            .map(|(id, _, rect)| (id, rect))
            .or_else(|| view.text.decoration_at(point));
        let Some((id, mut object)) = target else {
            return EventResponse::CONTINUE;
        };
        object.origin.x += origin.x;
        object.origin.y += origin.y;
        let response = view
            .text
            .dispatch_view_scroll(id, event, object, self.editor());
        self.repaint();
        response
    }
    fn scroll_content(
        &mut self,
        bounds: Rect<f32>,
        cache: &render::TextLayoutCache,
    ) -> Option<crate::core::scroll::ScrollContent> {
        self.editor()
            .with(|state| self.prepare(state, cache.system().clone(), bounds, &self.style))
            .ok()?;
        let view = self.view.borrow();
        Some(crate::core::scroll::ScrollContent {
            offset: view.scroll,
            size: view.text.size(),
        })
    }
    fn set_scroll_offset(&mut self, offset: Point<f32>) {
        self.view.borrow_mut().scroll = offset;
    }

    fn attributes_changed(&mut self, attributes: &BTreeMap<SmolStr, SmolStr>) {
        let placeholder = attributes.get("placeholder").map_or("", |s| s.as_str());
        if self.placeholder != placeholder {
            self.placeholder = placeholder.into();
            self.view.borrow_mut().placeholder_key = None;
        }
        self.readonly = attributes.contains_key("readonly");
        self.disabled = attributes.contains_key("disabled");
        if self.disabled {
            self.end_pointer_gesture();
        }
        self.rows = attributes
            .get("rows")
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(self.options.rows);
        self.columns = attributes
            .get("cols")
            .or_else(|| attributes.get("size"))
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(self.options.columns);
    }
    fn selected_text(&self) -> String {
        if let Some(text) = self.view.borrow().text.selected_view_text() {
            return text;
        }
        if let Some(text) = self
            .view
            .borrow()
            .text
            .with_focused_input(|input| input.selected_text())
        {
            return text;
        }
        self.editor().with(|s| s.selected_text())
    }
    fn selection_range(&self) -> Range<usize> {
        if let Some(range) = self
            .view
            .borrow()
            .text
            .with_focused_input(|input| input.selection_range())
        {
            return range;
        }
        self.editor().with(|s| display_selection(s).text_range())
    }
    fn text_for_range(&self, range: Range<usize>) -> Option<String> {
        if let Some(text) = self
            .view
            .borrow()
            .text
            .with_focused_input(|input| input.text_for_range(range.clone()))
        {
            return text;
        }
        self.editor().with(|s| display_slice(s, range))
    }
    fn marked_range(&self) -> Option<Range<usize>> {
        if let Some(range) = self
            .view
            .borrow()
            .text
            .with_focused_input(|input| input.marked_range())
        {
            return range;
        }
        self.editor().with(|s| {
            s.composition()
                .map(|c| c.range.start..c.range.start + c.text.len())
        })
    }
    fn is_empty(&self) -> bool {
        self.editor()
            .with(|s| s.document().is_empty() && s.composition().is_none())
    }
    fn update_presentation(
        &mut self,
        style: &TextStyle,
        placeholder: TextStyle,
        selection: SelectionColors,
    ) -> bool {
        // Placeholder and selection colors never reach `Node::computed`, so an
        // edit to those pseudo-elements is only observable here. Comparing before
        // assigning keeps an unchanged sheet from redrawing the window.
        let changed = self.style != *style
            || self.placeholder_style != placeholder
            || self.selection_colors != selection;
        if changed {
            self.style = style.clone();
            self.placeholder_style = placeholder;
            self.selection_colors = selection;
        }
        if !self.style.caret_animation {
            self.blink_at = None;
        } else if self.blink_enabled() && self.blink_at.is_none() {
            self.blink_at = Some(Instant::now() + self.options.blink_interval);
        }
        changed
    }
    fn focus_changed(&mut self, focused: bool, now: Instant) {
        if !focused {
            self.end_pointer_gesture();
            self.view.borrow().text.release_view_focus();
        }
        self.active = focused;
        if !focused {
            self.editor().update(|s| {
                s.cancel_composition();
                s.break_history_group();
            });
        }
        self.reset_caret(now);
    }
    fn next_frame(&self, now: Instant) -> Option<Instant> {
        let child = self
            .view
            .borrow()
            .text
            .with_focused_input(|input| input.next_frame(now))
            .flatten();
        let own = self.blink_enabled().then_some(self.blink_at).flatten();
        [own, child, self.drag_scroll_deadline()]
            .into_iter()
            .flatten()
            .min()
    }
    fn tick(&mut self, now: Instant) {
        self.tick_drag_scroll(now);
        self.view
            .borrow()
            .text
            .with_focused_input_mut(|input| input.tick(now));
        if self.blink_enabled() && self.blink_at.is_some_and(|t| t <= now) {
            self.caret_visible = !self.caret_visible;
            self.blink_at = Some(now + self.options.blink_interval);
            if let Some(invalidator) = &self.invalidator {
                invalidator.caret_visibility(self.caret_visible);
            }
        }
    }
    fn bounds_for_range(&mut self, range: Range<usize>, cx: InputContext<'_>) -> Option<Rect<f32>> {
        if self.view.borrow().text.with_focused_input(|_| ()).is_some() {
            return self
                .view
                .borrow()
                .text
                .focused_bounds_for_range(range, cx.text_layout);
        }
        self.editor().with(|s| {
            self.prepare(s, cx.text_layout.system().clone(), cx.bounds, cx.style)
                .ok()?;
            let view = self.view.borrow();
            if range.start > range.end
                || !display_boundary(s, range.start)
                || !display_boundary(s, range.end)
            {
                return None;
            }
            let width = cx.bounds.size.width.max(view.text.size().width);
            let align = cx.style.align.resolve(cx.style.direction);
            // The active caret must use the same side of a wrap or inserted view
            // as painting. Arbitrary range starts keep their downstream meaning.
            let selection = display_selection(s);
            let bias = if range.is_empty() && range.start == selection.head {
                selection.affinity
            } else {
                Bias::After
            };
            let mut bounds = view.text.caret(range.start, bias, width, align)?;
            if range.is_empty() {
                return Some(view::viewport_caret(bounds, cx.bounds, view.scroll));
            }
            let end = view.text.caret(range.end, Bias::Before, width, align)?;
            bounds.size.width = if end.origin.y == bounds.origin.y {
                (end.origin.x - bounds.origin.x).abs().max(1.0)
            } else {
                (width - bounds.origin.x).max(1.0)
            };
            if end.origin.y == bounds.origin.y {
                bounds.origin.x = bounds.origin.x.min(end.origin.x);
            }
            bounds.origin.x += cx.bounds.origin.x - view.scroll.x;
            bounds.origin.y += cx.bounds.origin.y - view.scroll.y;
            Some(bounds)
        })
    }
    fn handle_input(&mut self, event: &InputEvent, cx: InputContext<'_>) -> EventResult {
        let source_gesture = self.drag_position.is_some();
        if !matches!(
            event,
            InputEvent::Pointer {
                phase: PointerPhase::Move | PointerPhase::Up | PointerPhase::Cancel,
                ..
            } | InputEvent::Scroll(_)
        ) {
            self.view.borrow().text.cancel_view_pointer();
        }
        let stale_gesture = self
            .view
            .borrow()
            .pointer_projection
            .as_ref()
            .is_some_and(|p| {
                self.editor()
                    .with(|s| p.0 != s.revision() || s.composition().is_some())
            });
        if stale_gesture
            || !matches!(
                event,
                InputEvent::Pointer {
                    phase: PointerPhase::Move,
                    ..
                } | InputEvent::Scroll(_)
            )
        {
            self.end_text_gesture();
        }
        if let Err(error) = self
            .editor()
            .with(|s| self.prepare(s, cx.text_layout.system().clone(), cx.bounds, cx.style))
        {
            log::error!("text input layout failed: {error:#}");
            return EventResult::Unhandled;
        }
        let revision = self.editor().with(|s| s.revision());
        if self.route_embedded_input(event, &cx, source_gesture) {
            if self.editor().with(|s| s.revision()) != revision {
                self.publish_value();
                if let Some(callback) = &self.on_change {
                    callback(self.editor());
                }
            }
            self.repaint();
            return EventResult::Handled;
        }
        // A structured selection belongs to its view, not to the fallback text
        // caret retained for scrolling. If that view releases focus, typing or
        // IME must not silently turn the navigation anchor into editable source.
        if self.editor().with(|s| s.selections().is_structured())
            && matches!(
                event,
                InputEvent::Text(_)
                    | InputEvent::Paste(_)
                    | InputEvent::Cut
                    | InputEvent::Preedit(..)
                    | InputEvent::Commit(_)
                    | InputEvent::Key(KeyInput {
                        key: Key::Named(
                            NamedKey::Backspace
                                | NamedKey::Delete
                                | NamedKey::Enter
                                | NamedKey::Tab
                        ),
                        ..
                    })
            )
        {
            return EventResult::Handled;
        }
        if !cx.readonly
            && !self.editor().with(|s| {
                (s.composition().is_some() && !matches!(event, InputEvent::Commit(_)))
                    || s.selections().is_structured()
            })
        {
            let tx = self.editor().with(|s| {
                self.extension_host
                    .borrow_mut()
                    .command(event, &s.snapshot(), s.last_change())
            });
            match tx {
                Ok(Some(tx)) => {
                    let result = self.editor().update(|s| {
                        if matches!(event, InputEvent::Commit(_)) {
                            s.commit_transaction(tx)
                        } else {
                            s.transact(tx)
                        }
                    });
                    if result.is_ok() {
                        self.publish_value();
                        if let Some(cb) = &self.on_change {
                            cb(self.editor());
                        }
                        self.reset_caret(cx.now);
                    }
                    return EventResult::Handled;
                }
                Err(error) => {
                    log::error!("editor extension command failed: {error}");
                    return EventResult::Unhandled;
                }
                _ => {}
            }
        }

        let handled = match event {
            InputEvent::Key(key) => self.key(key, &cx),
            InputEvent::Text(text) | InputEvent::Paste(text) => {
                if !cx.readonly && !self.editor().with(|s| s.composition().is_some()) {
                    let text = self.normalized(text);
                    if !text.is_empty() {
                        self.editor()
                            .update(|s| {
                                s.replace_selections(
                                    &text,
                                    if matches!(event, InputEvent::Paste(_)) {
                                        EditKind::Paste
                                    } else {
                                        EditKind::Typing
                                    },
                                )
                            })
                            .ok();
                    }
                }
                true
            }
            InputEvent::Cut => {
                if !cx.readonly {
                    self.editor()
                        .update(|s| s.replace_selections("", EditKind::Command))
                        .ok();
                }
                true
            }
            InputEvent::Preedit(text, cursor) => {
                if !cx.readonly {
                    self.editor()
                        .update(|s| s.set_composition(text, *cursor))
                        .ok();
                }
                true
            }
            InputEvent::Commit(text) => {
                if !cx.readonly {
                    let text = self.normalized(text);
                    self.editor().update(|s| s.commit_composition(&text)).ok();
                }
                true
            }
            InputEvent::CancelComposition => {
                self.editor().update(|s| s.cancel_composition());
                true
            }
            InputEvent::Scroll(delta) => {
                let mut view = self.view.borrow_mut();
                view.scroll.x += delta.x;
                view.scroll.y += delta.y;
                clamp_scroll(&mut view, cx.bounds);
                self.repaint();
                true
            }
            InputEvent::Pointer {
                phase,
                position,
                modifiers,
                clicks,
            } => {
                if *phase == PointerPhase::Cancel {
                    self.drag_position = None;
                    return EventResult::Handled;
                }
                if *phase == PointerPhase::Up {
                    self.drag_position = None;
                    return EventResult::Handled;
                }
                if *phase == PointerPhase::Move {
                    if self.drag_position.is_none() {
                        return EventResult::Unhandled;
                    }
                }
                self.editor().update(|s| s.cancel_composition());
                let mut view = self.view.borrow_mut();
                if *phase == PointerPhase::Down
                    && (!self.extensions.is_empty()
                        || self.editor().with(|s| s.projection_revision() != 0))
                {
                    view.pointer_projection = view
                        .text
                        .projection_snapshot()
                        .map(|plan| Box::new((revision, plan)));
                }
                let point = Point::new(
                    position.x - cx.bounds.origin.x + view.scroll.x,
                    position.y - cx.bounds.origin.y + view.scroll.y,
                );
                // Compare in content coordinates: unchanged native notifications
                // preserve the selection, while scrolling can move its target.
                if *phase == PointerPhase::Move && self.drag_position == Some(point) {
                    return EventResult::Handled;
                }
                let width = cx.bounds.size.width.max(view.text.size().width);
                let align = cx.style.align.resolve(cx.style.direction);
                let mut hit = if *phase == PointerPhase::Down {
                    view.text
                        .object_at(point, width, align)
                        .map(|(_, r, _)| Selection::range(r.start, r.end))
                        .unwrap_or_else(|| view.text.hit_test(point, width, align))
                } else {
                    view.text.hit_test(point, width, align)
                };
                if *phase == PointerPhase::Down && *clicks >= 2 {
                    let range = if *clicks == 2 {
                        view.text.word_at(point, width, align)
                    } else {
                        view.text.hard_line_at(point, width, align)
                    };
                    hit = Selection::range(range.start, range.end);
                }
                self.editor().update(|s| {
                    if *phase == PointerPhase::Move || modifiers.shift_key() {
                        hit.anchor = s.selections().primary().anchor;
                    }
                    let selection = if *phase == PointerPhase::Down
                        && modifiers.alt_key()
                        && !modifiers.shift_key()
                    {
                        let mut selections: Vec<_> = s.selections().iter().copied().collect();
                        selections.push(hit);
                        let primary = selections.len() - 1;
                        SelectionSet::new(selections, primary).unwrap()
                    } else {
                        SelectionSet::single(hit)
                    };
                    s.select(selection).ok();
                });
                // Pointer selection owns the viewport. Keyboard and text edits
                // still request normal caret reveal through a new generation.
                view.reveal_generation = Some(self.editor().with(|s| s.generation()));
                if *phase == PointerPhase::Move {
                    view::reveal_drag_point(&mut view, point, cx.bounds);
                }
                self.drag_position = Some(point);
                true
            }
        };
        if handled {
            self.reset_caret(cx.now);
            if self.editor().with(|s| s.revision()) != revision {
                self.publish_value();
                if let Some(callback) = &self.on_change {
                    callback(self.editor());
                }
            }
            EventResult::Handled
        } else {
            EventResult::Unhandled
        }
    }
}
