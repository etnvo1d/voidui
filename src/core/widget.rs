use bitflags::bitflags;
use slotmap::new_key_type;
use voidui_gpui_wgpu::{Painter, Result};
use winit::keyboard::SmolStr;

use crate::core::{
    context::{DrawContext, LayoutContext},
    element::{Element, ElementProps, IntoElement},
    event::{Event, EventResult},
    layout::{LayoutInput, LayoutOutput},
};

pub trait Widget: std::any::Any {
    /// Expose a compact SVG subtree to the shared CSS selector adapter. Internal
    /// SVG nodes do not allocate UI layout, event, selection or animation state.
    fn svg_document(&self) -> Option<&crate::svg::SvgDocument> {
        None
    }
    fn svg_stylesheets(&self) -> &[crate::style::css::Stylesheet] {
        &[]
    }

    /// Opt into native text input independently of the built-in editing engine.
    fn text_input(&self) -> Option<&dyn super::input::TextInputClient> {
        None
    }
    fn text_input_mut(&mut self) -> Option<&mut dyn super::input::TextInputClient> {
        None
    }

    /// Connect external model changes to this retained node. Called at mount and
    /// after reconciliation; implementations should retain one subscription only.
    fn attach(&mut self, _invalidator: super::updates::WidgetInvalidator) {}
    /// Synchronize a directly bound model before style/layout/paint. Return only
    /// additional invalidation; the queued widget already needs repainting.
    fn update_model(&mut self) -> super::updates::Invalidation {
        Default::default()
    }
    /// User-agent defaults remain below author CSS and inline declarations.
    fn default_style(&self) -> Option<crate::style::style::Style> {
        None
    }
    /// A composite control may forward background clicks to its first text input.
    fn delegates_focus(&self) -> bool {
        false
    }

    /// Apply a matching description while retaining reusable data. Report whether
    /// visual inputs changed; the default safely replaces unknown custom widgets.
    fn reconcile(&mut self, _next: &dyn Widget) -> WidgetUpdate {
        WidgetUpdate::Replace
    }

    /// Opt in to click activation without adding event storage to ordinary nodes.
    fn accepts_click(&self) -> bool {
        false
    }
    fn on_click(&mut self) {}
    /// Activation with an execution host. Existing widgets keep their synchronous
    /// handler; async wrappers use this host without adding fields to DOM nodes.
    fn on_click_with_tasks(&mut self, _runtime: &crate::tasks::TaskRuntime) {
        self.on_click();
    }

    /// Element name used by CSS type selectors; custom widgets may override it.
    fn tag_name(&self) -> &'static str {
        "widget"
    }
    /// Text content contributes to :empty without creating a second DOM tree.
    fn text_content(&self) -> Option<&str> {
        None
    }

    /// Expose final shaped text for document selection. Custom text widgets can
    /// opt in without the runtime depending on their concrete widget type.
    fn prepared_text(&self) -> Option<&crate::core::text::PreparedText> {
        None
    }

    /// Opt into live text replacement. Return false if this widget cannot edit
    /// its text storage; editing commands are not otherwise implied by selection.
    fn set_text_content(&mut self, _: voidui_gpui_wgpu::SharedString) -> bool {
        false
    }

    /// Compute size or final layout according to `inputs.run_mode`.
    ///
    /// Containers can delegate to `ctx.layout_children(inputs)`. Intrinsic leaves
    /// should use `layout_leaf` so CSS sizing and content measurements stay separate.
    fn layout(&mut self, inputs: LayoutInput, ctx: LayoutContext<'_, '_>) -> LayoutOutput;

    /// Return false when draw() paints nothing. Containers can omit empty content
    /// entries while retaining their CSS backgrounds, descendants and hit region.
    fn paints_content(&self) -> bool {
        true
    }

    /// Override to paint widget-specific content inside the computed bounds.
    fn draw(&self, _painter: &mut Painter<'_>, _ctx: DrawContext) -> Result<()> {
        Ok(())
    }

    /// Opt a custom widget into general pointer/key routing. Builder listeners
    /// need no opt-in; their bindings are stored outside the widget itself.
    fn accepts_events(&self) -> bool {
        false
    }
    /// Custom widgets can retain EventHandler<E> fields and dispatch them here,
    /// reusing async ownership and errors without implementing an executor.
    fn on_event_with_tasks(
        &mut self,
        event: &Event,
        _runtime: &crate::TaskRuntime,
    ) -> super::event::EventResponse {
        self.on_event(event).into()
    }

    /// Widgets without interaction allow events to continue propagating.
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Unhandled
    }
}

