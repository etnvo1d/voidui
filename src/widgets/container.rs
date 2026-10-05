//! Shared child construction for widgets that lay out their children.
use crate::core::{
    element::IntoElement,
    widget::{Widget, WidgetBuilder},
};

/// Opt a container widget into the `.child()` and `.children()` builder methods.
/// Custom containers can implement this trait and lay out children through
/// `LayoutContext::layout_children`.
pub trait Container: Widget {}

impl<W: Container> WidgetBuilder<W> {
    /// Append one child, preserving its component or widget identity.
    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_element());
        self
    }

    /// Append children in iterator order. Give moving component instances a key.
    pub fn children(mut self, children: impl IntoIterator<Item = impl IntoElement>) -> Self {
        self.children
            .extend(children.into_iter().map(IntoElement::into_element));
        self
    }
}
