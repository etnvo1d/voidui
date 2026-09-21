use crate::{
    core::animation::Transitions,
    style::{computed::ComputedStyle, transition::TransitionStyle},
};
use slotmap::SlotMap;
use std::time::Instant;
use vector_map::VecMap;
use voidui_gpui_wgpu::{Painter, Result, TextLayoutCache};

use crate::style::{
    css::{CascadeStats, Stylesheet},
    paint::PaintStyle,
    text::TextStyle,
};

use crate::core::{
    context::{DrawContext, LayoutContext},
    element::{Element, ElementProps, IntoElement},
    geometry::{Point, Rect, Size},
    keycode::MouseButton,
    layout::{
        self, AvailableSpace, BlockContext, Cache, Display, Layout, LayoutInput, LayoutOutput,
        RunMode,
    },
    paint::paint_box,
    widget::{Widget, WidgetId, WidgetStatus},
};

pub(crate) struct Node {
    pub(crate) scroll: Option<Box<super::scroll::ScrollState>>,
    // Temporarily remove the widget while it recursively lays out its children.
    pub widget: Option<Box<dyn Widget>>,
    pub status: WidgetStatus,
    pub focused_descendants: usize,
    pub parent: Option<WidgetId>,
    pub previous_sibling: Option<WidgetId>,
    pub next_sibling: Option<WidgetId>,
    pub cascaded_style: Option<Box<crate::style::style::Style>>,
    pub children: Vec<WidgetId>,
    // Most nodes use their DOM children directly. Keep the absent override one
    // pointer wide; allocate a capacity-reusing Vec only for relocated children.
    #[allow(clippy::box_collection)]
    pub(crate) layout_children: Option<Box<Vec<WidgetId>>>,
    pub(crate) layout_parent: Option<WidgetId>,
    pub(crate) tree_order: usize,
    pub(crate) top_layer: Option<super::top_layer::TopLayerKind>,
    pub(crate) backdrop: Option<Box<super::stacking::Backdrop>>,
    pub props: ElementProps,
    pub computed: ComputedStyle,
    // Inline edits follow the ancestor path; selector/tree edits still rematch
    // globally because :has(), sibling selectors and nth-child can reach outside it.
    style_dirty: bool,
    subtree_style_dirty: bool,
    transition_style: Option<Box<TransitionStyle>>,
    pub(crate) style_revision: u64,
    pub(crate) highlight: Option<Box<crate::style::selection::HighlightState>>,
    paint_cache: crate::core::paint::PaintCache,
    transitions: Option<Box<Transitions>>,
    subtree_animating: bool,
    pub(crate) rendered: bool,
    pub layout: Layout,
    pub cache: Cache,
    pub global_bounds: Rect<f32>,
    pub(crate) visual: Option<std::sync::Arc<super::spatial::VisualGeometry>>,
}

/// One style-change event shares a timestamp and accumulates invalidation once.
struct StyleFrame {
    now: Instant,
    style_change: bool,
    all_styles: bool,
    full_cascade: bool,
    changes: StyleChange,
}

pub(crate) struct StatusedWidgets {
    pub(crate) top_hovered: Option<WidgetId>,
    pub(crate) focused: Option<WidgetId>,
    pub(crate) mouse_down: VecMap<MouseButton, Option<WidgetId>>,
}

impl StatusedWidgets {
    pub fn new() -> Self {
        Self {
            top_hovered: None,
            focused: None,
            mouse_down: VecMap::new(),
        }
    }
}

pub struct WidgetTree {
    pub(crate) window_host: super::decoration::WindowHost,
    pub(crate) scroll_options: super::scroll::ScrollOptions,
    pub(crate) scrollers: Vec<WidgetId>,
    pub(crate) scroll_drag: Option<super::scroll::ScrollDrag>,
    pub(crate) events: super::interaction::EventState,
    pub(crate) widget_updates: std::rc::Rc<super::updates::WidgetQueue>,
    pub(crate) tasks: crate::tasks::host::TaskHost,
    pub(crate) root: Option<WidgetId>,
    pub(crate) components: super::reconcile::Components,
    pub(crate) component_renders: usize,
    pub(crate) nodes: SlotMap<WidgetId, Node>,
    pub(crate) statused_widgets: StatusedWidgets,
    pub(crate) click_pressed: Option<WidgetId>,
    pub(crate) click_handlers: usize,
    pub(crate) input_handlers: usize,
    pub(crate) pointer_position: Option<Point<f32>>,
    pub(crate) pointer_revision: u64,
    pub(crate) layout_ready: bool,
    layout_cache_dirty: bool,
    layout_cache_root: Option<WidgetId>,
    layout_fonts: Option<(std::sync::Weak<voidui_gpui_wgpu::TextSystem>, u64)>,
    pub(crate) stylesheets: Vec<Stylesheet>,
    pub(crate) style_revision: u64,
    pub(crate) svg_css_state: WidgetStatus,
    pub(crate) css_dirty: bool,
    styles_ready: bool,
    css_viewport: [f32; 2],
    css_viewport_explicit: bool,
    style_context_dirty: bool,
    last_style_sample: Option<Instant>,
    cascade_stats: CascadeStats,
    style_resolutions: u64,
    pub(crate) viewport: super::positioning::ViewportLayout,
    pub(crate) top_layers: Vec<super::top_layer::TopLayerEntry>,
    pub(crate) transform_overflow: std::collections::HashMap<WidgetId, layout::Rect<f32>>,
    pub(crate) paint_order: std::cell::RefCell<super::stacking::PaintOrder>,
    pub(crate) paint_order_dirty: std::cell::Cell<bool>,
    pub(crate) paint_clip_dirty: std::cell::Cell<bool>,
    pub(crate) selection: std::cell::RefCell<super::selection::SelectionState>,
    pub(crate) selection_colors: crate::style::selection::SelectionColors,
}

impl WidgetTree {
    pub fn new() -> Self {
        Self::with_task_runtime(crate::tasks::TaskRuntime::default())
    }

