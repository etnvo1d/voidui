//! Desktop-specific policy. Window ownership and event translation stay in Winit.
use winit::{
    event_loop::{ActiveEventLoop, EventLoopBuilder},
    window::WindowAttributes,
};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

pub(crate) fn configure_event_loop<T>(builder: &mut EventLoopBuilder<T>) {
    #[cfg(target_os = "macos")]
    macos::configure_event_loop(builder);
    #[cfg(target_os = "windows")]
    windows::configure_event_loop(builder);
    let _ = builder;
}

pub(crate) fn resumed(event_loop: &ActiveEventLoop) {
    #[cfg(target_os = "macos")]
    macos::resumed(event_loop);
    let _ = event_loop;
}

pub(crate) fn window_attributes(attributes: WindowAttributes, app_id: &str) -> WindowAttributes {
    #[cfg(target_os = "linux")]
    return linux::window_attributes(attributes, app_id);
    #[cfg(not(target_os = "linux"))]
    {
        let _ = app_id;
        attributes
    }
}

/// AppKit's resize callback needs synchronous transaction-bound drawing.
pub(crate) const SYNCHRONOUS_RESIZE: bool = cfg!(target_os = "macos");

/// Wayland controls mapping through buffer commits; do not defer its first map.
pub(crate) const HIDE_UNTIL_SCENE_READY: bool =
    cfg!(any(target_os = "macos", target_os = "windows"));

pub(crate) fn prepare_present(renderer: &voidui_gpui_wgpu::WgpuRenderer, synchronous: bool) {
    #[cfg(target_os = "macos")]
    renderer.set_presents_with_transaction(synchronous);
    let _ = (renderer, synchronous);
}

/// The native UI family used by Parley's system-font alias.
pub(crate) fn system_font() -> &'static str {
    #[cfg(target_os = "macos")]
    return "Helvetica";
    #[cfg(target_os = "windows")]
    return "Segoe UI";
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    return "DejaVu Sans";
}

pub(crate) mod clipboard;

pub(crate) mod decoration;

#[cfg(target_os = "linux")]
pub(crate) use linux::watch_button_layout;
