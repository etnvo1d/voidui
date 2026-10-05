//! An ordinary CSS container with delegated background focus. Addons are real
//! children and never enter the editor's document, clipboard, or undo history.
use super::container::Container;
use crate::{
    core::{
        context::LayoutContext,
        element::ElementProps,
        layout::{Display, LayoutInput, LayoutOutput},
        widget::{Widget, WidgetBuilder, WidgetUpdate},
    },
    style::style::Style,
};
#[derive(Default)]
pub struct InputGroup;
pub fn input_group() -> WidgetBuilder<InputGroup> {
    WidgetBuilder {
        events: Default::default(),
        widget: InputGroup,
        props: ElementProps::new(Style::default()),
        children: Vec::new(),
    }
    .class("input-group")
    .attr("role", "group")
}
impl Container for InputGroup {}
impl Widget for InputGroup {
    fn tag_name(&self) -> &'static str {
        "div"
    }
    fn delegates_focus(&self) -> bool {
        true
    }
    fn paints_content(&self) -> bool {
        false
    }
    fn default_style(&self) -> Option<Style> {
        let mut style = Style::default();
        style.layout.display = Display::Flex;
        Some(style)
    }
    fn reconcile(&mut self, _next: &dyn Widget) -> WidgetUpdate {
        WidgetUpdate::Unchanged
    }
    fn layout(&mut self, inputs: LayoutInput, cx: LayoutContext<'_, '_>) -> LayoutOutput {
        cx.layout_children(inputs)
    }
}
