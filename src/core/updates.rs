//! Coalesced invalidation for retained custom widgets. Handles are weak and stale
//! node IDs are ignored, so a model can safely outlive its mounted views.
use super::{widget::WidgetId, widget_tree::WidgetTree};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::{Rc, Weak},
};

#[derive(Debug, Clone, Copy, Default)]
pub struct Invalidation {
    pub layout: bool,
    pub style: bool,
}
#[derive(Default)]
pub(crate) struct WidgetQueue {
    pending: RefCell<HashMap<WidgetId, Invalidation>>,
    wake: RefCell<Option<Rc<dyn Fn()>>>,
    pub paint_dirty: Cell<bool>,
}
impl WidgetQueue {
    /// Paint-only geometry updates coalesce without allocating a pending-node entry.
    pub(crate) fn repaint(&self) {
        if !self.paint_dirty.replace(true)
            && let Some(wake) = self.wake.borrow().as_ref()
        {
            wake();
        }
    }
}
#[derive(Clone)]
pub struct WidgetInvalidator {
    queue: Weak<WidgetQueue>,
    id: WidgetId,
}
impl WidgetInvalidator {
    pub fn repaint(&self) {
        self.invalidate(Invalidation::default());
    }
    pub fn relayout(&self) {
        self.invalidate(Invalidation {
            layout: true,
            style: false,
        });
    }
    pub fn restyle(&self) {
        self.invalidate(Invalidation {
            layout: false,
            style: true,
        });
    }
    pub fn invalidate(&self, value: Invalidation) {
        let Some(queue) = self.queue.upgrade() else {
            return;
        };
        let wake = {
            let mut pending = queue.pending.borrow_mut();
            let wake = pending.is_empty();
            let entry = pending.entry(self.id).or_default();
            entry.layout |= value.layout;
            entry.style |= value.style;
            wake
        };
        if wake {
            if let Some(wake) = queue.wake.borrow().clone() {
                wake();
            }
        }
    }
}
impl WidgetTree {
    pub fn invalidator(&self, id: WidgetId) -> WidgetInvalidator {
        WidgetInvalidator {
            queue: Rc::downgrade(&self.widget_updates),
            id,
        }
    }
    pub(crate) fn has_widget_updates(&self) -> bool {
        !self.widget_updates.pending.borrow().is_empty()
    }
    pub(crate) fn flush_widget_updates(&mut self) {
        if !self.has_widget_updates() {
            return;
        }
        let mut pending = std::mem::take(&mut *self.widget_updates.pending.borrow_mut());
        for (id, change) in pending.drain() {
            if !self.nodes.contains_key(id) {
                continue;
            }
            let model_change = self.nodes[id].widget.as_mut().unwrap().update_model();
            self.widget_updates.paint_dirty.set(true);
            self.css_dirty |= change.style || model_change.style;
            if change.layout || model_change.layout {
                self.invalidate_layout(id);
            }
        }
        let mut queued = self.widget_updates.pending.borrow_mut();
        if queued.is_empty() {
            *queued = pending;
        }
    }
    pub(crate) fn attach_widget(&mut self, id: WidgetId) {
        let invalidator = self.invalidator(id);
        let node = &mut self.nodes[id];
        let widget = node.widget.as_mut().unwrap();
        // Aggregate local SVG state selectors once on attachment. Pointer motion
        // then checks a bitmask instead of walking every widget's SVG stylesheet.
        for sheet in widget.svg_stylesheets() {
            self.svg_css_state |= sheet.state_mask();
        }
        widget.attach(invalidator);
        if let Some(input) = widget.text_input_mut() {
            input.attributes_changed(&node.props.attributes);
        }
    }
    pub(crate) fn set_widget_waker(&mut self, wake: Rc<dyn Fn()>) {
        *self.widget_updates.wake.borrow_mut() = Some(wake);
    }
}
