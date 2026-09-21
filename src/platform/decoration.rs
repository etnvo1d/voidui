//! Winit window chrome adaptation. The native callback sees an immutable snapshot,
//! never a borrowed WidgetTree: Win32 can synchronously reenter during a resize.
use crate::{
    WindowDecorations, WindowState,
    core::{geometry::Rect, widget_tree::WidgetTree},
};
use std::sync::Arc;
use winit::window::{Window, WindowAttributes};

pub(crate) fn attributes(
    mut attributes: WindowAttributes,
    options: &crate::WindowOptions,
) -> WindowAttributes {
    let mut buttons = winit::window::WindowButtons::CLOSE;
    buttons.set(winit::window::WindowButtons::MINIMIZE, options.minimizable);
    buttons.set(winit::window::WindowButtons::MAXIMIZE, options.resizable);
    attributes = attributes
        .with_resizable(options.resizable)
        .with_enabled_buttons(buttons);
    match options.decorations {
        WindowDecorations::System => attributes,
        WindowDecorations::None => attributes.with_decorations(false),
        WindowDecorations::Custom => {
            #[cfg(target_os = "macos")]
            {
                use winit::platform::macos::WindowAttributesExtMacOS;
                attributes
                    .with_titlebar_transparent(true)
                    .with_title_hidden(true)
                    .with_fullsize_content_view(true)
            }
            #[cfg(target_os = "windows")]
            {
                // Keep the OS frame style; the subclass removes only the caption
                // geometry. This preserves native resizing, shadows and snapping.
                attributes
            }
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            {
                attributes.with_decorations(false)
            }
        }
    }
}

pub(crate) struct Decoration {
    #[cfg(target_os = "macos")]
    mac: Option<super::macos::Chrome>,
    #[cfg(target_os = "windows")]
    windows: Option<super::windows::Chrome>,
}
impl Decoration {
    pub fn new(window: &Arc<Window>, options: &crate::WindowOptions) -> anyhow::Result<Self> {
        let _ = (window, options);
        Ok(Self {
            #[cfg(target_os = "macos")]
            mac: (options.decorations == WindowDecorations::Custom).then(|| {
                super::macos::Chrome::new(window, options.titlebar.traffic_light_position)
            }),
            #[cfg(target_os = "windows")]
            windows: if options.decorations == WindowDecorations::Custom {
                Some(super::windows::Chrome::new(window)?)
            } else {
                None
            },
        })
    }
    pub fn native_controls(&mut self, window: &Window) -> Option<Rect<f32>> {
        #[cfg(target_os = "macos")]
        if let Some(mac) = &mut self.mac {
            return mac.update(window);
        }
        let _ = window;
        None
    }
    pub fn publish(&self, tree: &WidgetTree, state: &WindowState, changed: bool) {
        #[cfg(target_os = "windows")]
        if let Some(windows) = &self.windows {
            windows.publish(tree, state, changed);
        }
        let _ = (tree, state, changed);
    }
    pub fn invalidate(&self) {
        #[cfg(target_os = "windows")]
        if let Some(windows) = &self.windows {
            windows.invalidate();
        }
    }
}

pub(crate) fn double_click(window: &Window) {
    #[cfg(target_os = "macos")]
    {
        super::macos::titlebar_double_click(window);
    }
    #[cfg(not(target_os = "macos"))]
    {
        if window.is_resizable() {
            window.set_maximized(!window.is_maximized());
        }
    }
}

/// Client-side border hit testing is separate from titlebar content and disabled
/// in maximized/fullscreen windows. Windows uses native DPI metrics instead.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn resize_edge(
    point: crate::core::geometry::Point<f32>,
    size: crate::core::geometry::Size<f32>,
    border: f32,
) -> Option<winit::window::ResizeDirection> {
    use winit::window::ResizeDirection::*;
    if border <= 0.0
        || point.x < 0.0
        || point.y < 0.0
        || point.x >= size.width
        || point.y >= size.height
    {
        return None;
    }
    let x = if point.x < border {
        -1
    } else if point.x >= size.width - border {
        1
    } else {
        0
    };
    let y = if point.y < border {
        -1
    } else if point.y >= size.height - border {
        1
    } else {
        0
    };
    match (x, y) {
        (-1, -1) => Some(NorthWest),
        (1, -1) => Some(NorthEast),
        (-1, 1) => Some(SouthWest),
        (1, 1) => Some(SouthEast),
        (-1, 0) => Some(West),
        (1, 0) => Some(East),
        (0, -1) => Some(North),
        (0, 1) => Some(South),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::geometry::{Point, Size};
    #[test]
    fn resize_edges_cover_all_corners_without_grabbing_content() {
        use winit::window::ResizeDirection::*;
        let size = Size::new(100.0, 80.0);
        for (point, expected) in [
            (Point::new(1.0, 1.0), Some(NorthWest)),
            (Point::new(99.0, 1.0), Some(NorthEast)),
            (Point::new(1.0, 79.0), Some(SouthWest)),
            (Point::new(99.0, 79.0), Some(SouthEast)),
            (Point::new(50.0, 1.0), Some(North)),
            (Point::new(50.0, 79.0), Some(South)),
            (Point::new(1.0, 40.0), Some(West)),
            (Point::new(99.0, 40.0), Some(East)),
            (Point::new(50.0, 40.0), None),
            (Point::new(-1.0, 1.0), None),
        ] {
            assert_eq!(resize_edge(point, size, 5.0), expected);
        }
        assert_eq!(resize_edge(Point::new(0.0, 0.0), size, 0.0), None);
    }
    #[test]
    fn native_button_flags_follow_window_capabilities() {
        let options = crate::WindowOptions {
            resizable: false,
            minimizable: false,
            ..Default::default()
        };
        let attrs = attributes(WindowAttributes::default(), &options);
        assert!(!attrs.resizable);
        assert_eq!(attrs.enabled_buttons, winit::window::WindowButtons::CLOSE);
    }
    #[test]
    fn invalid_chrome_metrics_are_rejected_before_native_creation() {
        for height in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(
                crate::TitlebarOptions {
                    height,
                    ..Default::default()
                }
                .validate()
                .is_err()
            );
        }
        assert!(
            crate::TitlebarOptions {
                traffic_light_position: Some(Point::new(f32::NAN, 0.0)),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
}
