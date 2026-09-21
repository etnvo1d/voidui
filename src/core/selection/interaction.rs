//! Pointer hit testing and cursor policy over retained glyph geometry.
use super::*;

impl WidgetTree {
    /// Return a caret's top edge in logical window coordinates. At a soft-wrap
    /// boundary the downstream row is preferred. No new text shaping is performed.
    pub fn text_caret_position(&self, id: WidgetId, byte: usize) -> Option<Point<f32>> {
        let node = self.nodes.get(id)?;
        let text = node.widget.as_ref()?.prepared_text()?;
        let bounds = self.scrolled_content_bounds(id);
        let p = text.caret_position(
            byte,
            bounds.size.width,
            node.computed
                .text
                .align
                .resolve(node.computed.text.direction),
        )?;
        Some(self.layout_to_window(id, Point::new(bounds.origin.x + p.x, bounds.origin.y + p.y)))
    }
    fn text_point_at(&self, id: WidgetId, point: Point<f32>) -> Option<SelectionPoint> {
        let point = self.window_to_layout(id, point)?;
        let node = &self.nodes[id];
        let text = node.widget.as_ref()?.prepared_text()?;
        let bounds = self.scrolled_content_bounds(id);
        let byte = text.hit_position(
            Point::new(point.x - bounds.origin.x, point.y - bounds.origin.y),
            bounds.size.width,
            node.computed
                .text
                .align
                .resolve(node.computed.text.direction),
        )?;
        Some(SelectionPoint::text(id, byte))
    }
    fn closest_text(
        &self,
        index: &DocumentIndex,
        point: Point<f32>,
        scope: Option<WidgetId>,
        selectable_only: bool,
    ) -> Option<SelectionPoint> {
        let mut best = None;
        let mut distance = f32::INFINITY;
        for id in &index.texts {
            if !self.selection_visible(*id)
                || scope.is_some_and(|scope| !self.is_descendant_or_self(*id, scope))
                || (selectable_only && self.used_select(*id) == UserSelect::None)
            {
                continue;
            }
            let bounds = self.layout_rect_to_window(*id, self.scrolled_content_bounds(*id));
            if bounds.is_empty() {
                continue;
            }
            let dx = (bounds.origin.x - point.x)
                .max(0.0)
                .max(point.x - bounds.origin.x - bounds.size.width);
            let dy = (bounds.origin.y - point.y)
                .max(0.0)
                .max(point.y - bounds.origin.y - bounds.size.height);
            let d = dx * dx + dy * dy;
            if d < distance {
                best = Some(*id);
                distance = d;
            }
        }
        // Build caret geometry only for the winning text box, not every box that
        // temporarily improves the nearest-distance candidate during the scan.
        best.and_then(|id| self.text_point_at(id, point))
    }
    fn point_for_selection(
        &self,
        index: &DocumentIndex,
        point: Point<f32>,
        drag: bool,
    ) -> Option<(SelectionPoint, UserSelect)> {
        self.ensure_paint_order();
        for entry in self.paint_order.borrow().entries.iter().rev() {
            if !self.entry_contains(entry, point) {
                continue;
            }
            let node = &self.nodes[entry.id];
            if !self.selection_visible(entry.id) {
                continue;
            }
            if entry.phase == crate::core::stacking::Phase::Backdrop {
                if node.backdrop.as_ref().is_some_and(|b| {
                    contains(b.bounds, point)
                        && b.style.layer.pointer_events != crate::style::layer::PointerEvents::None
                }) {
                    if drag {
                        break;
                    }
                    return None;
                }
                continue;
            }
            if !self
                .window_to_layout(entry.id, point)
                .is_some_and(|p| contains(node.global_bounds, p))
            {
                continue;
            }
            let used = self.used_select(entry.id);
            if let Some(hit) = self.text_point_at(entry.id, point) {
                return Some((hit, used));
            }
            if used == UserSelect::None {
                return Some((self.contents(entry.id).anchor, used));
            }
            if let Some(hit) = self.closest_text(index, point, Some(entry.id), true) {
                return Some((hit, self.used_select(hit.node())));
            }
            if !drag {
                return Some((self.contents(entry.id).anchor, used));
            }
            break;
        }
        if drag {
            self.closest_text(index, point, self.active_modal(), false)
                .map(|p| (p, self.used_select(p.node())))
        } else {
            None
        }
    }
    fn unit_at(
        &self,
        p: SelectionPoint,
        granularity: SelectionGranularity,
        pointer: Point<f32>,
    ) -> Selection {
        let collapsed = Selection {
            anchor: p,
            focus: p,
        };
        if granularity == SelectionGranularity::Grapheme {
            return collapsed;
        }
        let SelectionPoint::Text { node, .. } = p else {
            return collapsed;
        };
        let Some(text) = self.nodes[node]
            .widget
            .as_ref()
            .and_then(|w| w.prepared_text())
        else {
            return collapsed;
        };
        let b = self.scrolled_content_bounds(node);
        let Some(pointer) = self.window_to_layout(node, pointer) else {
            return collapsed;
        };
        let Some(range) = text.selection_unit(
            Point::new(pointer.x - b.origin.x, pointer.y - b.origin.y),
            b.size.width,
            self.nodes[node]
                .computed
                .text
                .align
                .resolve(self.nodes[node].computed.text.direction),
            granularity == SelectionGranularity::Word,
        ) else {
            return collapsed;
        };
        Selection {
            anchor: SelectionPoint::text(node, range.start),
            focus: SelectionPoint::text(node, range.end),
        }
    }
    fn cursor_for_pointer(
        &self,
        focus: SelectionPoint,
        point: Point<f32>,
    ) -> Option<(WidgetId, voidui_gpui_wgpu::parley::Selection)> {
        use voidui_gpui_wgpu::parley::{Affinity, Cursor};
        let SelectionPoint::Text { node, byte } = focus else {
            return None;
        };
        let text = self.nodes[node].widget.as_ref()?.prepared_text()?;
        let b = self.scrolled_content_bounds(node);
        let point = self.window_to_layout(node, point)?;
        let cursor = text.cursor_at(
            Point::new(point.x - b.origin.x, point.y - b.origin.y),
            b.size.width,
            self.nodes[node]
                .computed
                .text
                .align
                .resolve(self.nodes[node].computed.text.direction),
        )?;
        let cursor = if text.paragraph().source_index(cursor.index()) == byte {
            cursor
        } else {
            Cursor::from_byte_index(
                text.paragraph().layout(),
                text.paragraph().layout_index(byte),
                Affinity::Upstream,
            )
        };
        Some((node, cursor.into()))
    }
    /// Begin a user gesture. `clicks` is supplied by the native input policy:
    /// single=caret, double=Unicode word, triple=paragraph. None never clears an
    /// existing selection, including when the drag would leave the none element.
    pub fn selection_pointer_down(&mut self, point: Point<f32>, extend: bool, clicks: u8) -> bool {
        self.end_selection_drag();
        if !self.layout_ready {
            return false;
        }
        self.ensure_selection_index();
        let pointer = point;
        let (range, origin, granularity) = {
            let state = self.selection.borrow();
            let Some((point, used)) = self.point_for_selection(&state.index, point, false) else {
                return false;
            };
            if used == UserSelect::None {
                return false;
            }
            let granularity = match clicks {
                0 | 1 => SelectionGranularity::Grapheme,
                2 => SelectionGranularity::Word,
                _ => SelectionGranularity::Paragraph,
            };
            let mut range = self.unit_at(point, granularity, pointer);
            if extend && let Some(old) = state.range {
                range.anchor = old.anchor;
            }
            (
                self.normalize_user_selection(&state.index, point, range),
                point,
                granularity,
            )
        };
        let local_cursor = self.cursor_for_pointer(range.focus, pointer);
        let cursor = self.selection_cursor(pointer);
        let state = self.selection.get_mut();
        let Some(changed) = state.set_user(range) else {
            return false;
        };
        state.local_cursor = local_cursor;
        state.drag = Some(Drag {
            seed: range,
            origin,
            granularity,
            cursor,
        });
        changed
    }
    pub fn selection_pointer_move(&mut self, point: Point<f32>) -> bool {
        if !self.layout_ready {
            return false;
        }
        self.ensure_selection_index();
        let range = {
            let state = self.selection.borrow();
            let Some(drag) = state.drag else {
                return false;
            };
            if self.is_inert(drag.origin.node()) {
                drop(state);
                self.selection.get_mut().drag = None;
                return false;
            }
            let Some((mut focus, used)) = self.point_for_selection(&state.index, point, true)
            else {
                return false;
            };
            if used == UserSelect::None {
                let mut none = focus.node();
                while let Some(parent) = self.nodes[none].parent {
                    if self.used_select(parent) != UserSelect::None {
                        break;
                    }
                    none = parent;
                }
                if self.within(drag.origin, none) {
                    // A selectable descendant can opt back in. Dragging onto its
                    // none ancestor's padding must not jump outside that ancestor.
                    focus = self
                        .closest_text(&state.index, point, Some(none), true)
                        .unwrap_or(drag.origin);
                } else {
                    let forward = state.index.offset(self, drag.seed.anchor)
                        <= state.index.offset(self, focus);
                    focus = if forward {
                        self.before(none)
                    } else {
                        self.after(none)
                    };
                }
            }
            let forward =
                state.index.offset(self, drag.seed.anchor) <= state.index.offset(self, focus);
            let unit = self.unit_at(focus, drag.granularity, point);
            let range = if forward {
                Selection {
                    anchor: drag.seed.anchor,
                    focus: unit.focus,
                }
            } else {
                Selection {
                    anchor: drag.seed.focus,
                    focus: unit.anchor,
                }
            };
            self.normalize_user_selection(&state.index, drag.origin, range)
        };
        let cursor = self.cursor_for_pointer(range.focus, point);
        let state = self.selection.get_mut();
        if let Some(changed) = state.set_user(range) {
            state.local_cursor = cursor;
            changed
        } else {
            false
        }
    }
    /// CSS cursor used at a point. Pointer-events does not disable text-selection
    /// hit testing; a selector can opt out with user-select:none instead.
    pub fn selection_cursor(&self, point: Point<f32>) -> crate::style::selection::Cursor {
        use crate::style::selection::Cursor;
        if !self.layout_ready {
            return Cursor::Auto;
        }
        let hit = self.hit_test(point);
        if let Some(crate::core::top_layer::HitTarget::Backdrop(id)) = hit {
            return self.nodes[id]
                .backdrop
                .as_ref()
                .map(|b| b.style.selection.cursor)
                .unwrap_or_default();
        }
        let node = match hit {
            Some(crate::core::top_layer::HitTarget::Element(id)) => Some(id),
            _ => None,
        };
        if let Some(id) = node {
            let cursor = self.nodes[id].computed.selection.cursor;
            if cursor != Cursor::Auto {
                return cursor;
            }
        }
        if let Some(drag) = self.selection.borrow().drag {
            return drag.cursor;
        }
        // Cursor hover needs only existing text boxes, not a document index or
        // grapheme geometry. The heavier hit mapping remains lazy until selection.
        self.ensure_paint_order();
        for entry in self.paint_order.borrow().entries.iter().rev() {
            let n = &self.nodes[entry.id];
            if !self.selection_visible(entry.id)
                || !self.entry_contains(entry, point)
                || !self
                    .window_to_layout(entry.id, point)
                    .is_some_and(|p| contains(n.global_bounds, p))
            {
                continue;
            }
            if n.widget
                .as_ref()
                .is_some_and(|w| w.prepared_text().is_some())
                && self.used_select(entry.id) != UserSelect::None
            {
                return Cursor::Icon(winit::window::CursorIcon::Text);
            }
            return Cursor::Auto;
        }
        Cursor::Auto
    }
}
fn contains(b: Rect<f32>, p: Point<f32>) -> bool {
    p.x >= b.origin.x
        && p.y >= b.origin.y
        && p.x < b.origin.x + b.size.width
        && p.y < b.origin.y + b.size.height
}

impl WidgetTree {
    /// Active highlight rectangles in logical window coordinates, including
    /// disjoint pieces of bidirectional text. Ancestor paint clipping still applies.
    pub fn selection_rectangles(&self, id: WidgetId) -> Vec<Rect<f32>> {
        let Some(range) = self.selected_range(id) else {
            return Vec::new();
        };
        let node = &self.nodes[id];
        let Some(text) = node.widget.as_ref().and_then(|w| w.prepared_text()) else {
            return Vec::new();
        };
        let bounds = self.scrolled_content_bounds(id);
        text.selection_rectangles(
            range,
            bounds.size.width,
            node.computed
                .text
                .align
                .resolve(node.computed.text.direction),
        )
        .into_iter()
        .map(|(_, rect)| {
            self.layout_rect_to_window(
                id,
                Rect::from_xywh(
                    bounds.origin.x + rect.origin.x,
                    bounds.origin.y + rect.origin.y,
                    rect.size.width,
                    rect.size.height,
                ),
            )
        })
        .collect()
    }
}
