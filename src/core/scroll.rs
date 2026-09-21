//! Retained scrolling shared by containers and leaf widgets. Scroll changes are
//! geometry/paint-only: no component reconciliation, text shaping or Taffy layout.
#![doc = include_str!("../../docs/scrolling.md")]

mod interaction;
mod layout_adapter;
mod scrollbar;
mod transform;
pub(crate) use layout_adapter::inset_layout_width;

use super::{
    geometry::{Point, Rect, Size},
    layout::{self, LayoutStyle},
    widget::WidgetId,
    widget_tree::WidgetTree,
};
use crate::style::{
    color::{Color, Rgba8},
    scroll::{Overflow, OverscrollBehavior, ScrollbarColors, ScrollbarGutter, ScrollbarWidth},
};

/// Scrollbar placement is a host preference, not a CSS property.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollbarMode {
    /// Paint above content without reducing the viewport.
    #[default]
    Overlay,
    /// Reserve layout space for visible scrollbars and stable gutters.
    Classic,
}
/// Application-wide metrics in logical pixels. No OS timer or polling is needed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollOptions {
    pub mode: ScrollbarMode,
    pub width: f32,
    pub thin_width: f32,
    pub min_thumb_length: f32,
    pub colors: ScrollbarColors,
}
impl Default for ScrollOptions {
    fn default() -> Self {
        Self {
            mode: ScrollbarMode::Overlay,
            width: 12.0,
            thin_width: 6.0,
            min_thumb_length: 24.0,
            colors: ScrollbarColors {
                thumb: Rgba8::new(110, 110, 110, 180).into(),
                track: Rgba8::new(128, 128, 128, 35).into(),
            },
        }
    }
}
impl ScrollOptions {
    fn validate(&self) {
        assert!(
            [self.width, self.thin_width, self.min_thumb_length]
                .iter()
                .all(|v| v.is_finite() && *v > 0.0),
            "scrollbar metrics must be finite and positive"
        );
    }
    fn thickness(self, width: ScrollbarWidth) -> f32 {
        match width {
            ScrollbarWidth::Auto => self.width,
            ScrollbarWidth::Thin => self.thin_width,
            ScrollbarWidth::None => 0.0,
        }
    }
}
/// Retained content reported by controls that manage their own caret-following view.
#[derive(Debug, Clone, Copy)]
pub struct ScrollContent {
    pub offset: Point<f32>,
    pub size: Size<f32>,
}
/// CSSOM-style logical coordinates. RTL horizontal offsets range from min.x to 0.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ScrollMetrics {
    pub offset: Point<f32>,
    pub min: Point<f32>,
    pub max: Point<f32>,
    pub viewport: Size<f32>,
    pub content: Size<f32>,
}
#[derive(Default)]
pub(crate) struct ScrollState {
    pub metrics: ScrollMetrics,
    pub reserved: Point<bool>,
    pub bars: Point<bool>,
    // Only scroll containers need a used layout style. CSS computed values never
    // change when an auto scrollbar appears, so cascade/inheritance remain stable.
    pub layout: LayoutStyle,
    pub mirror_gutter: f32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollAxis {
    Horizontal,
    Vertical,
}
#[derive(Debug, Clone, Copy)]
pub struct ScrollbarGeometry {
    pub axis: ScrollAxis,
    pub track: Rect<f32>,
    pub thumb: Rect<f32>,
}
#[derive(Debug, Clone, Copy)]
pub(crate) struct ScrollDrag {
    pub owner: WidgetId,
    pub axis: ScrollAxis,
    pub grab: Option<f32>,
}

impl WidgetTree {
    /// Set host metrics and placement. Only changes to gutter dimensions reflow;
    /// palette and minimum-thumb updates are paint-only.
    pub fn set_scroll_options(&mut self, options: ScrollOptions) {
        options.validate();
        if self.scroll_options != options {
            if self.scroll_options.mode != options.mode
                || self.scroll_options.width != options.width
                || self.scroll_options.thin_width != options.thin_width
            {
                self.invalidate_all_layouts();
            }
            self.scroll_options = options;
            self.widget_updates.repaint();
        }
    }
    /// Change one element's placement policy. None restores the host default.
    pub fn set_scrollbar_mode(&mut self, id: WidgetId, mode: Option<ScrollbarMode>) -> bool {
        let Some(node) = self.nodes.get_mut(id) else {
            return false;
        };
        if node.props.scrollbar_mode == mode {
            return false;
        }
        node.props.scrollbar_mode = mode;
        self.invalidate_layout(id);
        self.widget_updates.repaint();
        true
    }
    pub fn scroll_options(&self) -> ScrollOptions {
        self.scroll_options
    }

