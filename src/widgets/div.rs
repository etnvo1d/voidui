use crate::core::{
    context::LayoutContext,
    layout::{LayoutInput, LayoutOutput},
    widget::{Widget, WidgetBuilder},
};

use super::container::Container;

/// A CSS container. Layout settings live in the element's style, not the widget.
#[derive(Default)]
pub struct Div;

impl Div {
    pub fn new() -> Self {
        Self
    }
}

/// Create a block container; use `.flex()` or `.grid()` to select another mode.
pub fn div() -> WidgetBuilder<Div> {
    WidgetBuilder::new()
}

impl Default for WidgetBuilder<Div> {
    fn default() -> Self {
        Self::new()
    }
}

impl WidgetBuilder<Div> {
    pub fn new() -> Self {
        Self::from_widget(Div::new())
    }
}

impl Container for Div {}

impl Widget for Div {
    fn reconcile(&mut self, _next: &dyn Widget) -> crate::core::widget::WidgetUpdate {
        crate::core::widget::WidgetUpdate::Unchanged
    }

    fn paints_content(&self) -> bool {
        false
    }

    fn tag_name(&self) -> &'static str {
        "div"
    }
    fn layout(&mut self, inputs: LayoutInput, ctx: LayoutContext<'_, '_>) -> LayoutOutput {
        ctx.layout_children(inputs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IntoElement;
    use crate::core::layout::{Display, length};

    #[test]
    fn builder_keeps_layout_in_element_style() {
        let element = div()
            .flex()
            .gap(1.0)
            .width(100.0)
            .class("test")
            .into_element();
        assert_eq!(element.props.style.layout.display, Display::Flex);
        assert_eq!(element.props.style.layout.gap.width, length(1.0));
        assert_eq!(element.props.style.layout.size.width, length(100.0));
    }
}