    /// Share a task executor with other trees or a native host.
    pub fn with_task_runtime(runtime: crate::tasks::TaskRuntime) -> Self {
        Self {
            window_host: Default::default(),
            scroll_options: Default::default(),
            scrollers: Vec::new(),
            scroll_drag: None,
            events: Default::default(),
            widget_updates: Default::default(),
            tasks: crate::tasks::host::TaskHost::new(runtime),
            root: None,
            components: Default::default(),
            component_renders: 0,
            nodes: SlotMap::with_key(),
            statused_widgets: StatusedWidgets::new(),
            click_pressed: None,
            click_handlers: 0,
            input_handlers: 0,
            pointer_position: None,
            pointer_revision: 0,
            layout_ready: false,
            layout_cache_dirty: true,
            layout_cache_root: None,
            layout_fonts: None,
            stylesheets: Vec::new(),
            style_revision: 0,
            svg_css_state: WidgetStatus::empty(),
            css_dirty: true,
            styles_ready: false,
            css_viewport: [0.0; 2],
            css_viewport_explicit: false,
            style_context_dirty: false,
            last_style_sample: None,
            cascade_stats: CascadeStats::default(),
            style_resolutions: 0,
            viewport: Default::default(),
            top_layers: Vec::new(),
            transform_overflow: Default::default(),
            paint_order: Default::default(),
            paint_order_dirty: std::cell::Cell::new(true),
            paint_clip_dirty: std::cell::Cell::new(true),
            selection: Default::default(),
            selection_colors: Default::default(),
        }
    }

    /// Access the executor. Headless hosts call tick() when its waker fires.
    pub fn task_runtime(&self) -> &crate::tasks::TaskRuntime {
        self.tasks.runtime()
    }

    /// Tasks in this scope survive root resets and stop when this tree is dropped.
    pub fn task_scope(&self) -> crate::tasks::TaskScope {
        self.tasks.window_scope()
    }

    /// Replace the previous tree and return the new root's generational ID.
    pub fn build_root(&mut self, element: impl IntoElement) -> WidgetId {
        let _task_update = self.tasks.update();
        let element = element.into_element();
        self.cancel_pointer_capture();
        self.events = Default::default();
        self.selection.get_mut().clear();
        self.click_pressed = None;
        self.click_handlers = 0;
        self.input_handlers = 0;
        self.components.clear();
        self.root = None;
        self.nodes.clear();
        self.scrollers.clear();
        self.svg_css_state = WidgetStatus::empty();
        self.top_layers.clear();
        self.paint_order_dirty.set(true);
        self.styles_ready = false;
        self.css_dirty = true;
        self.layout_ready = false;
        self.statused_widgets = StatusedWidgets::new();
        let root = self.mount_element(element, None, None);
        self.root = Some(root);
        root
    }