/// Result of applying new inputs to a retained widget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidgetUpdate {
    /// No layout or paint work is required for the widget's own inputs.
    Unchanged,
    /// Inputs were applied and may affect layout or drawing.
    Changed,
    /// Replace the widget with the new description.
    Replace,
}

new_key_type! {
    pub struct WidgetId;
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct WidgetStatus: u8 {
        const Hover = 0b0000_0001;
        const Active = 0b0000_0010;
        const Focused = 0b0000_0100;
    }
}

impl WidgetStatus {
    pub fn set_hovered(&mut self, hovered: bool) {
        self.set(Self::Hover, hovered)
    }

    pub fn is_hovered(&self) -> bool {
        self.contains(Self::Hover)
    }

    pub fn set_active(&mut self, active: bool) {
        self.set(Self::Active, active)
    }

    pub fn is_active(&self) -> bool {
        self.contains(Self::Active)
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.set(Self::Focused, focused)
    }

    pub fn is_focused(&self) -> bool {
        self.contains(Self::Focused)
    }
}

pub struct WidgetBuilder<W> {
    pub events: super::interaction::EventBindings,
    pub widget: W,
    pub props: ElementProps,
    pub children: Vec<Element>,
}

impl<W> WidgetBuilder<W> {
    /// Apply a conditional configuration without evaluating the unused branch.
    /// This changes the description; removing children unmounts their state.
    pub fn when(self, condition: bool, configure: impl FnOnce(Self) -> Self) -> Self {
        if condition { configure(self) } else { self }
    }

    crate::core::interaction::event_methods!();

    /// Mark a native control or drag region. Client excludes interactive content from dragging.
    pub fn window_control_area(mut self, area: super::decoration::WindowControlArea) -> Self {
        self.props.window_control_area = Some(area);
        self
    }

    /// Build a custom widget with empty events and default element properties.
    pub fn from_widget(widget: W) -> Self {
        Self {
            widget,
            events: Default::default(),
            props: ElementProps::new(Default::default()),
            children: Vec::new(),
        }
    }

    /// Compatibility alias; all new event APIs accept async closures directly.
    #[deprecated(note = "use on_click with the same async closure")]
    pub fn on_click_async<R: crate::tasks::HandlerOutput>(
        self,
        callback: impl AsyncFn() -> R + 'static,
    ) -> Self {
        self.on_click::<super::interaction::mode::AsyncNoArgs>(callback)
    }

    /// Override the CSS element name (normalized to ASCII lowercase).
    pub fn tag(mut self, tag: impl Into<SmolStr>) -> Self {
        self.props.tag = tag.into().to_ascii_lowercase().into();
        self
    }
    /// Set a selector-visible attribute. id/class use their canonical fields.
    pub fn attr(mut self, name: impl Into<SmolStr>, value: impl Into<SmolStr>) -> Self {
        let name = name.into().to_ascii_lowercase();
        let value = value.into();
        match name.as_str() {
            "id" => self.props.id = Some(value),
            "class" => {
                self.props.classes.clear();
                return self.class(value);
            }
            _ => {
                self.props.attributes.insert(name.into(), value);
            }
        }
        self
    }
    pub fn id(mut self, id: impl Into<SmolStr>) -> Self {
        self.props.id = Some(id.into());
        self
    }

    pub fn key(mut self, key: impl Into<SmolStr>) -> Self {
        self.props.key = Some(key.into());
        self
    }

    /// Alias for class, useful when sharing component authoring conventions.
    pub fn add_class(self, class: impl Into<SmolStr>) -> Self {
        self.class(class)
    }

    pub fn class(mut self, class: impl Into<SmolStr>) -> Self {
        let classes = class.into();
        for class in classes.split_ascii_whitespace() {
            if !self.props.classes.iter().any(|v| v == class) {
                self.props.classes.push(class.into());
            }
        }
        self
    }
}

impl<W> IntoElement for WidgetBuilder<W>
where
    W: Widget + 'static,
{
    fn into_element(mut self) -> Element {
        if self.props.tag == "widget" {
            self.props.tag = self.widget.tag_name().into();
        }
        Element {
            events: self.events,
            kind: super::element::ElementKind::Widget(Box::new(self.widget)),
            props: self.props,
            children: self.children,
        }
    }
}