    /// Read offsets, reachable ranges and viewport/content sizes after layout.
    pub fn scroll_metrics(&self, id: WidgetId) -> Option<ScrollMetrics> {
        self.nodes.get(id)?.scroll.as_ref().map(|s| s.metrics)
    }
    pub fn scroll_style(&self, id: WidgetId) -> &crate::style::scroll::ScrollStyle {
        &self.nodes[id].computed.scroll
    }

    fn scrollbar_mode(&self, id: WidgetId) -> ScrollbarMode {
        self.nodes[id]
            .props
            .scrollbar_mode
            .unwrap_or(self.scroll_options.mode)
    }

    /// Scroll without relayout. Hidden permits programmatic scrolling; clip does not.
    /// Non-finite input and stale IDs are ignored. Returns true only for a change.
    pub fn scroll_to(&mut self, id: WidgetId, offset: Point<f32>) -> bool {
        if !self.layout_ready || !offset.x.is_finite() || !offset.y.is_finite() {
            return false;
        }
        let Some(s) = self.nodes.get_mut(id).and_then(|n| n.scroll.as_mut()) else {
            return false;
        };
        let next = clamp(offset, s.metrics.min, s.metrics.max);
        if s.metrics.offset == next {
            return false;
        }
        s.metrics.offset = next;
        if let Some(input) = self.nodes[id]
            .widget
            .as_mut()
            .and_then(|w| w.text_input_mut())
        {
            input.set_scroll_offset(next);
        }
        self.reposition_scrolled_children(id);
        self.paint_clip_dirty.set(true);
        self.widget_updates.repaint();
        true
    }
    pub fn scroll_by(&mut self, id: WidgetId, delta: Point<f32>) -> bool {
        let Some(m) = self.scroll_metrics(id) else {
            return false;
        };
        self.scroll_to(id, Point::new(m.offset.x + delta.x, m.offset.y + delta.y))
    }
    pub(crate) fn scroll_offset(&self, id: WidgetId) -> Point<f32> {
        self.nodes[id]
            .scroll
            .as_ref()
            .map(|s| s.metrics.offset)
            .unwrap_or_default()
    }
    /// The padding-box viewport excludes classic scrollbar gutters.
    pub(crate) fn scrollport(&self, id: WidgetId) -> Rect<f32> {
        let node = &self.nodes[id];
        let b = node.global_bounds;
        let l = &node.layout;
        Rect::from_xywh(
            b.origin.x + l.border.left + self.mirror_gutter(id),
            b.origin.y + l.border.top,
            (b.size.width - l.border.left - l.border.right - l.scrollbar_size.width).max(0.0),
            (b.size.height - l.border.top - l.border.bottom - l.scrollbar_size.height).max(0.0),
        )
    }
    pub(crate) fn mirror_gutter(&self, id: WidgetId) -> f32 {
        self.nodes[id]
            .scroll
            .as_ref()
            .map_or(0.0, |s| s.mirror_gutter)
    }
    pub(crate) fn scrolled_content_bounds(&self, id: WidgetId) -> Rect<f32> {
        let mut b = self.content_bounds(id);
        // Editable controls retain their own caret-following offset and expose it
        // through the scroll protocol; translating their paint again would double it.
        if self.text_input(id).is_some() {
            return b;
        }
        let offset = self.scroll_offset(id);
        b.origin.x -= offset.x;
        b.origin.y -= offset.y;
        if self.nodes[id].scroll.is_some()
            && let Some(text) = self.nodes[id]
                .widget
                .as_ref()
                .and_then(|w| w.prepared_text())
        {
            let size = text.measurement().size;
            b.size.width = b.size.width.max(size.width);
            b.size.height = b.size.height.max(size.height);
        }
        b
    }
}
impl<W> super::widget::WidgetBuilder<W> {
    /// Override host scrollbar placement for this element only; this is not CSS.
    pub fn scrollbar_mode(mut self, mode: ScrollbarMode) -> Self {
        self.props.scrollbar_mode = Some(mode);
        self
    }
}
fn clamp(p: Point<f32>, min: Point<f32>, max: Point<f32>) -> Point<f32> {
    Point::new(p.x.clamp(min.x, max.x), p.y.clamp(min.y, max.y))
}
pub(crate) fn contains(r: Rect<f32>, p: Point<f32>) -> bool {
    p.x >= r.origin.x
        && p.y >= r.origin.y
        && p.x < r.origin.x + r.size.width
        && p.y < r.origin.y + r.size.height
}
fn axis_point(p: Point<f32>, axis: ScrollAxis) -> f32 {
    match axis {
        ScrollAxis::Horizontal => p.x,
        ScrollAxis::Vertical => p.y,
    }
}
fn axis_length(r: Rect<f32>, axis: ScrollAxis) -> f32 {
    match axis {
        ScrollAxis::Horizontal => r.size.width,
        ScrollAxis::Vertical => r.size.height,
    }
}
