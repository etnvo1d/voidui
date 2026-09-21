//! Sticky offsets are visual translations of normal-flow layout. Taffy retains
//! the original flow slot, so siblings never reflow when an element sticks.
use super::{
    geometry::{Point, Rect},
    layout::{ExpandedLengthPercentageAuto, LengthPercentageAuto},
    widget::WidgetId,
    widget_tree::WidgetTree,
};
use crate::style::layer::Position;
impl WidgetTree {
    pub(crate) fn apply_sticky(&mut self, id: WidgetId) {
        if self.nodes[id].computed.layer.position != Position::Sticky {
            return;
        }
        let parent = self.nodes[id].layout_parent;
        let mut ancestor = parent;
        let mut port = None;
        while let Some(p) = ancestor {
            let n = &self.nodes[p];
            if n.computed.scroll.overflow.x.is_scroll_container()
                || n.computed.scroll.overflow.y.is_scroll_container()
            {
                port = Some(self.scrollport(p));
                break;
            }
            ancestor = n.layout_parent;
        }
        let port = port.unwrap_or_else(|| {
            Rect::from_xywh(
                self.viewport.origin.x,
                self.viewport.origin.y,
                self.viewport.layout.size.width,
                self.viewport.layout.size.height,
            )
        });
        let mut containing = parent.map_or(port, |p| self.content_bounds(p));
        let original_width = containing.size.width;
        if let Some((parent, metrics)) = parent.and_then(|p| self.scroll_metrics(p).map(|m| (p, m)))
        {
            containing.size.width = containing.size.width.max(
                metrics.content.width
                    - self.nodes[parent].layout.padding.left
                    - self.nodes[parent].layout.padding.right,
            );
            containing.size.height = containing.size.height.max(
                metrics.content.height
                    - self.nodes[parent].layout.padding.top
                    - self.nodes[parent].layout.padding.bottom,
            );
        }
        if parent.is_some_and(|p| {
            self.nodes[p].computed.layout.direction == super::layout::Direction::Rtl
        }) {
            containing.origin.x -= containing.size.width - original_width;
        }
        let scroll = parent.map_or(Point::default(), |p| self.scroll_offset(p));
        containing.origin.x -= scroll.x;
        containing.origin.y -= scroll.y;
        let node = &self.nodes[id];
        let b = node.global_bounds;
        let margin = node.layout.margin;
        let Some(inset) = node.computed.sticky_inset.as_ref() else {
            return;
        };
        let resolve = |v: LengthPercentageAuto, basis: f32| match v.expand() {
            ExpandedLengthPercentageAuto::Length(v) => Some(v),
            ExpandedLengthPercentageAuto::Percent(v) => Some(v * basis),
            ExpandedLengthPercentageAuto::Calc(v) => {
                Some(crate::core::layout::resolve_calc(v, basis))
            }
            ExpandedLengthPercentageAuto::Auto => None,
        };
        let x = stick_axis(
            b.origin.x,
            b.size.width,
            port.origin.x,
            port.size.width,
            containing.origin.x,
            containing.size.width,
            margin.left,
            margin.right,
            resolve(inset.left, port.size.width),
            resolve(inset.right, port.size.width),
            node.computed.layout.direction == super::layout::Direction::Rtl,
        );
        let y = stick_axis(
            b.origin.y,
            b.size.height,
            port.origin.y,
            port.size.height,
            containing.origin.y,
            containing.size.height,
            margin.top,
            margin.bottom,
            resolve(inset.top, port.size.height),
            resolve(inset.bottom, port.size.height),
            false,
        );
        self.nodes[id].global_bounds.origin = Point::new(x, y);
    }
}
/// Reduce the end inset when the sticky view rectangle is smaller than the box.
/// Auto edges contribute zero to that rectangle but never request a displacement.
#[allow(clippy::too_many_arguments)]
fn stick_axis(
    pos: f32,
    size: f32,
    view: f32,
    extent: f32,
    cb: f32,
    cb_size: f32,
    margin_start: f32,
    margin_end: f32,
    start: Option<f32>,
    end: Option<f32>,
    reverse: bool,
) -> f32 {
    if start.is_none() && end.is_none() {
        return pos;
    }
    let mut low = start.unwrap_or(0.);
    let mut high = end.unwrap_or(0.);
    let deficit = (size - (extent - low - high)).max(0.);
    if reverse {
        low -= deficit;
    } else {
        high -= deficit;
    }
    let mut target = pos;
    if reverse {
        if start.is_some() {
            target = target.max(view + low);
        }
        if end.is_some() {
            target = target.min(view + extent - high - size);
        }
    } else {
        if end.is_some() {
            target = target.min(view + extent - high - size);
        }
        if start.is_some() {
            target = target.max(view + low);
        }
    }
    // CSS position-box margins are capped by the available distance to the
    // containing block; an already overflowing normal box is not pulled inward.
    let min = cb + margin_start.min(pos - cb);
    let max = cb + cb_size - size - margin_end.min(cb + cb_size - pos - size);
    target.max(min).min(max.max(min))
}
