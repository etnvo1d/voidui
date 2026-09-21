//! Wheel chaining, keyboard navigation and pointer capture use retained scroll metrics.
use super::*;

#[derive(Default)]
pub(crate) struct ScrollOutcome {
    pub changed: bool,
    /// Includes overscroll containment at an edge, even when nothing moved.
    pub consumed: bool,
}

impl WidgetTree {
    /// Consume each wheel axis in the nearest scrolling ancestor; only unused
    /// distance chains outward. Contain/none stop chaining without rubber-banding.
    pub fn scroll_wheel(
        &mut self,
        wheel: crate::core::event::MouseWheel,
        modifiers: crate::core::event::ModifiersState,
    ) -> bool {
        if self.pointer_capture().is_some() {
            return false;
        }
        let Some(crate::core::top_layer::HitTarget::Element(id)) =
            self.pointer_position.and_then(|p| self.hit_test(p))
        else {
            return false;
        };
        self.scroll_wheel_from(id, wheel, modifiers).changed
    }
    /// Reuse the wheel event's already hit-tested target instead of walking the
    /// retained paint list again for the default action.
    pub(crate) fn scroll_wheel_from(
        &mut self,
        mut id: WidgetId,
        wheel: crate::core::event::MouseWheel,
        modifiers: crate::core::event::ModifiersState,
    ) -> ScrollOutcome {
        if !self.layout_ready || self.pointer_capture().is_some() || !self.nodes.contains_key(id) {
            return ScrollOutcome::default();
        }
        let step = self
            .text_style(id)
            .line_height
            .resolve(self.text_style(id).font_size);
        let scale = if wheel.unit == crate::core::event::WheelUnit::Lines {
            step
        } else {
            1.0
        };
        let mut delta = Point::new(-wheel.x * scale, -wheel.y * scale);
        if !delta.x.is_finite() || !delta.y.is_finite() {
            return ScrollOutcome::default();
        }
        if modifiers.shift_key() && delta.x == 0.0 {
            delta = Point::new(delta.y, 0.0);
        }
        let mut changed = false;
        let mut contained = false;
        loop {
            let o = self.nodes[id].computed.scroll.overflow;
            let chain = self.nodes[id].computed.scroll.overscroll;
            if let Some(before) = self.scroll_metrics(id) {
                changed |= self.scroll_by(
                    id,
                    Point::new(
                        if o.x.allows_user_scroll() {
                            delta.x
                        } else {
                            0.0
                        },
                        if o.y.allows_user_scroll() {
                            delta.y
                        } else {
                            0.0
                        },
                    ),
                );
                let after = self.scroll_metrics(id).unwrap();
                delta.x -= after.offset.x - before.offset.x;
                delta.y -= after.offset.y - before.offset.y;
                if o.x.is_scroll_container() && chain.x != OverscrollBehavior::Auto {
                    contained |= delta.x != 0.0;
                    delta.x = 0.0;
                }
                if o.y.is_scroll_container() && chain.y != OverscrollBehavior::Auto {
                    contained |= delta.y != 0.0;
                    delta.y = 0.0;
                }
            }
            if delta == Point::default() {
                break;
            }
            let Some(parent) = self.nodes[id].layout_parent else {
                break;
            };
            id = parent;
            if self.is_inert(id) {
                break;
            }
        }
        ScrollOutcome {
            changed,
            consumed: changed || contained,
        }
    }
    pub(crate) fn scrollbar_at(&self, point: Point<f32>) -> Option<(WidgetId, ScrollbarGeometry)> {
        let crate::core::top_layer::HitTarget::Element(id) = self.hit_test(point)? else {
            return None;
        };
        let point = self.window_to_layout(id, point)?;
        [ScrollAxis::Vertical, ScrollAxis::Horizontal]
            .into_iter()
            .find_map(|axis| {
                let g = self.scrollbar_geometry(id, axis)?;
                contains(g.track, point).then_some((id, g))
            })
    }
    pub(crate) fn start_scrollbar_drag(&mut self, point: Point<f32>) -> bool {
        let Some((id, g)) = self.scrollbar_at(point) else {
            return false;
        };
        let Some(point) = self.window_to_layout(id, point) else {
            return false;
        };
        self.click_pressed = None;
        self.end_selection_drag();
        if contains(g.thumb, point) {
            self.scroll_drag = Some(ScrollDrag {
                owner: id,
                axis: g.axis,
                grab: Some(axis_point(point, g.axis) - axis_point(g.thumb.origin, g.axis)),
            });
        } else {
            let m = self.scroll_metrics(id).unwrap();
            let direction = if axis_point(point, g.axis) < axis_point(g.thumb.origin, g.axis) {
                -1.0
            } else {
                1.0
            };
            self.scroll_by(
                id,
                match g.axis {
                    ScrollAxis::Horizontal => Point::new(direction * m.viewport.width, 0.0),
                    ScrollAxis::Vertical => Point::new(0.0, direction * m.viewport.height),
                },
            );
            // Keep ownership until release even for a track click, preventing a
            // click or text selection from being delivered to newly exposed content.
            self.scroll_drag = Some(ScrollDrag {
                owner: id,
                axis: g.axis,
                grab: None,
            });
        }
        true
    }
    pub(crate) fn move_scrollbar_drag(&mut self, point: Point<f32>) -> bool {
        let Some(drag) = self.scroll_drag else {
            return false;
        };
        let Some(point) = self.window_to_layout(drag.owner, point) else {
            return false;
        };
        let Some(grab) = drag.grab else {
            return false;
        };
        let Some(g) = self.scrollbar_geometry(drag.owner, drag.axis) else {
            return false;
        };
        let m = self.scroll_metrics(drag.owner).unwrap();
        let travel = axis_length(g.track, g.axis) - axis_length(g.thumb, g.axis);
        let fraction = if travel > 0.0 {
            ((axis_point(point, g.axis) - axis_point(g.track.origin, g.axis) - grab) / travel)
                .clamp(0.0, 1.0)
        } else {
            0.0
        };
        let next = match g.axis {
            ScrollAxis::Horizontal => {
                Point::new(m.min.x + fraction * (m.max.x - m.min.x), m.offset.y)
            }
            ScrollAxis::Vertical => Point::new(m.offset.x, fraction * m.max.y),
        };
        self.scroll_to(drag.owner, next)
    }
    /// Keyboard scrolling for custom hosts, after editable/button defaults and
    /// key listeners. Focused containers accept arrows, Page Up/Down, Home/End,
    /// and Space; otherwise the root viewport is the default target.
    pub fn scroll_key(
        &mut self,
        key: &crate::core::event::Key,
        modifiers: crate::core::event::ModifiersState,
    ) -> bool {
        use crate::core::event::{Key, NamedKey};
        if modifiers.control_key() || modifiers.super_key() || modifiers.alt_key() {
            return false;
        }
        let Key::Named(key) = key else {
            return false;
        };
        if !matches!(
            key,
            NamedKey::ArrowLeft
                | NamedKey::ArrowRight
                | NamedKey::ArrowUp
                | NamedKey::ArrowDown
                | NamedKey::PageUp
                | NamedKey::PageDown
                | NamedKey::Home
                | NamedKey::End
                | NamedKey::Space
        ) {
            return false;
        }
        let mut current = self.focused().or(self.root);
        while let Some(id) = current {
            if self.is_inert(id) {
                break;
            }
            if let Some(m) = self.scroll_metrics(id) {
                let o = self.nodes[id].computed.scroll.overflow;
                let step = self
                    .text_style(id)
                    .line_height
                    .resolve(self.text_style(id).font_size);
                let mut next = m.offset;
                match key {
                    NamedKey::ArrowLeft if o.x.allows_user_scroll() => next.x -= step,
                    NamedKey::ArrowRight if o.x.allows_user_scroll() => next.x += step,
                    _ if o.y.allows_user_scroll() => match key {
                        NamedKey::ArrowUp => next.y -= step,
                        NamedKey::ArrowDown => next.y += step,
                        NamedKey::PageUp => next.y -= m.viewport.height,
                        NamedKey::PageDown => next.y += m.viewport.height,
                        NamedKey::Home => next.y = m.min.y,
                        NamedKey::End => next.y = m.max.y,
                        NamedKey::Space => {
                            next.y += if modifiers.shift_key() {
                                -m.viewport.height
                            } else {
                                m.viewport.height
                            }
                        }
                        _ => (),
                    },
                    _ => (),
                }
                if self.scroll_to(id, next) {
                    return true;
                }
                let chain = self.nodes[id].computed.scroll.overscroll;
                let contained = if matches!(key, NamedKey::ArrowLeft | NamedKey::ArrowRight) {
                    o.x.is_scroll_container() && chain.x != OverscrollBehavior::Auto
                } else {
                    o.y.is_scroll_container() && chain.y != OverscrollBehavior::Auto
                };
                if contained {
                    return true;
                }
            }
            current = self.nodes[id].layout_parent;
        }
        false
    }
    /// Reveal an element with the smallest necessary movement in each containing
    /// scrollport. Fixed/top-layer positioning follows its actual layout owner.
    pub fn scroll_into_view(&mut self, id: WidgetId) -> bool {
        if !self.layout_ready {
            return false;
        }
        let Some(node) = self.nodes.get(id) else {
            return false;
        };
        let mut parent = node.layout_parent;
        let mut changed = false;
        let distance = |start: f32, length: f32, view_start: f32, view_length: f32| {
            if start < view_start && start + length > view_start + view_length {
                0.0
            } else if start < view_start {
                start - view_start
            } else if start + length > view_start + view_length {
                (start + length - view_start - view_length).min(start - view_start)
            } else {
                0.0
            }
        };
        while let Some(owner) = parent {
            let b = self.bounds(id);
            let view = self.scrollport(owner);
            changed |= self.scroll_by(
                owner,
                Point::new(
                    distance(b.origin.x, b.size.width, view.origin.x, view.size.width),
                    distance(b.origin.y, b.size.height, view.origin.y, view.size.height),
                ),
            );
            parent = self.nodes[owner].layout_parent;
        }
        changed
    }
}
