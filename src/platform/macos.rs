use winit::{
    event_loop::{ActiveEventLoop, EventLoopBuilder},
    platform::macos::{ActivationPolicy, ActiveEventLoopExtMacOS, EventLoopBuilderExtMacOS},
};

pub(super) fn configure_event_loop<T>(builder: &mut EventLoopBuilder<T>) {
    // Keep AppKit window operations on Winit's main-thread application loop.
    builder
        .with_activation_policy(ActivationPolicy::Regular)
        .with_default_menu(true);
}

pub(super) fn resumed(event_loop: &ActiveEventLoop) {
    // Native tab ownership is not part of this runtime. Let each declared window
    // retain its own surface, size, and scene instead of being auto-tabbed by macOS.
    event_loop.set_allows_automatic_window_tabbing(false);
}

use crate::core::geometry::{Point, Rect};
use objc2::{MainThreadMarker, rc::Retained};
use objc2_app_kit::{NSView, NSWindow, NSWindowButton};
use objc2_foundation::{NSPoint, NSRect, NSUserDefaults, ns_string};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::sync::Arc;
use winit::window::Window;

pub(super) struct Chrome {
    // Retain the Winit owner for every borrowed AppKit view. All calls stay on
    // the application's main thread, including teardown and fullscreen changes.
    owner: Arc<Window>,
    position: Option<Point<f32>>,
    original: Option<([NSRect; 3], NSRect)>,
}
fn native_view(window: &Window) -> Option<&NSView> {
    MainThreadMarker::new()?;
    let RawWindowHandle::AppKit(handle) = window.window_handle().ok()?.as_raw() else {
        return None;
    };
    // SAFETY: Winit's AppKit handle is a live NSView; the borrow cannot outlive window.
    Some(unsafe { handle.ns_view.cast::<NSView>().as_ref() })
}
fn buttons(window: &NSWindow) -> Option<[Retained<objc2_app_kit::NSButton>; 3]> {
    Some([
        window.standardWindowButton(NSWindowButton::CloseButton)?,
        window.standardWindowButton(NSWindowButton::MiniaturizeButton)?,
        window.standardWindowButton(NSWindowButton::ZoomButton)?,
    ])
}
impl Chrome {
    pub fn new(owner: &Arc<Window>, position: Option<Point<f32>>) -> Self {
        Self {
            owner: owner.clone(),
            position,
            original: None,
        }
    }
    pub fn update(&mut self, window: &Window) -> Option<Rect<f32>> {
        let view = native_view(&self.owner)?;
        let native = view.window()?;
        // Fetch live buttons each time: AppKit may replace them on fullscreen transitions.
        let buttons = buttons(&native)?;
        // SAFETY: Main-thread AppKit access; the returned parent is retained.
        let container = unsafe { buttons[0].superview() }?;
        if window.fullscreen().is_some() {
            if let Some((frames, parent)) = self.original.take() {
                container.setFrame(parent);
                for (button, frame) in buttons.iter().zip(frames) {
                    button.setFrame(frame);
                }
            }
            return None;
        }
        if let Some(position) = self.position {
            let (frames, _) = *self
                .original
                .get_or_insert_with(|| (buttons.each_ref().map(|b| b.frame()), container.frame()));
            let spacing = frames[1].origin.x - frames[0].origin.x;
            let height = frames[0].size.height + 2.0 * f64::from(position.y);
            let mut frame = container.frame();
            // Preserve the container's top edge while extending the content under it.
            frame.origin.y += frame.size.height - height;
            frame.size.height = height;
            if container.frame() != frame {
                container.setFrame(frame);
            }
            for (index, button) in buttons.iter().enumerate() {
                let point = NSPoint::new(
                    f64::from(position.x) + spacing * index as f64,
                    f64::from(position.y),
                );
                if button.frame().origin != point {
                    button.setFrameOrigin(point);
                }
            }
        }
        let mut min = Point::new(f32::INFINITY, f32::INFINITY);
        let mut max = Point::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
        for button in &buttons {
            if button.isHidden() {
                continue;
            }
            let rect = button.convertRect_toView(button.bounds(), Some(view));
            let y = if view.isFlipped() {
                rect.origin.y
            } else {
                view.bounds().size.height - rect.origin.y - rect.size.height
            };
            min.x = min.x.min(rect.origin.x as f32);
            min.y = min.y.min(y as f32);
            max.x = max.x.max((rect.origin.x + rect.size.width) as f32);
            max.y = max.y.max((y + rect.size.height) as f32);
        }
        min.x
            .is_finite()
            .then(|| Rect::from_xyxy(min.x, min.y, max.x, max.y))
    }
}
pub(super) fn titlebar_double_click(window: &Window) {
    let Some(view) = native_view(window) else {
        return;
    };
    let Some(native) = view.window() else {
        return;
    };
    let action =
        NSUserDefaults::standardUserDefaults().stringForKey(ns_string!("AppleActionOnDoubleClick"));
    match action.as_ref().map(|value| value.to_string()).as_deref() {
        Some("Minimize") => {
            if window
                .enabled_buttons()
                .contains(winit::window::WindowButtons::MINIMIZE)
            {
                native.performMiniaturize(None);
            }
        }
        Some("None") => {}
        _ => {
            if window.is_resizable() {
                native.performZoom(None);
            }
        }
    }
}
