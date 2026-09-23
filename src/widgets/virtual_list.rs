//! Fixed-height virtualization with stable keyed rows and pinned interaction owners.
use crate::core::{
    context::LayoutContext,
    geometry::{Point, Rect},
    layout::{LayoutInput, LayoutOutput},
    updates::WidgetInvalidator,
    widget::{Widget, WidgetBuilder, WidgetUpdate},
};
use crate::{Element, IntoElement, State, component, div, state};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
#[derive(Clone, Debug)]
pub struct VirtualListOptions {
    pub row_height: f32,
    pub overscan_rows: usize,
    /// Keep editors or captured rows mounted even when outside the viewport.
    pub pinned: Vec<usize>,
}
impl Default for VirtualListOptions {
    fn default() -> Self {
        Self {
            row_height: 30.0,
            overscan_rows: 8,
            pinned: Vec::new(),
        }
    }
}
impl VirtualListOptions {
    pub fn visible_range(&self, count: usize, offset: f32, height: f32) -> std::ops::Range<usize> {
        assert!(
            self.row_height.is_finite() && self.row_height > 0.,
            "row height must be positive"
        );
        let first = (offset.max(0.) / self.row_height).floor() as usize;
        let last = ((offset.max(0.) + height.max(0.)) / self.row_height).ceil() as usize;
        first.saturating_sub(self.overscan_rows).min(count)
            ..last.saturating_add(self.overscan_rows).min(count)
    }
}
/// Render only rows intersecting the scrollport. Each row must occupy exactly
/// row_height logical pixels including margins; supply stable keys in render().
pub fn virtual_list(
    count: usize,
    options: VirtualListOptions,
    render: impl Fn(usize) -> Element + 'static,
) -> crate::core::component::ComponentElement {
    let render = Rc::new(render);
    component(move || {
        let range = state(|| options.visible_range(count, 0., 0.));
        let active = range.get();
        let mut indices: Vec<_> = active
            .filter(|i| *i < count)
            .chain(options.pinned.iter().copied().filter(|i| *i < count))
            .collect();
        indices.sort_unstable();
        indices.dedup();
        let mut children = Vec::new();
        let mut cursor = 0;
        for i in indices {
            if i > cursor {
                children.push(
                    div()
                        .height((i - cursor) as f32 * options.row_height)
                        .flex_shrink(0.)
                        .key(format!("gap-{cursor}"))
                        .into_element(),
                );
            }
            children.push(render(i));
            cursor = i + 1;
        }
        if cursor < count {
            children.push(
                div()
                    .height((count - cursor) as f32 * options.row_height)
                    .flex_shrink(0.)
                    .key(format!("gap-{cursor}"))
                    .into_element(),
            );
        }
        let mut builder = WidgetBuilder::from_widget(Viewport {
            count,
            options: options.clone(),
            range,
            scope: None,
            pending: Rc::new(RefCell::new(None)),
            queued: Rc::new(Cell::new(false)),
        })
        .flex()
        .flex_col()
        .flex_grow(1.)
        .width(crate::style::pct(100.))
        .height(crate::style::pct(100.))
        .min_height(0)
        .overflow_y(crate::Overflow::Auto);
        builder.children = children;
        builder
    })
}
struct Viewport {
    count: usize,
    options: VirtualListOptions,
    range: State<std::ops::Range<usize>>,
    scope: Option<crate::OwnedTaskScope>,
    pending: Rc<RefCell<Option<std::ops::Range<usize>>>>,
    queued: Rc<Cell<bool>>,
}
impl Widget for Viewport {
    fn paints_content(&self) -> bool {
        false
    }
    fn attach(&mut self, invalidator: WidgetInvalidator) {
        if self.scope.is_none() {
            self.scope = invalidator.task_runtime().map(|r| r.scope());
        }
    }
    fn reconcile(&mut self, next: &dyn Widget) -> WidgetUpdate {
        let Some(next) = (next as &dyn std::any::Any).downcast_ref::<Self>() else {
            return WidgetUpdate::Replace;
        };
        self.count = next.count;
        self.options = next.options.clone();
        self.range = next.range;
        WidgetUpdate::Changed
    }
    fn layout(&mut self, input: LayoutInput, cx: LayoutContext<'_, '_>) -> LayoutOutput {
        cx.layout_children(input)
    }
    fn viewport_changed(&mut self, bounds: Rect<f32>, offset: Point<f32>) {
        let next = self
            .options
            .visible_range(self.count, offset.y, bounds.size.height);
        if self.range.with_untracked(|range| *range == next) {
            return;
        }
        *self.pending.borrow_mut() = Some(next);
        if self.queued.replace(true) {
            return;
        }
        if let Some(scope) = &self.scope {
            let (pending, queued, range) = (self.pending.clone(), self.queued.clone(), self.range);
            // Publish after layout has finished; setting component state during
            // a draw would invalidate the tree that is still being traversed.
            scope.spawn(async move {
                queued.set(false);
                if let Some(next) = pending.borrow_mut().take() {
                    if range.is_mounted() {
                        range.set_if_changed(next);
                    }
                }
            });
        } else {
            self.queued.set(false);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn visible_work_is_independent_of_document_size() {
        let options = VirtualListOptions {
            row_height: 20.,
            overscan_rows: 2,
            pinned: vec![],
        };
        assert_eq!(options.visible_range(100_000, 1000., 200.), 48..62);
        assert_eq!(options.visible_range(3, 1000., 200.), 3..3);
    }
}
