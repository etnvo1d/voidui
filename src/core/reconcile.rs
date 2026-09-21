//! Component boundaries live in a sparse side table, not in the layout/CSS tree.
use super::{
    component::ComponentElement,
    element::{Element, ElementKind, IntoElement},
    state::{self, Hooks, Signal, UpdateQueue},
    widget::WidgetId,
    widget_tree::WidgetTree,
};
use slotmap::{SlotMap, new_key_type};
use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
};
use winit::keyboard::SmolStr;
new_key_type! { pub(crate) struct ComponentId; }
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mounted {
    Widget(WidgetId),
    Component(ComponentId),
}
struct Instance {
    description: Option<Box<ComponentElement>>,
    hooks: Hooks,
    context: Rc<super::environment::ContextScope>,
    signal: Rc<Signal>,
    output: Option<Mounted>,
    root: Option<WidgetId>,
    parent: Option<ComponentId>,
    depth: usize,
}
impl Drop for Instance {
    fn drop(&mut self) {
        self.signal.mounted.set(false);
    }
}
#[derive(Default)]
pub(crate) struct Components {
    instances: SlotMap<ComponentId, Instance>,
    // Only component roots have entries. Ordinary widgets pay no per-node cost.
    roots: HashMap<WidgetId, ComponentId>,
    pub queue: Rc<UpdateQueue>,
}
impl Components {
    pub fn reserve(&mut self, additional: usize) {
        self.instances.reserve(additional);
        self.roots.reserve(additional);
    }

    pub fn clear(&mut self) {
        self.instances.clear();
        self.roots.clear();
        self.queue.pending.borrow_mut().clear();
        self.queue.contexts.borrow_mut().clear();
    }
}

impl WidgetTree {
    /// True when state readers are queued. A clean check is constant-time.
    pub fn has_pending_updates(&self) -> bool {
        !self.components.queue.pending.borrow().is_empty()
            || !self.components.queue.contexts.borrow().is_empty()
            || self.has_widget_updates()
    }

    /// Install a main-thread wakeup for a headless host. Native windows install
    /// their own redraw wakeup. One batch invokes it at most once; do not reenter
    /// the tree or write state from this callback.
    pub fn set_update_waker(&mut self, wake: impl Fn() + 'static) {
        let wake = Rc::new(wake);
        self.set_widget_waker(wake.clone());
        self.window_host.set_waker(wake.clone());
        *self.components.queue.wake.borrow_mut() = Some(Box::new(move || wake()));
    }

    /// Apply one batch before layout. Readers are processed parent-first; a child
    /// already rendered by its parent is skipped. Unrelated branches are untouched.
    /// Returns the number of component bodies executed, including descendants.
    pub fn flush_updates(&mut self) -> usize {
        if !self.has_pending_updates() {
            return 0;
        }
        self.flush_widget_updates();
        let _task_update = self.tasks.update();
        let mut pending = std::mem::take(&mut *self.components.queue.pending.borrow_mut());
        let before = self.component_renders;
        loop {
            pending.extend(self.components.queue.contexts.borrow_mut().drain(..));
            pending.sort_unstable_by_key(|id| {
                self.components
                    .instances
                    .get(*id)
                    .map(|i| i.depth)
                    .unwrap_or(usize::MAX)
            });
            for id in pending.drain(..) {
                if let Some(instance) = self.components.instances.get(id)
                    && instance.signal.dirty.get()
                    && let Some(root) = instance.root
                {
                    let parent = self.nodes[root].parent;
                    let previous = self.nodes[root].previous_sibling;
                    let new_root = self.render_component(id, parent);
                    self.attach_replacement(parent, previous, root, new_root);
                }
            }
            // Context changes only flow to descendants. Finish that finite wave
            // before layout, even through memo boundaries. Ordinary destructor
            // writes remain in the next batch, preserving the state contract.
            if self.components.queue.contexts.borrow().is_empty() {
                break;
            }
        }
        // User value destructors can enqueue writes while old output unmounts.
        // Keep that work for the next batch; never spin to convergence in a frame.
        let mut queued = self.components.queue.pending.borrow_mut();
        if queued.is_empty() {
            *queued = pending;
        }
        drop(queued);
        self.validate_pointer_capture();
        self.component_renders - before
    }

