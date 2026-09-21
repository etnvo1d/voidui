//! A small titlebar composition. Native policy lives in core/platform modules;
//! all client-drawn controls use normal CSS, focus and click dispatch.
use super::div::Div;
use crate::core::{layout::AlignItems, widget::WidgetBuilder};
use crate::{IntoElement, WindowDecorations, component, div, window_context};
#[cfg(not(target_os = "macos"))]
use crate::{WindowButton, WindowState};

/// Wrap arbitrary content with platform window controls. Use `.class()` and CSS
/// to style `.window-controls`, `.window-control` and `[data-window-button]`.
/// Set WindowOptions::decorations to Custom to replace the system titlebar.
pub fn title_bar(content: impl IntoElement) -> WidgetBuilder<Div> {
    div()
        .class("title-bar")
        .flex_row()
        .align_items(AlignItems::STRETCH)
        .flex_shrink(0.0)
        .user_select(crate::style::selection::UserSelect::None)
        .window_control_area(crate::WindowControlArea::Drag)
        .child(component(|| controls(true)))
        .child(
            div()
                .class("title-bar-content")
                .flex_row()
                .align_items(AlignItems::CENTER)
                .flex_grow(1.0)
                .min_width(0.0)
                .child(content),
        )
        .child(component(|| controls(false)))
}
fn controls(left: bool) -> WidgetBuilder<Div> {
    let state = window_context().state().unwrap_or_default();
    let mut row = div()
        .class("window-controls")
        .flex_row()
        .flex_shrink(0.0)
        .height(state.titlebar.height);
    if state.fullscreen || state.decorations != WindowDecorations::Custom {
        return row;
    }
    if let Some(bounds) = state.native_controls {
        if left {
            // Reserve the measured button group plus its native leading margin.
            // No OS-specific button width or fixed traffic-light offset is needed.
            row = row.width(bounds.origin.x + bounds.size.width + bounds.origin.x);
        }
        return row;
    }
    // AppKit may temporarily remove buttons during a fullscreen transition.
    #[cfg(target_os = "macos")]
    {
        let _ = left;
        return row;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let buttons = if left {
            &state.button_layout.left
        } else {
            &state.button_layout.right
        };
        for button in buttons {
            if (*button == WindowButton::Minimize && !state.minimizable)
                || (*button == WindowButton::Maximize && !state.resizable)
            {
                continue;
            }
            row = row.child(control(*button, &state));
        }
        row
    }
}
#[cfg(not(target_os = "macos"))]
fn control(button: WindowButton, state: &WindowState) -> impl IntoElement {
    // Vector icons are independent of installed fonts. CSS currentColor follows
    // user styling, including hover/active and high-contrast application themes.
    let (label, path) = match button {
        WindowButton::Minimize => ("Minimize", "M3 8H13"),
        WindowButton::Maximize if state.maximized => ("Restore", "M5 5V3H13V11H11 M3 5H11V13H3Z"),
        WindowButton::Maximize => ("Maximize", "M3 3H13V13H3Z"),
        WindowButton::Close => ("Close", "M3 3L13 13 M13 3L3 13"),
    };
    let source = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="{path}" fill="none" stroke="currentColor" stroke-width="1"/></svg>"#
    );
    div()
        .tag("button")
        .class("window-control")
        .attr("data-window-button", button.name())
        .attr("aria-label", label)
        .attr("tabindex", "0")
        .window_control_area(button.area())
        .width(state.titlebar.height)
        .height(state.titlebar.height)
        .flex_shrink(0.0)
        .flex_row()
        .align_items(AlignItems::CENTER)
        .justify_content(crate::core::layout::JustifyContent::CENTER)
        .child(
            crate::svg_from_str(&source)
                .expect("built-in window control SVG is valid")
                .width(16.0)
                .height(16.0),
        )
}
