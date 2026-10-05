use crate::core::{
    context::LayoutContext,
    layout::{LayoutInput, LayoutOutput},
    widget::{Widget, WidgetBuilder, WidgetUpdate},
};

use super::container::Container;

/// A focusable button container. Appearance is controlled by CSS and fluent styles.
#[derive(Default)]
pub struct Button;

/// Create a button with pointer/Tab focus and non-selectable content by default.
/// Register `.on_click(...)` for pointer, keyboard, and programmatic activation.
///
/// ```
/// use voidui::button;
///
/// let action = button().child("Save").on_click(|| println!("Saved"));
/// ```
pub fn button() -> WidgetBuilder<Button> {
    WidgetBuilder::from_widget(Button)
}

impl Container for Button {}

impl Widget for Button {
    fn tag_name(&self) -> &'static str {
        "button"
    }

    fn reconcile(&mut self, _next: &dyn Widget) -> WidgetUpdate {
        WidgetUpdate::Unchanged
    }

    fn paints_content(&self) -> bool {
        false
    }

    fn layout(&mut self, inputs: LayoutInput, ctx: LayoutContext<'_, '_>) -> LayoutOutput {
        ctx.layout_children(inputs)
    }
}