    /// Reconcile new root inputs, preserving matching widget IDs and state.
    /// build_root remains an explicit reset with fresh state and generational IDs.
    pub fn reconcile_root(&mut self, element: impl IntoElement) -> WidgetId {
        let _task_update = self.tasks.update();
        let old = self.root().map(|id| self.mounted_at(id));
        let next = self.reconcile_element(old, element.into_element(), None, None);
        let root = self.mounted_root(next);
        self.root = Some(root);
        self.validate_pointer_capture();
        root
    }
    /// Number of mounted component boundaries; ordinary widgets are not counted.
    pub fn component_count(&self) -> usize {
        self.components.instances.len()
    }
    /// Count retained slots for diagnostics. This visits mounted components.
    pub fn state_count(&self) -> usize {
        self.components
            .instances
            .values()
            .map(|i| i.hooks.len())
            .sum()
    }

    pub(crate) fn mounted_at(&self, root: WidgetId) -> Mounted {
        self.components
            .roots
            .get(&root)
            .copied()
            .map(Mounted::Component)
            .unwrap_or(Mounted::Widget(root))
    }
    pub(crate) fn surviving_root(&self, mounted: Mounted) -> Option<WidgetId> {
        match mounted {
            Mounted::Widget(id) => self.nodes.contains_key(id).then_some(id),
            Mounted::Component(id) => self.components.instances.get(id).and_then(|i| i.root),
        }
    }
    fn mounted_root(&self, mounted: Mounted) -> WidgetId {
        match mounted {
            Mounted::Widget(id) => id,
            Mounted::Component(id) => self.components.instances[id].root.unwrap(),
        }
    }
    fn mounted_key(&self, mounted: Mounted) -> Option<&SmolStr> {
        match mounted {
            Mounted::Widget(id) => self.nodes[id].props.key.as_ref(),
            Mounted::Component(id) => self.components.instances[id]
                .description
                .as_ref()
                .unwrap()
                .key
                .as_ref(),
        }
    }
    fn element_key(element: &Element) -> Option<&SmolStr> {
        match &element.kind {
            ElementKind::Widget(_) => element.props.key.as_ref(),
            ElementKind::Component(c) => c.key.as_ref(),
        }
    }
    pub(crate) fn component_owner(&self, mut node: WidgetId) -> Option<ComponentId> {
        loop {
            if let Some(mut id) = self.components.roots.get(&node).copied() {
                while let Some(Mounted::Component(child)) = self.components.instances[id].output {
                    id = child;
                }
                return Some(id);
            }
            node = self.nodes[node].parent?;
        }
    }
    pub(crate) fn mount_element(
        &mut self,
        element: Element,
        parent: Option<WidgetId>,
        owner: Option<ComponentId>,
    ) -> WidgetId {
        let mounted = self.reconcile_element(None, element, parent, owner);
        self.mounted_root(mounted)
    }
    fn matches(&self, old: Mounted, next: &Element) -> bool {
        if self.mounted_key(old) != Self::element_key(next) {
            return false;
        }
        match (old, &next.kind) {
            (Mounted::Widget(id), ElementKind::Widget(widget)) => {
                let old = self.nodes[id].widget.as_deref().unwrap();
                old.type_id() == widget.as_ref().type_id()
            }
            (Mounted::Component(id), ElementKind::Component(next)) => {
                self.components.instances[id]
                    .description
                    .as_ref()
                    .unwrap()
                    .kind
                    == next.kind
            }
            _ => false,
        }
    }
    fn reconcile_element(
        &mut self,
        old: Option<Mounted>,
        element: Element,
        parent: Option<WidgetId>,
        owner: Option<ComponentId>,
    ) -> Mounted {
        let old = old.filter(|old| {
            if self.matches(*old, &element) {
                true
            } else {
                self.unmount_element(*old, None);
                false
            }
        });
        match element.kind {
            ElementKind::Component(mut description) => {
                description.events.append(element.events);
                let id = if let Some(Mounted::Component(id)) = old {
                    let instance = &self.components.instances[id];
                    if !instance.signal.dirty.get()
                        && instance
                            .description
                            .as_ref()
                            .unwrap()
                            .same_inputs(&description)
                    {
                        return Mounted::Component(id);
                    }
                    self.components.instances[id].description = Some(description);
                    id
                } else {
                    let depth = owner
                        .map(|id| self.components.instances[id].depth + 1)
                        .unwrap_or(0);
                    let queue = Rc::downgrade(&self.components.queue);
                    let context = super::environment::ContextScope::new(
                        owner.map(|id| self.components.instances[id].context.clone()),
                    );
                    self.components.instances.insert_with_key(|id| Instance {
                        context,
                        description: Some(description),
                        hooks: Hooks::default(),
                        signal: Rc::new(Signal::component(id, queue)),
                        output: None,
                        root: None,
                        parent: owner,
                        depth,
                    })
                };
                self.render_component(id, parent);
                Mounted::Component(id)
            }
            ElementKind::Widget(widget) => {
                let id = if let Some(Mounted::Widget(id)) = old {
                    let text_changed = self.nodes[id].widget.as_ref().unwrap().text_content()
                        != widget.text_content();
                    if text_changed {
                        let length = self.nodes[id]
                            .widget
                            .as_ref()
                            .unwrap()
                            .text_content()
                            .map(str::len);
                        if let (Some(length), Some(text)) = (length, widget.text_content()) {
                            self.selection_after_text_change(id, 0..length, text.len());
                        } else {
                            self.selection.get_mut().clear();
                        }
                    }
                    let mut props = element.props;
                    if self.nodes[id].top_layer == Some(super::top_layer::TopLayerKind::Modal) {
                        props.attributes.insert("open".into(), "".into());
                    }
                    self.set_scrollbar_mode(id, props.scrollbar_mode);
                    if self.nodes[id].props != props {
                        let old = &self.nodes[id].props;
                        let selectors_changed = old.tag != props.tag
                            || old.id != props.id
                            || old.classes != props.classes
                            || old.attributes != props.attributes;
                        self.nodes[id].props = props;
                        if selectors_changed {
                            self.css_dirty = true;
                        } else {
                            self.invalidate_style(id);
                        }
                        self.selection.get_mut().invalidate(false);
                        self.paint_clip_dirty.set(true);
                        if self.focused().is_some_and(|id| {
                            self.is_inert(id)
                                || self.nodes[id].props.attributes.contains_key("disabled")
                        }) {
                            self.set_focused(None);
                        }
                    }
                    let accepted_input = self.nodes[id]
                        .widget
                        .as_ref()
                        .unwrap()
                        .text_input()
                        .is_some();
                    let accepted_events = self.nodes[id].widget.as_ref().unwrap().accepts_events();
                    let accepted_click = self.nodes[id].widget.as_ref().unwrap().accepts_click();
                    let retained = self.nodes[id]
                        .widget
                        .as_mut()
                        .unwrap()
                        .reconcile(widget.as_ref());
                    if retained == super::widget::WidgetUpdate::Replace {
                        self.nodes[id].widget = Some(widget);
                    }
                    self.events.custom_listeners = self.events.custom_listeners
                        + usize::from(self.nodes[id].widget.as_ref().unwrap().accepts_events())
                        - usize::from(accepted_events);
                    self.install_events(id, element.events);
                    self.input_handlers = self.input_handlers
                        + usize::from(
                            self.nodes[id]
                                .widget
                                .as_ref()
                                .unwrap()
                                .text_input()
                                .is_some(),
                        )
                        - usize::from(accepted_input);
                    self.click_handlers = self.click_handlers
                        + usize::from(self.nodes[id].widget.as_ref().unwrap().accepts_click())
                        - usize::from(accepted_click);
                    // Unknown widget inputs may affect geometry or drawing. Built-in
                    // widgets preserve their expensive shaping caches via reconcile.
                    if retained != super::widget::WidgetUpdate::Unchanged || text_changed {
                        self.invalidate_layout(id);
                        // Default-style changes are local. Text and compact SVG
                        // contents also participate in :empty/:has() selectors.
                        if text_changed
                            || self.nodes[id]
                                .widget
                                .as_ref()
                                .unwrap()
                                .svg_document()
                                .is_some()
                        {
                            self.css_dirty = true;
                        } else {
                            self.invalidate_style(id);
                        }
                        self.paint_order_dirty.set(true);
                    }
                    self.attach_widget(id);
                    self.reconcile_children(id, element.children, owner);
                    id
                } else {
                    self.build_widget_node(
                        widget,
                        element.props,
                        element.events,
                        element.children,
                        parent,
                        owner,
                    )
                };
                Mounted::Widget(id)
            }
        }
    }
    fn render_component(&mut self, id: ComponentId, parent: Option<WidgetId>) -> WidgetId {
        let instance = &mut self.components.instances[id];
        let description = instance
            .description
            .take()
            .expect("component rendering cannot reenter itself");
        let mut hooks = std::mem::take(&mut instance.hooks);
        let signal = instance.signal.clone();
        let context = instance.context.clone();
        context.begin();
        let old = instance.output;
        let old_root = instance.root;
        // Restore reusable inputs and slots even when user code panics. The TLS
        // guard restores the ambient scope before the panic reaches this boundary.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            state::render(
                &mut hooks,
                signal,
                self.tasks.clone(),
                self.window_host.context(),
                context.clone(),
                || (description.render.borrow_mut())(),
            )
        }));
        let instance = &mut self.components.instances[id];
        instance.description = Some(description);
        instance.hooks = hooks;
        let mut element = match result {
            Ok(element) => element,
            Err(error) => std::panic::resume_unwind(error),
        };
        context.finish();
        // Forward wrapper modifiers before comparing retained props. Applying
        // them afterward would mark identical output CSS-dirty on every update.
        let description = self.components.instances[id].description.as_ref().unwrap();
        let mut events = description.events.clone();
        events.component_source(id);
        element.events.append(events);
        // The invocation's explicit styles override only the returned root's
        // matching properties. Forward the overlay through transparent component
        // chains before reconciliation, so memo comparisons and removals see it.
        if let Some(style) = &description.style {
            let target = match &mut element.kind {
                ElementKind::Widget(_) => &mut element.props.style,
                ElementKind::Component(child) => child.style.get_or_insert_with(Default::default),
            };
            target.overlay_inline(style, &Default::default());
        }
        let (output_id, classes) = match &mut element.kind {
            ElementKind::Widget(_) => (&mut element.props.id, &mut element.props.classes),
            ElementKind::Component(child) => (&mut child.id, &mut child.classes),
        };
        if description.id.is_some() {
            *output_id = description.id.clone();
        }
        for class in &description.classes {
            if !classes.contains(class) {
                classes.push(class.clone());
            }
        }
        self.component_renders += 1;
        // Hide this root boundary while replacing its output. A changed root
        // unmounts the old output, but must not unmount its surviving wrappers.
        let head = old_root.and_then(|root| self.components.roots.remove(&root));
        let output = self.reconcile_element(old, element, parent, Some(id));
        let root = self.mounted_root(output);
        let instance = &mut self.components.instances[id];
        instance.output = Some(output);
        instance.root = Some(root);
        let mut current = id;
        loop {
            let instance = &self.components.instances[current];
            let Some(parent) = instance.parent else {
                break;
            };
            if self.components.instances[parent].output != Some(Mounted::Component(current)) {
                break;
            }
            self.components.instances[parent].root = Some(root);
            current = parent;
        }
        self.components.roots.insert(root, head.unwrap_or(id));
        root
    }
    fn reconcile_children(
        &mut self,
        parent: WidgetId,
        elements: Vec<Element>,
        owner: Option<ComponentId>,
    ) {
        Self::validate_keys(&elements);
        if elements.is_empty() && self.nodes[parent].children.is_empty() {
            return;
        }
        let old = self.nodes[parent].children.clone();
        // Build a key index once. Reversing a keyed list remains O(siblings),
        // while unkeyed children retain their exact sibling positions.
        let mut keyed = HashMap::new();
        let mut unkeyed = Vec::with_capacity(old.len());
        for (index, root) in old.iter().copied().enumerate() {
            let mounted = self.mounted_at(root);
            if let Some(key) = self.mounted_key(mounted) {
                keyed.insert(key.clone(), mounted);
                unkeyed.push(None);
            } else {
                unkeyed.push(Some(mounted));
            }
            debug_assert_eq!(index + 1, unkeyed.len());
        }
        let mut next = Vec::with_capacity(elements.len());
        for (index, element) in elements.into_iter().enumerate() {
            let previous = match Self::element_key(&element) {
                Some(key) => keyed.remove(key),
                None => unkeyed.get_mut(index).and_then(Option::take),
            };
            let inserted = previous.is_none_or(|old| !self.matches(old, &element));
            let mounted = self.reconcile_element(previous, element, Some(parent), owner);
            if inserted {
                self.selection_before_insert(parent, index);
            }
            next.push(self.mounted_root(mounted));
        }
        let mut removed: HashMap<_, _> = keyed
            .into_values()
            .chain(unkeyed.into_iter().flatten())
            .map(|mounted| (self.mounted_root(mounted), mounted))
            .collect();
        // Delete in reverse DOM order with known indices. The parent vector is
        // committed once below, so removing many siblings is linear, including
        // live selection adjustment, instead of repeatedly retaining/scanning it.
        if !removed.is_empty() {
            for index in (0..self.nodes[parent].children.len()).rev() {
                let root = self.nodes[parent].children[index];
                if let Some(mounted) = removed.remove(&root) {
                    self.unmount_element(mounted, Some(index));
                }
            }
        }
        if old != next {
            self.selection.get_mut().invalidate(true);
            self.css_dirty = true;
            self.layout_ready = false;
            self.paint_order_dirty.set(true);
        }
        self.nodes[parent].children = next;
        self.link_children(parent);
    }
    pub(crate) fn validate_child_key(&self, parent: WidgetId, element: &Element) {
        if let Some(key) = Self::element_key(element) {
            assert!(
                !self.nodes[parent]
                    .children
                    .iter()
                    .any(|id| self.mounted_key(self.mounted_at(*id)) == Some(key)),
                "duplicate sibling key: {key}"
            );
        }
    }
    pub(crate) fn validate_keys(elements: &[Element]) {
        let mut keys = HashSet::new();
        for element in elements {
            if let Some(key) = Self::element_key(element) {
                assert!(keys.insert(key), "duplicate sibling key: {key}");
            }
        }
    }
    pub(crate) fn link_children(&mut self, parent: WidgetId) {
        for index in 0..self.nodes[parent].children.len() {
            let id = self.nodes[parent].children[index];
            self.nodes[id].previous_sibling =
                index.checked_sub(1).map(|i| self.nodes[parent].children[i]);
            self.nodes[id].next_sibling = self.nodes[parent].children.get(index + 1).copied();
        }
    }
    fn attach_replacement(
        &mut self,
        parent: Option<WidgetId>,
        previous: Option<WidgetId>,
        old: WidgetId,
        next: WidgetId,
    ) {
        if old == next {
            return;
        }
        if let Some(parent) = parent {
            // Finding an insertion index is needed only when the physical root
            // changes. Ordinary local updates never scan the sibling list.
            let index = previous
                .map(|id| {
                    self.nodes[parent]
                        .children
                        .iter()
                        .position(|child| *child == id)
                        .unwrap()
                        + 1
                })
                .unwrap_or(0);
            self.selection_before_insert(parent, index);
            self.nodes[parent].children.insert(index, next);
            self.link_children(parent);
        } else {
            self.root = Some(next);
        }
        self.css_dirty = true;
        self.layout_ready = false;
        self.paint_order_dirty.set(true);
    }
    fn unmount_element(&mut self, mounted: Mounted, index: Option<usize>) {
        match mounted {
            Mounted::Component(id) => {
                if let Some(instance) = self.components.instances.remove(id) {
                    if let Some(root) = instance.root {
                        self.components.roots.remove(&root);
                    }
                    if let Some(output) = instance.output {
                        self.unmount_element(output, index);
                    }
                }
            }
            Mounted::Widget(id) => {
                self.remove_subtree_at(id, index);
            }
        }
    }
    /// Called as ordinary DOM nodes are removed. Wrappers at the same root form
    /// a chain; nested roots are visited by remove_subtree's existing traversal.
    pub(crate) fn unmount_components_at(&mut self, root: WidgetId) {
        let mut next = self.components.roots.remove(&root);
        while let Some(id) = next {
            next =
                self.components
                    .instances
                    .remove(id)
                    .and_then(|instance| match instance.output {
                        Some(Mounted::Component(child)) => Some(child),
                        _ => None,
                    });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{component, div, state};
    use std::cell::RefCell;

    #[test]
    fn equal_wrapped_output_keeps_css_layout_and_paint_caches_clean() {
        let handle = Rc::new(RefCell::new(None));
        let slot = handle.clone();
        let mut tree = WidgetTree::new();
        tree.build_root(
            component(move || {
                let value = state(|| 0usize);
                value.with(|_| ());
                *slot.borrow_mut() = Some(value);
                component(div).id("inner").class("inner")
            })
            .id("outer")
            .class("outer"),
        );
        tree.update_styles(std::time::Instant::now());
        tree.layout_ready = true;
        tree.paint_order_dirty.set(false);
        handle.borrow().as_ref().unwrap().set(1);
        tree.flush_updates();
        assert!(!tree.css_dirty);
        assert!(tree.layout_ready);
        assert!(!tree.paint_order_dirty.get());
    }
}
