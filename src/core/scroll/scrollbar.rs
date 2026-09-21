//! Scrollbar geometry is shared by hit testing and painting without widget allocation.
use super::*;

impl WidgetTree {
    /// Track and thumb bounds are shared by drawing and input; no extra widgets.
    pub fn scrollbar_geometry(&self, id: WidgetId, axis: ScrollAxis) -> Option<ScrollbarGeometry> {
        let node = self.nodes.get(id)?;
        let s = node.scroll.as_ref()?;
        let width = self.scroll_options.thickness(node.computed.scroll.width);
        if width == 0.0
            || !match axis {
                ScrollAxis::Horizontal => s.bars.x,
                ScrollAxis::Vertical => s.bars.y,
            }
        {
            return None;
        }
        let b = node.global_bounds;
        let l = &node.layout;
        let inner = Rect::from_xywh(
            b.origin.x + l.border.left,
            b.origin.y + l.border.top,
            (b.size.width - l.border.left - l.border.right).max(0.0),
            (b.size.height - l.border.top - l.border.bottom).max(0.0),
        );
        let (track, viewport, content, position) = match axis {
            ScrollAxis::Horizontal => (
                Rect::from_xywh(
                    inner.origin.x + s.mirror_gutter,
                    inner.origin.y + (inner.size.height - width).max(0.0),
                    (inner.size.width
                        - s.mirror_gutter
                        - if s.bars.y {
                            width
                        } else {
                            l.scrollbar_size.width - s.mirror_gutter
                        })
                    .max(0.0),
                    width.min(inner.size.height),
                ),
                s.metrics.viewport.width,
                s.metrics.content.width,
                s.metrics.offset.x - s.metrics.min.x,
            ),
            ScrollAxis::Vertical => (
                Rect::from_xywh(
                    inner.origin.x + (inner.size.width - width).max(0.0),
                    inner.origin.y,
                    width.min(inner.size.width),
                    (inner.size.height
                        - if s.bars.x {
                            width
                        } else {
                            l.scrollbar_size.height
                        })
                    .max(0.0),
                ),
                s.metrics.viewport.height,
                s.metrics.content.height,
                s.metrics.offset.y,
            ),
        };
        let length = axis_length(track, axis);
        let thumb_length = (length
            * if content > 0.0 {
                viewport / content
            } else {
                1.0
            })
        .max(self.scroll_options.min_thumb_length)
        .min(length);
        let start = if content > viewport {
            (length - thumb_length) * position / (content - viewport)
        } else {
            0.0
        };
        let mut thumb = track;
        match axis {
            ScrollAxis::Horizontal => {
                thumb.origin.x += start;
                thumb.size.width = thumb_length;
            }
            ScrollAxis::Vertical => {
                thumb.origin.y += start;
                thumb.size.height = thumb_length;
            }
        }
        Some(ScrollbarGeometry { axis, track, thumb })
    }
    pub(crate) fn paint_scrollbars(&self, id: WidgetId, painter: &mut crate::render::Painter<'_>) {
        let colors = self.nodes[id]
            .computed
            .scroll
            .colors
            .as_deref()
            .copied()
            .unwrap_or(self.scroll_options.colors);
        for axis in [ScrollAxis::Horizontal, ScrollAxis::Vertical] {
            if let Some(g) = self.scrollbar_geometry(id, axis) {
                paint_rect(
                    painter,
                    g.track,
                    colors.track.resolve(self.nodes[id].computed.text.color),
                );
                paint_rect(
                    painter,
                    g.thumb,
                    colors.thumb.resolve(self.nodes[id].computed.text.color),
                );
            }
        }
    }
}
fn paint_rect(painter: &mut crate::render::Painter<'_>, r: Rect<f32>, color: Color) {
    use crate::render::{Bounds, fill, point, px, size};
    painter.paint_quad(fill(
        Bounds::new(
            point(px(r.origin.x), px(r.origin.y)),
            size(px(r.size.width), px(r.size.height)),
        ),
        crate::render::Hsla::from(color),
    ));
}
