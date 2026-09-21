//! Transform overflow expands (never shrinks) the normal layout overflow. Work
//! is bottom-up, and only altered rectangles retain a baseline for later frames.
use super::*;
use crate::render::Affine;
impl WidgetTree {
    pub(crate) fn transform_gutters_need_layout(&self) -> bool {
        self.scrollers.iter().any(|id| {
            if self.scrollbar_mode(*id) != ScrollbarMode::Classic {
                return false;
            }
            let n = &self.nodes[*id];
            let s = n.scroll.as_ref().unwrap();
            let o = &n.computed.scroll;
            (o.overflow.x == Overflow::Auto && s.reserved.x && !s.bars.x)
                || (o.overflow.y == Overflow::Auto
                    && s.reserved.y
                    && !s.bars.y
                    && o.gutter == ScrollbarGutter::Auto)
        })
    }

    /// Restore the normal-flow rectangles before either reusing cached layout or
    /// computing new geometry. Expanded paint overflow must not become its baseline.
    pub(crate) fn restore_transform_overflow(&mut self) {
        for (id, rect) in self.transform_overflow.drain() {
            if let Some(n) = self.nodes.get_mut(id) {
                n.layout.scrollable_overflow_rect = rect;
            }
        }
    }

    pub(crate) fn update_transform_overflow(&mut self) {
        self.restore_transform_overflow();
        if !self.nodes.values().any(|n| !n.computed.transform.is_none()) {
            return;
        }
        fn visit(tree: &mut WidgetTree, id: WidgetId) -> bool {
            let mut changed = false;
            for index in 0..tree.layout_children(id).len() {
                let child = tree.layout_children(id)[index];
                let child_changed = visit(tree, child);
                let c = &tree.nodes[child];
                if !c.rendered || (c.computed.transform.is_none() && !child_changed) {
                    continue;
                }
                let l = &c.layout;
                let o = c.computed.scroll.overflow;
                let scroll = o.x.is_scroll_container() || o.y.is_scroll_container();
                let contain = c.computed.layout.contain.contains_scrollable_overflow();
                let propagate_x = !scroll && !contain && o.x == Overflow::Visible;
                let propagate_y = !scroll && !contain && o.y == Overflow::Visible;
                let r = l.scrollable_overflow_rect;
                // Taffy stores overflow from the logical start of the padding box.
                let rtl = c.computed.layout.direction == layout::Direction::Rtl;
                let left = if propagate_x {
                    if rtl {
                        (l.size.width - l.border.right - r.right).min(0.)
                    } else {
                        (r.left + l.border.left).min(0.)
                    }
                } else {
                    0.
                };
                let right = if propagate_x {
                    if rtl {
                        (l.size.width - l.border.right - r.left).max(l.size.width)
                    } else {
                        (r.right + l.border.left).max(l.size.width)
                    }
                } else {
                    l.size.width
                };
                let top = if propagate_y {
                    (r.top + l.border.top).min(0.)
                } else {
                    0.
                };
                let bottom = if propagate_y {
                    (r.bottom + l.border.top).max(l.size.height)
                } else {
                    l.size.height
                };
                let origin = &c.computed.transform_origin;
                let x = origin.x.resolve(l.size.width);
                let y = origin.y.resolve(l.size.height);
                let m = Affine::translate(x, y)
                    .compose(c.computed.transform.matrix(l.size.width, l.size.height))
                    .compose(Affine::translate(-x, -y));
                if m.inverse().is_none() {
                    continue;
                }
                let b = m.map_bounds(
                    crate::core::paint::render_bounds(Rect::from_xywh(
                        left,
                        top,
                        right - left,
                        bottom - top,
                    ))
                    .scale(1.),
                );
                let p = &tree.nodes[id];
                let pl = &p.layout;
                let left = b.origin.x.0 + l.location.x;
                let right = left + b.size.width.0;
                let top = b.origin.y.0 + l.location.y - pl.border.top;
                let bottom = top + b.size.height.0;
                let (left, right) = if p.computed.layout.direction == layout::Direction::Rtl {
                    (
                        pl.size.width - pl.border.right - right,
                        pl.size.width - pl.border.right - left,
                    )
                } else {
                    (left - pl.border.left, right - pl.border.left)
                };
                let pscroll = p.computed.scroll.overflow.x.is_scroll_container()
                    || p.computed.scroll.overflow.y.is_scroll_container();
                if pscroll && (right <= 0. || bottom <= 0.) {
                    continue;
                }
                let mut next = pl.scrollable_overflow_rect;
                next.left = next.left.min(left);
                next.right = next.right.max(right);
                next.top = next.top.min(top);
                next.bottom = next.bottom.max(bottom);
                if next != pl.scrollable_overflow_rect {
                    tree.transform_overflow
                        .entry(id)
                        .or_insert(pl.scrollable_overflow_rect);
                    tree.nodes[id].layout.scrollable_overflow_rect = next;
                    changed = true;
                }
            }
            changed
        }
        if let Some(root) = self.root() {
            visit(self, root);
        }
        for index in 0..self.viewport.children.len() {
            visit(self, self.viewport.children[index]);
        }
    }
}
