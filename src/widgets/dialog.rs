use crate::core::{
    context::LayoutContext,
    layout::{LayoutInput, LayoutOutput},
    widget::{Widget, WidgetBuilder, WidgetUpdate},
};

use super::container::Container;

/// A dialog container with CSS defaults and explicit Rust-managed visibility.
#[derive(Default)]
pub struct Dialog;

/// Create an initially hidden dialog. Use `.attr("open", "")` for a non-modal
/// dialog, or `AppWindow::show_modal` / `WidgetTree::show_modal` for modality.
///
/// ```
/// use voidui::{button, dialog};
///
/// let panel = dialog()
///     .attr("open", "")
///     .child("Settings")
///     .child(button().child("Apply"));
/// ```
pub fn dialog() -> WidgetBuilder<Dialog> {
    WidgetBuilder::from_widget(Dialog)
}

impl Container for Dialog {}

impl Widget for Dialog {
    fn tag_name(&self) -> &'static str {
        "dialog"
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
