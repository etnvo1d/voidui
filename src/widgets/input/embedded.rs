//! Route widget interactions before any source-editor selection or command.
use super::*;

impl TextEdit {
    pub(super) fn embedded_cursor(
        &self,
        position: Option<Point<f32>>,
        bounds: Rect<f32>,
    ) -> Option<Cursor> {
        // Text selection uses the host's CSS cursor even when it crosses a widget.
        if self.drag_position.is_some() {
            return None;
        }
        let system = self.view.borrow().system.clone()?;
        self.editor()
            .with(|state| self.prepare(state, system, bounds, &self.style))
            .ok()?;
        let view = self.view.borrow();
        let width = bounds.size.width.max(view.text.size().width);
        let align = self.style.align.resolve(self.style.direction);
        let origin = Point::new(
            bounds.origin.x - view.scroll.x,
            bounds.origin.y - view.scroll.y,
        );
        let target = view.text.captured_view().or_else(|| {
            let point = position?;
            view.text
                .object_at(
                    Point::new(point.x - origin.x, point.y - origin.y),
                    width,
                    align,
                )
                .map(|(id, _, _)| id)
                .or_else(|| {
                    view.text
                        .decoration_at(Point::new(point.x - origin.x, point.y - origin.y))
                        .map(|(id, _)| id)
                })
        })?;
        let mut widget = view.text.view_bounds(target, width, align)?;
        widget.origin.x += origin.x;
        widget.origin.y += origin.y;
        view.text.view_cursor(target, position, widget)
    }
    pub(super) fn route_embedded_input(
        &mut self,
        event: &InputEvent,
        cx: &InputContext<'_>,
        source_gesture: bool,
    ) -> bool {
        let view = self.view.borrow();
        if matches!(
            event,
            InputEvent::Key(KeyInput {
                key: Key::Named(NamedKey::Escape),
                ..
            })
        ) {
            view.text.cancel_view_pointer();
            view.text.release_view_focus();
            return false;
        }
        let width = cx.bounds.size.width.max(view.text.size().width);
        let align = cx.style.align.resolve(cx.style.direction);
        let origin = Point::new(
            cx.bounds.origin.x - view.scroll.x,
            cx.bounds.origin.y - view.scroll.y,
        );
        let target = if let InputEvent::Pointer {
            position, phase, ..
        } = event
        {
            // A text drag stays a text drag when it crosses an embedded view.
            if source_gesture && *phase != PointerPhase::Down {
                return false;
            }
            if *phase == PointerPhase::Down {
                let hit = view
                    .text
                    .object_at(
                        Point::new(position.x - origin.x, position.y - origin.y),
                        width,
                        align,
                    )
                    .map(|(id, _, _)| id)
                    .or_else(|| {
                        view.text
                            .decoration_at(Point::new(position.x - origin.x, position.y - origin.y))
                            .map(|(id, _)| id)
                    });
                if view.text.focused_view() != hit {
                    view.text.release_view_focus();
                }
                hit
            } else {
                view.text.captured_view()
            }
        } else {
            view.text.focused_view()
        };
        let Some(id) = target else { return false };
        let Some(mut bounds) = view.text.view_bounds(id, width, align) else {
            view.text.cancel_view_pointer();
            return false;
        };
        bounds.origin.x += origin.x;
        bounds.origin.y += origin.y;
        let handled = view.text.dispatch_view(
            id,
            event,
            InputContext {
                bounds,
                text_layout: cx.text_layout,
                style: cx.style,
                readonly: cx.readonly,
                now: cx.now,
            },
            self.editor(),
        );
        drop(view);
        self.schedule_drag_scroll(event, cx, handled);
        if handled && matches!(event, InputEvent::Pointer { .. }) {
            // Explicit widget actions may move the caret. As with text clicks,
            // they keep the pointer's viewport instead of revealing a far edge.
            self.view.borrow_mut().reveal_generation = Some(self.editor().with(|s| s.generation()));
        }
        handled
    }
}
