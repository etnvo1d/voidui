//! Used scrollport geometry and a convergent, layout-only scrollbar gutter adapter.
use super::*;
use taffy::Dimension;

impl WidgetTree {
    /// Refresh retained input extents after model/style changes. Native windows
    /// call this before painting; headless hosts can do the same without reflow.
    pub fn refresh_scroll_content(&mut self, cache: &crate::render::TextLayoutCache) {
        for index in 0..self.scrollers.len() {
            let id = self.scrollers[index];
            if self.nodes.contains_key(id) {
                self.refresh_input_scroll(id, cache);
            }
        }
    }
    pub(crate) fn refresh_input_scroll(
        &mut self,
        id: WidgetId,
        cache: &crate::render::TextLayoutCache,
    ) {
        let Some(node) = self.nodes.get(id) else {
            return;
        };
        if node.scroll.is_none() {
            return;
        }
        let bounds = self.content_bounds(id);
        let classic = self.scrollbar_mode(id) == ScrollbarMode::Classic;
        let node = &mut self.nodes[id];
        let Some(content) = node
            .widget
            .as_mut()
            .and_then(|w| w.text_input_mut())
            .and_then(|w| w.scroll_content(bounds, cache))
        else {
            return;
        };
        let previous = node.layout.scrollable_overflow_rect;
        node.layout.scrollable_overflow_rect = layout::Rect {
            left: 0.0,
            top: 0.0,
            right: content.size.width + node.layout.padding.left + node.layout.padding.right,
            bottom: content.size.height + node.layout.padding.top + node.layout.padding.bottom,
        };
        node.scroll.as_mut().unwrap().metrics.offset = content.offset;
        if self.layout_ready {
            if classic && previous != node.layout.scrollable_overflow_rect {
                self.invalidate_layout(id);
            }
            self.update_scroll_metrics(id);
        }
    }
    pub(crate) fn used_layout_style(&self, id: WidgetId) -> &LayoutStyle {
        let node = &self.nodes[id];
        node.scroll
            .as_ref()
            .map(|s| &s.layout)
            .unwrap_or(&node.computed.layout)
    }
    pub(crate) fn prepare_scrolling(&mut self) {
        self.scrollers.clear();
        let mut changed = Vec::new();
        for (id, node) in &mut self.nodes {
            let previous = node
                .scroll
                .as_ref()
                .map(|s| (s.layout.overflow, s.layout.scrollbar_width, s.mirror_gutter));
            let overflow = node.computed.scroll.overflow;
            if !node.rendered
                || !(overflow.x.is_scroll_container() || overflow.y.is_scroll_container())
            {
                self.paint_order_dirty
                    .set(self.paint_order_dirty.get() || node.scroll.is_some());
                node.scroll = None;
                if previous.is_some() {
                    changed.push(id);
                }
                continue;
            }
            self.scrollers.push(id);
            if node.scroll.is_none() {
                self.paint_order_dirty.set(true);
            }
            let s = node.scroll.get_or_insert_with(Default::default);
            s.layout.clone_from(&node.computed.layout);
            let width = self.scroll_options.thickness(node.computed.scroll.width);
            let classic = node
                .props
                .scrollbar_mode
                .unwrap_or(self.scroll_options.mode)
                == ScrollbarMode::Classic;
            s.layout.scrollbar_width = if classic { width } else { 0.0 };
            // Start auto without gutters every reflow. Otherwise a former scrollbar
            // can keep its own overflow alive after content shrinks.
            s.reserved = Point::new(
                classic && width > 0.0 && overflow.x == Overflow::Scroll,
                classic
                    && width > 0.0
                    && (overflow.y == Overflow::Scroll
                        || (node.computed.scroll.gutter != ScrollbarGutter::Auto
                            && overflow.y.is_scroll_container())),
            );
            s.mirror_gutter = if s.reserved.y
                && node.computed.scroll.gutter == ScrollbarGutter::StableBothEdges
            {
                width
            } else {
                0.0
            };
            s.layout.overflow = layout::Point {
                x: overflow.x.layout(s.reserved.x),
                y: overflow.y.layout(s.reserved.y),
            };
            if previous != Some((s.layout.overflow, s.layout.scrollbar_width, s.mirror_gutter)) {
                changed.push(id);
            }
        }
        for id in changed {
            self.invalidate_layout(id);
        }
    }
    /// Monotonic gutter discovery converges after at most two additions per box.
    /// Overlay scrollbars never request another layout pass.
    pub(crate) fn resolve_scroll_gutters(&mut self) -> bool {
        let mut changed = Vec::new();
        for &id in &self.scrollers {
            let classic = self.scrollbar_mode(id) == ScrollbarMode::Classic;
            let node = &mut self.nodes[id];
            let overflow = node.computed.scroll.overflow;
            let s = node.scroll.as_mut().unwrap();
            if !classic || s.layout.scrollbar_width == 0.0 {
                continue;
            }
            let needed = Point::new(
                node.layout.scroll_width() > 0.0,
                node.layout.scroll_height() > 0.0,
            );
            let previous = s.reserved;
            if overflow.x == Overflow::Auto && needed.x && !s.reserved.x {
                s.reserved.x = true;
                s.layout.overflow.x = layout::Overflow::Scroll;
            }
            if overflow.y == Overflow::Auto && needed.y && !s.reserved.y {
                s.reserved.y = true;
                s.layout.overflow.y = layout::Overflow::Scroll;
            }
            if previous != s.reserved {
                changed.push(id);
            }
        }
        let any = !changed.is_empty();
        for id in changed {
            self.invalidate_layout(id);
        }
        any
    }
    pub(crate) fn finish_scrolling(&mut self) {
        for index in 0..self.scrollers.len() {
            let id = self.scrollers[index];
            self.update_scroll_metrics(id);
        }
    }
    fn update_scroll_metrics(&mut self, id: WidgetId) {
        let node = &mut self.nodes[id];
        let s = node.scroll.as_mut().unwrap();
        let overflow = node.computed.scroll.overflow;
        let width = if overflow.x.is_scroll_container() {
            node.layout.scroll_width()
        } else {
            0.0
        };
        let height = if overflow.y.is_scroll_container() {
            node.layout.scroll_height()
        } else {
            0.0
        };
        let rtl = node.computed.layout.direction == layout::Direction::Rtl;
        s.metrics.min = Point::new(if rtl { -width } else { 0.0 }, 0.0);
        s.metrics.max = Point::new(if rtl { 0.0 } else { width }, height);
        s.metrics.offset = clamp(s.metrics.offset, s.metrics.min, s.metrics.max);
        let l = &node.layout;
        s.metrics.viewport = Size::new(
            (l.size.width - l.border.left - l.border.right - l.scrollbar_size.width).max(0.0),
            (l.size.height - l.border.top - l.border.bottom - l.scrollbar_size.height).max(0.0),
        );
        s.metrics.content = Size::new(
            s.metrics.viewport.width + width,
            s.metrics.viewport.height + height,
        );
        s.bars = Point::new(
            overflow.x == Overflow::Scroll || (overflow.x == Overflow::Auto && width > 0.0),
            overflow.y == Overflow::Scroll || (overflow.y == Overflow::Auto && height > 0.0),
        );
        if let Some(input) = node.widget.as_mut().and_then(|w| w.text_input_mut()) {
            input.set_scroll_offset(s.metrics.offset);
        }
    }
}
/// Taffy has one trailing gutter per axis. Model the optional mirrored inline
/// gutter as an inset viewport, retaining the original outer dimensions:
///
/// outer border box
/// | mirrored gutter | Taffy viewport | trailing gutter |
///
/// The parent always sees the outer style/output. Only this widget's layout call
/// receives inset dimensions. Percentages resolve against the original containing
/// block before the inset is applied, so no synthetic CSS padding is introduced.
pub(crate) fn inset_layout_width(
    style: &mut LayoutStyle,
    input: &mut layout::LayoutInput,
    inset: f32,
) {
    use layout::{ExpandedDimension, ExpandedLengthPercentageAuto, LengthPercentageAuto};
    let context = input.parent_size.width;
    let dim = |v: Dimension| match v.expand() {
        ExpandedDimension::Length(n) => Dimension::length((n - inset).max(0.0)),
        ExpandedDimension::Percent(n) if context.is_some() => {
            Dimension::length((n * context.unwrap() - inset).max(0.0))
        }
        ExpandedDimension::Calc(handle) if context.is_some() => {
            Dimension::length((layout::resolve_calc(handle, context.unwrap()) - inset).max(0.0))
        }
        _ => v,
    };
    let limit = |v: LengthPercentageAuto| match v.expand() {
        ExpandedLengthPercentageAuto::Length(n) => {
            LengthPercentageAuto::length((n - inset).max(0.0))
        }
        ExpandedLengthPercentageAuto::Percent(n) if context.is_some() => {
            LengthPercentageAuto::length((n * context.unwrap() - inset).max(0.0))
        }
        ExpandedLengthPercentageAuto::Calc(handle) if context.is_some() => {
            LengthPercentageAuto::length(
                (layout::resolve_calc(handle, context.unwrap()) - inset).max(0.0),
            )
        }
        _ => v,
    };
    style.size.width = dim(style.size.width);
    style.min_size.width = limit(style.min_size.width);
    style.max_size.width = limit(style.max_size.width);
    input.known_dimensions.width = input.known_dimensions.width.map(|v| (v - inset).max(0.0));
    if let layout::AvailableSpace::Definite(v) = &mut input.available_space.width {
        *v = (*v - inset).max(0.0);
    }
}
