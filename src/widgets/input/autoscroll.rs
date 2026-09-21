//! Captured source-object selection scrolls at a time-based rate even when the
//! pointer is stationary. Idle controls, buttons and scrollbar drags never tick.
use super::*;
use crate::editing::ViewId;
use winit::keyboard::ModifiersState;

const FRAME_INTERVAL: Duration = Duration::from_millis(16);
const EDGE_LINES: f32 = 1.5;
const SPEED_LINES_PER_SECOND: f32 = 24.0;

pub(super) struct SelectionDrag {
    id: ViewId,
    position: Point<f32>,
    modifiers: ModifiersState,
    bounds: Rect<f32>,
    readonly: bool,
    previous: Instant,
    next: Instant,
}

fn edge_speed(position: f32, start: f32, size: f32, band: f32) -> f32 {
    let band = band.min(size / 2.0).max(1.0);
    if position < start + band {
        -((start + band - position) / band).min(1.0)
    } else if position > start + size - band {
        ((position - start - size + band) / band).min(1.0)
    } else {
        0.0
    }
}

impl TextEdit {
    fn drag_velocity(&self, drag: &SelectionDrag) -> Point<f32> {
        let view = self.view.borrow();
        let width = drag.bounds.size.width.max(view.text.size().width);
        let Some(mut block) = view.text.view_bounds(
            drag.id,
            width,
            self.style.align.resolve(self.style.direction),
        ) else {
            return Point::default();
        };
        block.origin.x += drag.bounds.origin.x - view.scroll.x;
        let left = block.origin.x.max(drag.bounds.origin.x);
        let right =
            (block.origin.x + block.size.width).min(drag.bounds.origin.x + drag.bounds.size.width);
        let line = self.style.line_height.resolve(self.style.font_size);
        let band = line * EDGE_LINES;
        Point::new(
            edge_speed(drag.position.x, left, (right - left).max(0.0), band)
                * line
                * SPEED_LINES_PER_SECOND,
            edge_speed(
                drag.position.y,
                drag.bounds.origin.y,
                drag.bounds.size.height,
                band,
            ) * line
                * SPEED_LINES_PER_SECOND,
        )
    }
    pub(super) fn schedule_drag_scroll(
        &self,
        event: &InputEvent,
        cx: &InputContext<'_>,
        handled: bool,
    ) {
        let InputEvent::Pointer {
            phase,
            position,
            modifiers,
            ..
        } = event
        else {
            return;
        };
        let target = self.view.borrow().text.captured_selection_view();
        if !handled || matches!(phase, PointerPhase::Up | PointerPhase::Cancel) || target.is_none()
        {
            self.view.borrow_mut().selection_drag = None;
            return;
        }
        let drag = SelectionDrag {
            id: target.unwrap(),
            position: *position,
            modifiers: *modifiers,
            bounds: cx.bounds,
            readonly: cx.readonly,
            previous: cx.now,
            next: cx.now + FRAME_INTERVAL,
        };
        let velocity = self.drag_velocity(&drag);
        self.view.borrow_mut().selection_drag =
            (velocity != Point::default()).then(|| Box::new(drag));
    }
    pub(super) fn drag_scroll_deadline(&self) -> Option<Instant> {
        let view = self.view.borrow();
        let drag = view.selection_drag.as_ref()?;
        (view.text.captured_selection_view() == Some(drag.id)).then_some(drag.next)
    }
    pub(super) fn tick_drag_scroll(&mut self, now: Instant) {
        if self
            .drag_scroll_deadline()
            .is_none_or(|deadline| now < deadline)
        {
            return;
        }
        let Some(drag) = self.view.borrow_mut().selection_drag.take() else {
            return;
        };
        let Some(system) = self.view.borrow().system.clone() else {
            return;
        };
        if self
            .editor()
            .with(|state| self.prepare(state, system.clone(), drag.bounds, &self.style))
            .is_err()
        {
            return;
        }
        let velocity = self.drag_velocity(&drag);
        // Bound catch-up after a stalled frame rather than jumping by seconds.
        let elapsed = now
            .saturating_duration_since(drag.previous)
            .min(FRAME_INTERVAL * 4)
            .as_secs_f32();
        let changed = {
            let mut view = self.view.borrow_mut();
            let horizontal = view
                .text
                .scroll_view_source(drag.id, Point::new(velocity.x * elapsed, 0.0));
            let previous = view.scroll;
            let width = drag.bounds.size.width.max(view.text.size().width);
            let block = view.text.view_bounds(
                drag.id,
                width,
                self.style.align.resolve(self.style.direction),
            );
            let desired = view.scroll.y + velocity.y * elapsed;
            view.scroll.y = if let Some(block) = block {
                if velocity.y > 0.0 {
                    desired.min(
                        (block.origin.y + block.size.height - drag.bounds.size.height)
                            .max(view.scroll.y),
                    )
                } else {
                    desired.max(block.origin.y.min(view.scroll.y))
                }
            } else {
                desired
            };
            clamp_scroll(&mut view, drag.bounds);
            let viewport = Rect::new(view.scroll, drag.bounds.size);
            if let Ok(y) = view.text.set_viewport(viewport) {
                view.scroll.y = y;
            }
            horizontal || previous != view.scroll
        };
        if !changed {
            return;
        }
        let cache = render::TextLayoutCache::new(system);
        let style = self.style.clone();
        self.route_embedded_input(
            &InputEvent::Pointer {
                phase: PointerPhase::Move,
                position: drag.position,
                modifiers: drag.modifiers,
                clicks: 1,
            },
            &InputContext {
                bounds: drag.bounds,
                text_layout: &cache,
                style: &style,
                readonly: drag.readonly,
                now,
            },
            false,
        );
        self.repaint();
    }
}