    /// Insert an ordinary element at runtime without rebuilding the document or
    /// invalidating existing IDs. Selector links and layout ownership stay separate.
    pub fn append_child(
        &mut self,
        parent: WidgetId,
        element: impl IntoElement,
    ) -> anyhow::Result<WidgetId> {
        let _task_update = self.tasks.update();
        anyhow::ensure!(self.nodes.contains_key(parent), "stale parent element");
        let element = element.into_element();
        self.validate_child_key(parent, &element);
        let index = self.nodes[parent].children.len()
            + usize::from(
                self.nodes[parent]
                    .widget
                    .as_ref()
                    .is_some_and(|w| w.text_content().is_some()),
            );
        self.selection_before_insert(parent, index);
        let owner = self.component_owner(parent);
        let id = self.mount_element(element, Some(parent), owner);
        if let Some(previous) = self.nodes[parent].children.last().copied() {
            self.nodes[previous].next_sibling = Some(id);
            self.nodes[id].previous_sibling = Some(previous);
        }
        self.nodes[parent].children.push(id);
        self.css_dirty = true;
        self.layout_ready = false;
        self.paint_order_dirty.set(true);
        Ok(id)
    }
    /// Remove a subtree and its top-layer entries, keeping surviving generational
    /// IDs and sibling links intact. Stale IDs are harmless no-ops.
    pub fn remove_subtree(&mut self, id: WidgetId) -> bool {
        let _task_update = self.tasks.update();
        self.remove_subtree_at(id, None)
    }
    /// A known index belongs to a bulk removal whose caller commits the parent
    /// child list afterward. Sibling/status/overlay cleanup is shared by both paths.
    pub(crate) fn remove_subtree_at(&mut self, id: WidgetId, index: Option<usize>) -> bool {
        if !self.nodes.contains_key(id) {
            return false;
        }
        if self
            .pointer_capture()
            .is_some_and(|owner| self.is_descendant_or_self(owner, id))
        {
            self.cancel_pointer_capture();
        }
        if let Some(index) = index {
            let parent = self.nodes[id]
                .parent
                .expect("bulk removals require a parent");
            let own_text = usize::from(
                self.nodes[parent]
                    .widget
                    .as_ref()
                    .is_some_and(|w| w.text_content().is_some()),
            );
            self.selection_before_remove_at(id, parent, index + own_text);
        } else {
            self.selection_before_remove(id);
        }
        let overlays: Vec<_> = self
            .top_layers
            .iter()
            .filter(|e| self.is_descendant_or_self(e.id, id))
            .map(|e| e.id)
            .collect();
        for overlay in overlays {
            self.close_top_layer(overlay);
        }
        if self
            .focused()
            .is_some_and(|focus| self.is_descendant_or_self(focus, id))
        {
            self.set_focused(None);
        }
        if self
            .statused_widgets
            .top_hovered
            .is_some_and(|hover| self.is_descendant_or_self(hover, id))
        {
            let mut current = self.statused_widgets.top_hovered.take();
            while let Some(hover) = current {
                let mut status = self.nodes[hover].status;
                status.set_hovered(false);
                self.set_status(hover, status);
                current = self.nodes[hover].parent;
            }
        }
        if self
            .statused_widgets
            .mouse_down
            .get(&MouseButton::Left)
            .copied()
            .flatten()
            .is_some_and(|pressed| self.is_descendant_or_self(pressed, id))
        {
            self.statused_widgets
                .mouse_down
                .insert(MouseButton::Left, None);
        }
        let parent = self.nodes[id].parent;
        let previous = self.nodes[id].previous_sibling;
        let next = self.nodes[id].next_sibling;
        if let Some(previous) = previous {
            self.nodes[previous].next_sibling = next;
        }
        if let Some(next) = next {
            self.nodes[next].previous_sibling = previous;
        }
        if let Some(parent) = parent {
            if index.is_none() {
                self.nodes[parent].children.retain(|child| *child != id);
            }
        } else {
            self.root = None;
        }
        let mut pending = vec![id];
        while let Some(id) = pending.pop() {
            self.unmount_components_at(id);
            self.events.remove(id);
            if let Some(node) = self.nodes.remove(id) {
                self.events.custom_listeners -=
                    usize::from(node.widget.as_ref().is_some_and(|w| w.accepts_events()));
                self.input_handlers -= usize::from(
                    node.widget
                        .as_ref()
                        .is_some_and(|w| w.text_input().is_some()),
                );
                self.click_handlers -=
                    usize::from(node.widget.as_ref().is_some_and(|w| w.accepts_click()));
                pending.extend(node.children);
            }
        }
        self.css_dirty = true;
        self.layout_ready = false;
        self.paint_order_dirty.set(true);
        true
    }
    /// Find the first matching CSS ID in DOM insertion order. Callers may cache
    /// the returned generational ID across style updates and overlay operations.
    /// Read a canonical element attribute without exposing mutable selector state.
    pub fn attribute(&self, id: WidgetId, name: &str) -> Option<std::borrow::Cow<'_, str>> {
        let props = &self.nodes.get(id)?.props;
        match name {
            "id" => props.id.as_deref().map(std::borrow::Cow::Borrowed),
            "class" => (!props.classes.is_empty())
                .then(|| std::borrow::Cow::Owned(props.classes.join(" "))),
            _ => props
                .attributes
                .get(name)
                .map(|s| std::borrow::Cow::Borrowed(s.as_str())),
        }
    }
    /// Read current widget text without requiring shaping or layout.
    pub fn text_content(&self, id: WidgetId) -> Option<&str> {
        self.nodes.get(id)?.widget.as_ref()?.text_content()
    }

    pub fn find_by_id(&self, value: &str) -> Option<WidgetId> {
        self.nodes
            .iter()
            .find(|(_, n)| n.props.id.as_deref() == Some(value))
            .map(|(id, _)| id)
    }

    pub(crate) fn build_widget_node(
        &mut self,
        widget: Box<dyn Widget>,
        props: ElementProps,
        events: super::interaction::EventBindings,
        children: Vec<Element>,
        parent: Option<WidgetId>,
        owner: Option<super::reconcile::ComponentId>,
    ) -> WidgetId {
        Self::validate_keys(&children);
        // The immediate boundary count is already known at mount. Reserving it
        // once avoids geometric slot-table slack in large component lists.
        self.components.reserve(
            children
                .iter()
                .filter(|child| matches!(child.kind, super::element::ElementKind::Component(_)))
                .count(),
        );
        self.events.custom_listeners += usize::from(widget.accepts_events());
        self.click_handlers += usize::from(widget.accepts_click());
        self.input_handlers += usize::from(widget.text_input().is_some());
        let node = Node {
            scroll: None,
            widget: Some(widget),
            status: WidgetStatus::default(),
            focused_descendants: 0,
            parent,
            previous_sibling: None,
            next_sibling: None,
            cascaded_style: None,
            children: Vec::with_capacity(children.len()),
            layout_children: None,
            layout_parent: parent,
            tree_order: 0,
            top_layer: None,
            backdrop: None,
            props,
            computed: ComputedStyle::default(),
            style_dirty: false,
            subtree_style_dirty: false,
            transition_style: None,
            style_revision: 0,
            highlight: None,
            paint_cache: Default::default(),
            transitions: None,
            subtree_animating: false,
            rendered: false,
            layout: Layout::default(),
            cache: Cache::new(),
            global_bounds: Rect::default(),
            visual: None,
        };
        let id = self.nodes.insert(node);
        self.install_events(id, events);
        self.attach_widget(id);

        for child in children {
            let child_id = self.mount_element(child, Some(id), owner);
            if let Some(previous) = self.nodes[id].children.last().copied() {
                self.nodes[child_id].previous_sibling = Some(previous);
                self.nodes[previous].next_sibling = Some(child_id);
            }
            self.nodes[id].children.push(child_id);
        }

        self.css_dirty = true;
        self.layout_ready = false;
        self.paint_order_dirty.set(true);
        id
    }

    /// Install compiled author stylesheets. Equal replacements do not invalidate.
    pub fn set_stylesheets(&mut self, sheets: Vec<Stylesheet>) -> bool {
        if sheets.len() == self.stylesheets.len()
            && sheets
                .iter()
                .zip(&self.stylesheets)
                .all(|(a, b)| a.same_rules(b))
        {
            return false;
        }
        self.stylesheets = sheets;
        self.css_dirty = true;
        true
    }
    pub fn cascade_stats(&self) -> CascadeStats {
        self.cascade_stats
    }
    /// Cumulative computed-style resolutions, including inheritance and animation.
    /// Unlike cascade_stats, this also counts work that does not match selectors.
    pub fn style_resolutions(&self) -> u64 {
        self.style_resolutions
    }
    pub fn has_dynamic_css(&self) -> bool {
        self.stylesheets.iter().any(Stylesheet::uses_state) || !self.svg_css_state.is_empty()
    }
    pub(crate) fn cascaded_style(&self, id: WidgetId) -> &crate::style::style::Style {
        self.nodes[id]
            .cascaded_style
            .as_deref()
            .unwrap_or(&self.nodes[id].props.style)
    }
    pub fn set_tag(&mut self, id: WidgetId, tag: &str) {
        let tag = tag.to_ascii_lowercase();
        if self.nodes[id].props.tag != tag {
            self.nodes[id].props.tag = tag.into();
            self.css_dirty = true;
        }
    }
    pub fn set_id(&mut self, id: WidgetId, value: Option<&str>) {
        let value = value.map(Into::into);
        if self.nodes[id].props.id != value {
            self.nodes[id].props.id = value;
            self.css_dirty = true;
        }
    }
    pub fn set_classes(&mut self, id: WidgetId, classes: &str) {
        let mut value = Vec::new();
        for class in classes.split_ascii_whitespace() {
            let class: winit::keyboard::SmolStr = class.into();
            if !value.contains(&class) {
                value.push(class);
            }
        }
        if self.nodes[id].props.classes != value {
            self.nodes[id].props.classes = value;
            self.css_dirty = true;
        }
    }
    pub fn set_attribute(&mut self, id: WidgetId, name: &str, value: Option<&str>) {
        match name {
            "id" => {
                self.set_id(id, value);
                return;
            }
            "class" => {
                self.set_classes(id, value.unwrap_or(""));
                return;
            }
            _ => {}
        }
        let name: winit::keyboard::SmolStr = name.to_ascii_lowercase().into();
        let affects_input = matches!(name.as_str(), "inert" | "disabled");
        let previous = match value {
            Some(value) => self.nodes[id].props.attributes.insert(name, value.into()),
            None => self.nodes[id].props.attributes.remove(&name),
        };
        if previous.as_deref() != value {
            self.attach_widget(id);
            self.layout_ready = false;
            self.css_dirty = true;
            if affects_input {
                self.validate_pointer_capture();
                self.selection.get_mut().invalidate(false);
                self.paint_clip_dirty.set(true);
                if self.focused().is_some_and(|id| {
                    self.is_inert(id) || self.nodes[id].props.attributes.contains_key("disabled")
                }) {
                    self.set_focused(None);
                }
            }
        }
    }
    pub fn set_status(&mut self, id: WidgetId, status: WidgetStatus) -> bool {
        if self.nodes[id].status == status {
            return false;
        }
        let changed_flags = self.nodes[id].status ^ status;
        let was_focused = self.nodes[id].status.is_focused();
        self.nodes[id].status = status;
        if was_focused != status.is_focused() {
            let mut current = Some(id);
            while let Some(id) = current {
                if status.is_focused() {
                    self.nodes[id].focused_descendants += 1;
                } else {
                    self.nodes[id].focused_descendants -= 1;
                }
                current = self.nodes[id].parent;
            }
        }
        if self
            .stylesheets
            .iter()
            .any(|sheet| sheet.state_mask().intersects(changed_flags))
            || self.svg_css_state.intersects(changed_flags)
        {
            self.css_dirty = true;
            return true;
        }
        false
    }

    pub fn root(&self) -> Option<WidgetId> {
        self.root
    }

    pub(crate) fn requires_layout(&self) -> bool {
        !self.layout_ready
    }

    pub(crate) fn styles_pending(&self) -> bool {
        self.css_dirty
            || self.style_context_dirty
            || self
                .root
                .is_some_and(|id| self.nodes[id].subtree_style_dirty)
    }

    /// Only selector inputs require a whole-tree cascade. An inline declaration
    /// changes its own style and then propagates through inheritance until stable.
    pub(crate) fn invalidate_style(&mut self, id: WidgetId) {
        self.nodes[id].style_dirty = true;
        let mut current = Some(id);
        while let Some(id) = current {
            let node = &mut self.nodes[id];
            if node.subtree_style_dirty {
                break;
            }
            node.subtree_style_dirty = true;
            current = node.parent;
        }
    }

    /// Intrinsic sizes flow upward. Layout owners are DOM ancestors (or the
    /// synthetic viewport), so this also covers relocated absolute/fixed boxes.
    pub(crate) fn invalidate_layout(&mut self, id: WidgetId) {
        self.layout_ready = false;
        self.viewport.cache.clear();
        let mut current = Some(id);
        while let Some(id) = current {
            let node = &mut self.nodes[id];
            node.cache.clear();
            current = node.parent;
        }
    }

    pub(crate) fn invalidate_all_layouts(&mut self) {
        self.layout_ready = false;
        self.layout_cache_dirty = true;
    }

    pub fn children(&self, id: WidgetId) -> &[WidgetId] {
        &self.nodes[id].children
    }

    pub fn parent(&self, id: WidgetId) -> Option<WidgetId> {
        self.nodes[id].parent
    }

    pub fn style(&self, id: WidgetId) -> &crate::style::style::Style {
        &self.nodes[id].props.style
    }

    /// Edit specified styles. Call update_styles before drawing, and run layout
    /// if its returned invalidation flags require it. Native windows do this automatically.
    pub fn style_mut(&mut self, id: WidgetId) -> &mut crate::style::style::Style {
        self.invalidate_style(id);
        &mut self.nodes[id].props.style
    }

    pub fn selection_style(&self, id: WidgetId) -> &crate::style::selection::SelectionStyle {
        &self.nodes[id].computed.selection
    }
    pub fn highlight_style(&self, id: WidgetId) -> crate::style::selection::HighlightStyle {
        self.nodes[id]
            .highlight
            .as_ref()
            .map(|h| h.computed)
            .unwrap_or_default()
    }

    /// Current computed stacking, positioning, visibility and pointer-event values.
    pub fn layer_style(&self, id: WidgetId) -> &crate::style::layer::LayerStyle {
        &self.nodes[id].computed.layer
    }

    pub fn paint_style(&self, id: WidgetId) -> &PaintStyle {
        &self.nodes[id].computed.paint
    }

    /// Current computed layout properties, including any sampled transition value.
    pub fn layout_style(&self, id: WidgetId) -> &layout::LayoutStyle {
        &self.nodes[id].computed.layout
    }

    /// The unrounded CSS border box, padding, border, and overflow information.
    pub fn layout_result(&self, id: WidgetId) -> &Layout {
        &self.nodes[id].layout
    }

    pub fn bounds(&self, id: WidgetId) -> Rect<f32> {
        self.nodes[id].global_bounds
    }

    /// Global content box of the most recently laid out element.
    pub fn content_bounds(&self, id: WidgetId) -> Rect<f32> {
        let node = &self.nodes[id];
        let layout = &node.layout;
        Rect::from_xywh(
            node.global_bounds.origin.x
                + layout.border.left
                + layout.padding.left
                + self.mirror_gutter(id),
            node.global_bounds.origin.y + layout.border.top + layout.padding.top,
            (layout.size.width
                - layout.border.left
                - layout.border.right
                - layout.padding.left
                - layout.padding.right
                - layout.scrollbar_size.width)
                .max(0.0),
            (layout.size.height
                - layout.border.top
                - layout.border.bottom
                - layout.padding.top
                - layout.padding.bottom
                - layout.scrollbar_size.height)
                .max(0.0),
        )
    }

    /// Layout from the root in logical pixels. An empty tree occupies no space.
    ///
    /// Available space guides CSS sizing and wrapping; it is not a hard maximum.
    /// Set size/min_size/max_size on the root style to constrain its CSS box.
    pub fn layout(
        &mut self,
        available_space: layout::Size<AvailableSpace>,
        text_layout: &TextLayoutCache,
    ) -> Size<f32> {
        self.flush_updates();
        match self.root {
            Some(id) => self.layout_subtree(id, available_space, Point::default(), text_layout),
            None => Size::default(),
        }
    }

    /// Lay out a subtree as an independent root at `origin` in global coordinates.
    ///
    /// Ancestors and siblings are not reflowed. Use `layout` when a subtree's size
    /// change should affect the rest of the tree.
    pub fn layout_subtree(
        &mut self,
        root_id: WidgetId,
        available_space: layout::Size<AvailableSpace>,
        origin: Point<f32>,
        text_layout: &TextLayoutCache,
    ) -> Size<f32> {
        let mounted = self.mounted_at(root_id);
        if self.root == Some(root_id) {
            self.infer_css_viewport(available_space);
        }
        self.layout_ready = false;
        self.update_styles(Instant::now());
        let Some(root_id) = self.surviving_root(mounted) else {
            return Size::default();
        };
        self.layout_computed_subtree(root_id, available_space, origin, text_layout)
    }

    /// Lay out the styles already sampled by `update_styles`, without reading the
    /// clock again. This is useful for custom runtimes and deterministic animation
    /// tests. Use `layout` for the combined real-time update and layout operation.
    pub fn layout_computed(
        &mut self,
        available_space: layout::Size<AvailableSpace>,
        text_layout: &TextLayoutCache,
    ) -> Size<f32> {
        self.infer_css_viewport(available_space);
        if self.style_context_dirty && self.styles_ready {
            self.update_styles(self.last_style_sample.unwrap_or_else(Instant::now));
        }
        if self.root.is_none() {
            self.layout_ready = true;
            self.styles_ready = true;
            self.css_dirty = false;
            return Size::default();
        }
        assert!(
            self.styles_ready && !self.styles_pending() && !self.has_pending_updates(),
            "call update_styles before layout_computed"
        );
        self.root
            .map(|root| {
                self.layout_computed_subtree(root, available_space, Point::default(), text_layout)
            })
            .unwrap_or_default()
    }

    fn layout_computed_subtree(
        &mut self,
        root_id: WidgetId,
        available_space: layout::Size<AvailableSpace>,
        origin: Point<f32>,
        text_layout: &TextLayoutCache,
    ) -> Size<f32> {
        self.paint_clip_dirty.set(true);
        let fonts = std::sync::Arc::downgrade(text_layout.system());
        let font_revision = text_layout.font_revision();
        if self.layout_cache_dirty
            || self.layout_cache_root != Some(root_id)
            || self
                .layout_fonts
                .as_ref()
                .is_none_or(|(old, revision)| !old.ptr_eq(&fonts) || *revision != font_revision)
        {
            for node in self.nodes.values_mut() {
                node.cache.clear();
            }
            self.viewport.cache.clear();
            self.layout_cache_dirty = false;
        }
        self.layout_cache_root = Some(root_id);
        self.layout_fonts = Some((fonts, font_revision));
        self.prepare_positioning(root_id);
        self.prepare_scrolling();
        loop {
            self.restore_transform_overflow();
            // Gutter changes invalidate their owners and ancestors. Unaffected
            // measurements survive both convergence passes and later frames.
            let mut context = LayoutContext::new(root_id, self, text_layout, None);
            layout::layout_root(&mut context, root_id.into(), available_space);
            self.finish_positioning(root_id, available_space, origin, text_layout);
            self.refresh_scroll_content(text_layout);
            self.update_transform_overflow();
            if !self.resolve_scroll_gutters() {
                break;
            }
        }
        self.finish_scrolling();
        self.calc_layout_positions(root_id, origin);
        for index in 0..self.viewport.children.len() {
            self.calc_layout_positions(self.viewport.children[index], origin);
        }
        self.layout_backdrops();
        self.layout_ready = self.root == Some(root_id);
        self.nodes[root_id].global_bounds.size
    }

    pub(crate) fn layout_node(
        &mut self,
        id: WidgetId,
        inputs: LayoutInput,
        text_layout: &TextLayoutCache,
        block_context: Option<&mut BlockContext<'_>>,
    ) -> LayoutOutput {
        // Hidden ancestors suppress the entire subtree, including custom widgets.
        if inputs.run_mode == RunMode::PerformHiddenLayout
            || self.nodes[id].computed.layout.display == Display::None
        {
            let mut context = LayoutContext::new(id, self, text_layout, None);
            return layout::layout_hidden(&mut context, id.into());
        }
        if let Some(output) = self.nodes[id].cache.get(&inputs) {
            return output;
        }
        let mut widget = self.take_widget_assert(id);
        let mirror = self.mirror_gutter(id);
        let mut inner_inputs = inputs;
        let saved = if mirror > 0.0 {
            let style = &mut self.nodes[id].scroll.as_mut().unwrap().layout;
            let saved = (style.size.width, style.min_size.width, style.max_size.width);
            super::scroll::inset_layout_width(style, &mut inner_inputs, mirror);
            Some(saved)
        } else {
            None
        };
        let context = LayoutContext::new(id, self, text_layout, block_context);
        let mut output = widget.layout(inner_inputs, context);
        if let Some((size, min, max)) = saved {
            let style = &mut self.nodes[id].scroll.as_mut().unwrap().layout;
            style.size.width = size;
            style.min_size.width = min;
            style.max_size.width = max;
            output.size.width += mirror;
            if let Some(width) = inputs.known_dimensions.width {
                output.size.width = width;
            }
        }
        self.put_widget_assert(id, widget);
        self.nodes[id].cache.store(&inputs, output);
        output
    }

    /// Current computed text properties, including any sampled transition value.
    pub fn text_style(&self, id: WidgetId) -> &TextStyle {
        &self.nodes[id].computed.text
    }

    /// Paint visible widgets in tree order using their final content boxes.
    /// Run a full layout after building or changing styles before calling this.
    /// Errors from glyph rasterization or the atlas are returned to the caller.
    pub fn draw(&self, painter: &mut Painter<'_>) -> Result<()> {
        let Some(root) = self.root else {
            return Ok(());
        };
        if !self.layout_ready || self.styles_pending() || self.has_pending_updates() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "WidgetTree::layout must run after changes and before drawing",
            )
            .into());
        }
        let _ = root;
        self.ensure_paint_order();
        for entry in &self.paint_order.borrow().entries {
            let clip = entry.clip.bounds(painter.content_mask().bounds);
            if let Some(space) = &entry.space {
                if space.inverse.is_none() {
                    continue;
                }
                painter.with_space(space, |painter| self.draw_entry(entry.clone(), painter))?;
            } else {
                painter.with_clip(clip, |painter| self.draw_entry(entry.clone(), painter))?;
            }
        }
        Ok(())
    }

    fn draw_entry(
        &self,
        entry: super::stacking::PaintEntry,
        painter: &mut Painter<'_>,
    ) -> Result<()> {
        use super::stacking::Phase;
        let node = &self.nodes[entry.id];
        if entry.phase == Phase::Backdrop {
            if let Some(backdrop) = &node.backdrop
                && backdrop.style.layout.display != Display::None
                && backdrop.style.layer.visibility == crate::style::layer::Visibility::Visible
            {
                paint_box(
                    painter,
                    backdrop.bounds,
                    &backdrop.layout,
                    &backdrop.style.paint,
                    &backdrop.style.text,
                    &backdrop.cache,
                );
            }
            return Ok(());
        }
        if node.computed.layer.visibility != crate::style::layer::Visibility::Visible
            && !(entry.phase == Phase::Content
                && node
                    .widget
                    .as_ref()
                    .is_some_and(|w| w.svg_document().is_some()))
        {
            return Ok(());
        }
        let layout = &node.layout;
        if entry.phase == Phase::Box {
            paint_box(
                painter,
                node.global_bounds,
                layout,
                &node.computed.paint,
                &node.computed.text,
                &node.paint_cache,
            );
            return Ok(());
        }
        if entry.phase == Phase::Scrollbar {
            self.paint_scrollbars(entry.id, painter);
            return Ok(());
        }
        let content_bounds = self.scrolled_content_bounds(entry.id);
        node.widget
            .as_ref()
            .expect("widget must be present while drawing")
            .draw(
                painter,
                DrawContext {
                    element: Some((entry.id, self)),
                    selection: self.selection_paint(entry.id),
                    status: node.status,
                    bounds: node.global_bounds,
                    content_bounds,
                    color: node.computed.text.color,
                    text_align: node
                        .computed
                        .text
                        .align
                        .resolve(node.computed.text.direction),
                },
            )?;
        Ok(())
    }

    /// Set the logical viewport used by vw/vh before sampling styles.
    /// Native windows set this automatically. Intrinsic/subtree measurements keep
    /// the current viewport, since their available space is not a new window.
    pub fn set_viewport_size(&mut self, size: layout::Size<f32>) {
        self.css_viewport_explicit = true;
        self.update_css_viewport(size);
    }

    fn update_css_viewport(&mut self, size: layout::Size<f32>) {
        assert!(
            size.width.is_finite()
                && size.height.is_finite()
                && size.width >= 0.0
                && size.height >= 0.0,
            "viewport size must be finite and nonnegative"
        );
        let next = [size.width, size.height];
        if next != self.css_viewport {
            self.css_viewport = next;
            self.style_context_dirty = true;
            self.layout_ready = false;
        }
    }

    fn infer_css_viewport(&mut self, space: layout::Size<AvailableSpace>) {
        if self.css_viewport_explicit {
            return;
        }
        let axis = |v, previous| match v {
            AvailableSpace::Definite(n) => n,
            _ => previous,
        };
        self.update_css_viewport(layout::Size {
            width: axis(space.width, self.css_viewport[0]),
            height: axis(space.height, self.css_viewport[1]),
        });
    }

    /// Resolve dirty styles and sample active transitions without running layout.
    /// A clean, non-animating tree returns immediately without traversing nodes.
    pub fn update_styles(&mut self, now: Instant) -> StyleChange {
        self.flush_updates();
        let Some(root) = self.root else {
            return StyleChange::default();
        };
        // Consume this frame's paint request even when CSS/SVG or highlight
        // changes already require repainting. Leaving the flag set suppresses
        // the next scroll wakeup, so an idle editor would keep its stale scene.
        let paint_dirty = self.widget_updates.paint_dirty.replace(false);
        self.last_style_sample = Some(now);
        let full_cascade = self.css_dirty;
        let all_styles = full_cascade || self.style_context_dirty;
        let style_change = self.styles_pending();
        let mut backdrop_changed = false;
        let mut highlight_changed = false;
        if !style_change && !self.nodes[root].subtree_animating {
            return StyleChange {
                paint: paint_dirty,
                layout: !self.layout_ready,
            };
        }
        self.style_revision = self.style_revision.wrapping_add(1);
        if self.css_dirty {
            // Topology and selector changes can alter layout ownership, including
            // hidden/positioned descendants. Keep that conservative boundary.
            self.layout_cache_dirty = true;
            self.svg_css_state = WidgetStatus::empty();
            let highlights = crate::style::css::sheet::cascade_highlights(self, &self.stylesheets);
            let mut declarations: slotmap::SecondaryMap<WidgetId, crate::style::style::Style> =
                highlights.into_iter().collect();
            for (id, node) in &mut self.nodes {
                if let Some(widget) = &node.widget {
                    for sheet in widget.svg_stylesheets() {
                        self.svg_css_state |= sheet.state_mask();
                    }
                }
                let next = declarations.remove(id).map(Box::new);
                if let Some(highlight) = &mut node.highlight {
                    highlight_changed |= highlight.specified != next;
                    highlight.specified = next;
                } else if next.is_some() {
                    highlight_changed = true;
                    node.highlight = Some(Box::new(crate::style::selection::HighlightState {
                        specified: next,
                        ..Default::default()
                    }));
                }
            }
            if self.stylesheets.is_empty()
                && !self.nodes.values().any(|n| {
                    n.widget
                        .as_ref()
                        .is_some_and(|w| w.svg_document().is_some())
                })
            {
                let ids: Vec<_> = self.nodes.keys().collect();
                for id in ids {
                    let node = &self.nodes[id];
                    if matches!(
                        node.props.tag.as_str(),
                        "dialog" | "button" | "meter" | "progress" | "select"
                    ) || node
                        .widget
                        .as_ref()
                        .is_some_and(|w| w.default_style().is_some())
                        || node.props.attributes.contains_key("popover")
                    {
                        let mut style = crate::style::css::sheet::user_agent_style(self, id);
                        style.overlay_inline(
                            &node.props.style,
                            &crate::style::style::Style::default(),
                        );
                        self.nodes[id].cascaded_style = Some(Box::new(style));
                    } else {
                        self.nodes[id].cascaded_style = None;
                    }
                }
            } else {
                let mut stats = self.cascade_stats;
                let styles = crate::style::css::sheet::cascade(self, &self.stylesheets, &mut stats);
                self.cascade_stats = stats;
                for (id, style) in styles {
                    if let Some(existing) = &mut self.nodes[id].cascaded_style {
                        **existing = style;
                    } else {
                        self.nodes[id].cascaded_style = Some(Box::new(style));
                    }
                }
            }
            for index in 0..self.top_layers.len() {
                let id = self.top_layers[index].id;
                let style = crate::style::css::sheet::cascade_backdrop(self, id, &self.stylesheets);
                if let Some(backdrop) = &mut self.nodes[id].backdrop {
                    backdrop_changed |= backdrop.specified != style;
                    backdrop.specified = style;
                } else {
                    backdrop_changed = true;
                    self.nodes[id].backdrop = Some(Box::new(super::stacking::Backdrop {
                        specified: style,
                        style: ComputedStyle::default(),
                        layout: Default::default(),
                        bounds: Default::default(),
                        cache: Default::default(),
                    }));
                }
            }
            self.css_dirty = false;
        }
        if backdrop_changed {
            self.paint_clip_dirty.set(true);
        }
        let mut frame = StyleFrame {
            now,
            style_change,
            all_styles,
            full_cascade,
            changes: StyleChange {
                // Compact SVG children have CSS state without separate widget
                // boxes. A descendant-only rule change must still rebuild paint.
                paint: (full_cascade
                    && self.nodes.values().any(|n| {
                        n.widget
                            .as_ref()
                            .is_some_and(|w| w.svg_document().is_some())
                    }))
                    || backdrop_changed
                    || highlight_changed
                    || paint_dirty,
                ..Default::default()
            },
        };
        self.style_context_dirty = false;
        self.resolve_style_frame(
            root,
            &ComputedStyle::default(),
            &TransitionStyle::default(),
            false,
            true,
            &mut frame,
        );
        // Pseudo boxes resolve after their originating element and root font.
        // Their ordinary properties keep independent defaults; custom properties
        // inherit from the originating element.
        for entry in &self.top_layers {
            let node = &mut self.nodes[entry.id];
            if let Some(backdrop) = &mut node.backdrop {
                let parent = ComputedStyle {
                    custom_properties: node.computed.custom_properties.clone(),
                    ..ComputedStyle::default()
                };
                let next = ComputedStyle::resolve_with_context(
                    &backdrop.specified,
                    &parent,
                    true,
                    Some(node.computed.root_font_size),
                    self.css_viewport,
                );
                frame.changes.layout |= !backdrop.style.same_layout(&next);
                frame.changes.paint |= backdrop.style != next;
                self.paint_clip_dirty
                    .set(self.paint_clip_dirty.get() || backdrop.style != next);
                backdrop.style = next;
            }
        }
        if self.paint_clip_dirty.get() && self.layout_ready && !frame.changes.layout {
            self.update_transform_overflow();
            self.finish_scrolling();
            if self.transform_gutters_need_layout() || self.resolve_scroll_gutters() {
                frame.changes.layout = true;
            }
            self.refresh_visual_geometry();
        }
        // Trees without editable text skip the scan entirely; the maintained count
        // also stops it as soon as every input has been found.
        if self.input_handlers > 0 {
            let mut remaining = self.input_handlers;
            let mut input_ids = Vec::with_capacity(remaining);
            for (id, node) in self.nodes.iter() {
                if node
                    .widget
                    .as_ref()
                    .is_some_and(|w| w.text_input().is_some())
                {
                    input_ids.push(id);
                    remaining -= 1;
                    if remaining == 0 {
                        break;
                    }
                }
            }
            for id in input_ids {
                let placeholder =
                    crate::style::css::sheet::cascade_placeholder(self, id, &self.stylesheets);
                let colors = self
                    .highlight_style(id)
                    .colors(self.nodes[id].computed.text.color, self.selection_colors);
                let node = &mut self.nodes[id];
                // ::placeholder and ::selection are not part of the computed style
                // compared above, so this is the only signal that they changed.
                frame.changes.paint |= node
                    .widget
                    .as_mut()
                    .unwrap()
                    .text_input_mut()
                    .unwrap()
                    .update_presentation(&node.computed.text, placeholder, colors);
            }
        }
        self.styles_ready = true;
        if backdrop_changed {
            self.layout_backdrops();
        }

        // Reconciled content may invalidate layout without changing any CSS.
        // Report that work to headless hosts through the same public result.
        frame.changes.layout |= !self.layout_ready;
        if frame.changes.layout {
            self.layout_ready = false;
        }
        frame.changes.paint |= self.validate_pointer_capture();
        frame.changes
    }

    /// A future deadline while all transitions are delayed, or `now` while any
    /// transition is playing. Empty trees and completed animations return None.
    pub fn next_animation_frame(&self, now: Instant) -> Option<Instant> {
        fn visit(tree: &WidgetTree, id: WidgetId, now: Instant) -> Option<Instant> {
            let node = &tree.nodes[id];
            if !node.subtree_animating {
                return None;
            }
            node.transitions
                .as_ref()
                .and_then(|s| s.next_frame(now))
                .into_iter()
                .chain(node.children.iter().filter_map(|id| visit(tree, *id, now)))
                .min()
        }
        self.root.and_then(|id| visit(self, id, now))
    }

    fn resolve_style_frame(
        &mut self,
        id: WidgetId,
        parent: &ComputedStyle,
        parent_transitions: &TransitionStyle,
        parent_changed: bool,
        visible: bool,
        frame: &mut StyleFrame,
    ) {
        if !frame.all_styles
            && !parent_changed
            && !self.nodes[id].subtree_style_dirty
            && !self.nodes[id].subtree_animating
        {
            return;
        }
        let own_dirty = self.nodes[id].style_dirty;
        let restyle = frame.all_styles || own_dirty || parent_changed;
        if own_dirty && !frame.full_cascade {
            let mut stats = self.cascade_stats;
            let styles =
                crate::style::css::sheet::cascade_nodes(self, &self.stylesheets, [id], &mut stats);
            self.cascade_stats = stats;
            self.nodes[id].cascaded_style = Some(Box::new(styles.into_iter().next().unwrap().1));
        }
        self.style_resolutions += 1;
        let values = crate::style::css::values::resolve(
            self.cascaded_style(id),
            parent,
            (self.root != Some(id)).then_some(parent.root_font_size),
            self.css_viewport,
        );
        let transition_style = values.style.resolve_transitions(parent_transitions);
        let mut resolved =
            ComputedStyle::resolve_values(values, parent, self.is_top_layer(id), self.css_viewport);
        let visible = visible && resolved.layout.display != Display::None;
        let parent_highlight = self.nodes[id]
            .parent
            .and_then(|p| self.nodes[p].highlight.as_ref())
            .map(|h| h.computed)
            .unwrap_or_default();
        let node = &mut self.nodes[id];
        node.style_dirty = false;
        node.subtree_style_dirty = false;
        let transition_changed = node
            .transition_style
            .as_deref()
            .unwrap_or(&TransitionStyle::default())
            != &transition_style;
        if transition_changed {
            node.transition_style = (transition_style != TransitionStyle::default())
                .then(|| Box::new(transition_style.clone()));
        }
        let was_visible = node.rendered;
        let previous_highlight = node
            .highlight
            .as_ref()
            .map(|h| h.computed)
            .unwrap_or_default();
        let declarations = node
            .highlight
            .as_ref()
            .and_then(|h| h.specified.as_ref())
            .map(|style| {
                use crate::style::declaration::Property;
                let values = crate::style::css::values::resolve(
                    style,
                    &resolved,
                    Some(resolved.root_font_size),
                    resolved.viewport_size,
                );
                crate::style::selection::HighlightDeclarations {
                    color: style
                        .is_marked(Property::Color)
                        .then_some(values.style.color),
                    background: style
                        .is_marked(Property::Background)
                        .then_some(values.style.background),
                }
            })
            .unwrap_or_default();
        let highlight =
            crate::style::selection::HighlightStyle::resolve(declarations, parent_highlight);
        if highlight != Default::default()
            || declarations != Default::default()
            || node
                .highlight
                .as_ref()
                .is_some_and(|h| h.specified.is_some())
        {
            let h = node.highlight.get_or_insert_with(Default::default);
            frame.changes.paint |= h.computed != highlight;
            h.computed = highlight;
            h.declarations = declarations;
        } else if node.highlight.is_some() {
            frame.changes.paint = true;
            node.highlight = None;
        }
        Transitions::update(
            &mut node.transitions,
            &node.computed,
            &mut resolved,
            &transition_style,
            frame.now,
            frame.style_change && restyle && self.styles_ready && node.rendered,
            visible,
            [node.layout.size.width, node.layout.size.height],
        );
        if self.root == Some(id) {
            resolved.root_font_size = resolved.text.font_size;
        }
        node.rendered = visible;
        let changed = resolved != node.computed;
        frame.changes.paint |= changed;
        let layout_changed = !resolved.same_layout(&node.computed);
        frame.changes.layout |= layout_changed;
        let ownership_changed = node.computed.layer.position != resolved.layer.position
            || node.computed.transform.is_none() != resolved.transform.is_none()
            || node.computed.layout.display != resolved.layout.display;
        if restyle || changed {
            // SVG descendants have styles without separate widget nodes. Their
            // source cache follows this element, not unrelated tree-wide revisions.
            node.style_revision = self.style_revision;
            frame.changes.paint |= node
                .widget
                .as_ref()
                .is_some_and(|w| w.svg_document().is_some());
        }
        let transform_changed = node.computed.transform != resolved.transform
            || node.computed.transform_origin != resolved.transform_origin
            || node.computed.sticky_inset != resolved.sticky_inset;
        if transform_changed {
            self.paint_clip_dirty.set(true);
        }
        if node.computed.transform.is_none() != resolved.transform.is_none()
            || node.computed.layer != resolved.layer
            || node.computed.layout.display != resolved.layout.display
        {
            self.paint_order_dirty.set(true);
        }
        if node.computed.selection != resolved.selection
            || node.computed.layer.visibility != resolved.layer.visibility
            || node.computed.layout.display != resolved.layout.display
        {
            self.selection.get_mut().invalidate(false);
        }
        node.computed = resolved.clone();
        let mut animating = node.transitions.is_some();
        let propagate = changed
            || transition_changed
            || was_visible != visible
            || previous_highlight != highlight;
        if ownership_changed {
            self.layout_cache_dirty = true;
        }
        if layout_changed {
            self.invalidate_layout(id);
        }
        for i in 0..self.nodes[id].children.len() {
            let child = self.nodes[id].children[i];
            self.resolve_style_frame(
                child,
                &resolved,
                &transition_style,
                propagate,
                visible,
                frame,
            );
            animating |= self.nodes[child].subtree_animating;
        }
        self.nodes[id].subtree_animating = animating;
    }

    /// Change focused widget and update :focus-within in O(tree depth).
    pub fn set_focused(&mut self, id: Option<WidgetId>) -> bool {
        if id.is_some_and(|id| {
            self.is_inert(id)
                || self.nodes[id].props.attributes.contains_key("disabled")
                || (!self.styles_pending()
                    && (!self.is_rendered(id)
                        || self.nodes[id].computed.layer.visibility
                            == crate::style::layer::Visibility::Hidden))
        }) {
            return false;
        }
        if self.statused_widgets.focused == id {
            return false;
        }
        let mut changed = false;
        if let Some(old) = self.statused_widgets.focused {
            let mut status = self.nodes[old].status;
            status.set_focused(false);
            changed |= self.set_status(old, status);
            self.input_focus_changed(old, false);
        }
        if let Some(new) = id {
            let mut status = self.nodes[new].status;
            status.set_focused(true);
            changed |= self.set_status(new, status);
            self.input_focus_changed(new, true);
        }
        self.statused_widgets.focused = id;
        if let Some(id) = id {
            self.scroll_into_view(id);
        }
        let _ = changed;
        true
    }

    /// Update hit/hover transitions after style or geometry changes, without
    /// synthesizing a pointer move or another drag sample.
    pub fn refresh_pointer(&mut self) -> bool {
        self.refresh_event_pointer()
    }

    pub(crate) fn pointer_position(&self) -> Option<Point<f32>> {
        self.pointer_position
    }

    fn get_node_mut_assert(&mut self, id: WidgetId) -> &mut Node {
        self.nodes.get_mut(id).unwrap()
    }

    fn take_widget_assert(&mut self, id: WidgetId) -> Box<dyn Widget> {
        self.get_node_mut_assert(id).widget.take().expect("widget is currently not in the node, check if it has been taken and not given back yet")
    }

    fn put_widget_assert(&mut self, id: WidgetId, widget: Box<dyn Widget>) {
        self.get_node_mut_assert(id).widget = Some(widget);
    }
}

impl Default for WidgetTree {
    fn default() -> Self {
        Self::new()
    }
}

/// Work invalidated by a style/animation update. Paint-only changes preserve layout.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StyleChange {
    pub layout: bool,
    pub paint: bool,
}
