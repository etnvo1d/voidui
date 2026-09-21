//! Custom native chrome with normal UI content. Run with --smoke to verify two
//! windows, native state changes, and retained-scene idle behavior automatically.
use std::{
    borrow::Cow,
    sync::Arc,
    time::{Duration, Instant},
};
use voidui::core::{
    geometry::{Point, Size},
    layout::AlignItems,
};
use voidui::render::{ParleyTextSystem, TextSystem};
use voidui::{
    Application, TitlebarOptions, WindowDecorations, WindowOptions, component, div, text,
    title_bar, window_context,
};

fn content() -> impl voidui::IntoElement {
    div()
        .width(voidui::style::pct(100.0))
        .height(voidui::style::pct(100.0))
        .flex_col()
        .class("window")
        .child(title_bar(
            div()
                .flex_row()
                .align_items(AlignItems::CENTER)
                .gap(18.0)
                .child(text("VOIDUI"))
                .child(
                    div()
                        .class("toolbar-button")
                        .attr("tabindex", "0")
                        .on_click(|| println!("Toolbar click; the window did not move"))
                        .child("Toolbar action"),
                )
                .child(component(|| {
                    let context = window_context();
                    let state = context.state().unwrap_or_default();
                    text(if state.focused { "Active" } else { "Inactive" }).class("status")
                })),
        ))
        .child(
            div()
                .padding(24.0)
                .child(text("Your content, native window behavior.").font_size(24.0))
                .child("Drag empty titlebar space. Double-click it to resize the window.")
                .child(
                    "Window controls follow the desktop layout. Toolbar actions remain clickable.",
                ),
        )
}
fn main() -> anyhow::Result<()> {
    let smoke = std::env::args().any(|arg| arg == "--smoke");
    let fonts = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    fonts.add_fonts(vec![Cow::Borrowed(include_bytes!(
        "../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
    ))])?;
    let options = WindowOptions {
        title: "Voidui custom titlebar".into(),
        decorations: WindowDecorations::Custom,
        titlebar: TitlebarOptions {
            traffic_light_position: Some(Point::new(12.0, 11.0)),
            height: 40.0,
            ..Default::default()
        },
        size: Size::new(800.0, 460.0),
        min_size: Some(Size::new(360.0, 200.0)),
        ..Default::default()
    };
    let app = Application::new()
        .on_window_event(|event, window| {
            if let winit::event::WindowEvent::KeyboardInput { event, .. } = event {
                if event.state == winit::event::ElementState::Pressed
                    && !event.repeat
                    && event.logical_key
                        == winit::keyboard::Key::Named(winit::keyboard::NamedKey::F11)
                {
                    let fullscreen = window
                        .native_window()
                        .fullscreen()
                        .is_none()
                        .then_some(winit::window::Fullscreen::Borderless(None));
                    window.native_window().set_fullscreen(fullscreen);
                }
            }
            Ok(())
        })
        .text_system(Arc::new(TextSystem::new(Arc::new(fonts))))
        .css(
            r#"
            .window { background: #17212b; color: #e4ecf4; font-size: 14px; }
            .title-bar { background: #243444; }
            .title-bar-content { padding: 0 12px; }
            .toolbar-button { padding: 5px 10px; border-radius: 4px; }
            .toolbar-button:hover, .window-control:hover { background: #405466; }
            .window-control:active { background: #526779; }
            .window-control[data-window-button=close]:hover { background: #c52b37; color: white; }
            .window-control:focus { border: 1px solid #88bbee; }
            .status { color: #a5b8c9; }
        "#,
        )?;
    if !smoke {
        return app.window(options, content()).run();
    }
    struct SmokeWindow {
        stage: u8,
        since: Instant,
        baseline: (u64, u64),
        context: voidui::WindowContext,
    }
    fn wake_after(window: &voidui::AppWindow, delay: Duration) {
        let native = window.native_window().clone();
        window.task_scope().spawn(async move {
            voidui::tasks::time::sleep(delay).await.unwrap();
            native.request_redraw();
        });
    }
    let mut states = std::collections::HashMap::new();
    app.window(options.clone(), content())
        .window(
            WindowOptions {
                title: "Voidui independent titlebar".into(),
                ..options
            },
            content(),
        )
        .on_frame(move |window| {
            let id = window.native_window().id();
            let entry = states.entry(id).or_insert_with(|| SmokeWindow {
                stage: 0,
                since: Instant::now(),
                baseline: (0, 0),
                context: window.window_context(),
            });
            match entry.stage {
                0 => {
                    println!("initial {id:?}: {:?}", window.window_state());
                    let _ = window
                        .native_window()
                        .request_inner_size(winit::dpi::LogicalSize::new(700.0, 400.0));
                    entry.stage = 1;
                    wake_after(window, Duration::from_millis(300));
                }
                1 if entry.since.elapsed() >= Duration::from_millis(300) => {
                    #[cfg(target_os = "macos")]
                    {
                        let bounds = window
                            .window_state()
                            .native_controls
                            .expect("native traffic lights have measured bounds");
                        anyhow::ensure!(
                            (bounds.origin.y - 11.0).abs() < 1.0,
                            "resize must preserve traffic-light placement: {bounds:?}"
                        );
                    }
                    entry.stage = 2;
                    entry.since = Instant::now();
                    entry.baseline = (window.stats().layout_passes, window.stats().scene_builds);
                    wake_after(window, Duration::from_secs(2));
                }
                2 if entry.since.elapsed() >= Duration::from_secs(2) => {
                    anyhow::ensure!(
                        (window.stats().layout_passes, window.stats().scene_builds)
                            == entry.baseline,
                        "idle exposure must reuse layout and scene: {:?}",
                        window.stats()
                    );
                    println!("smoke ok {id:?}: {:?}", window.stats());
                    entry.stage = 3;
                }
                _ => {}
            }
            // Closing one window activates the other. Check both idle intervals
            // before closing either, so focus changes cannot invalidate the baseline.
            if states.len() == 2 && states.values().all(|s| s.stage == 3) {
                let contexts: Vec<_> = states
                    .values_mut()
                    .map(|entry| {
                        entry.stage = 4;
                        entry.context.request(voidui::WindowAction::Minimize);
                        entry.context.clone()
                    })
                    .collect();
                window.task_scope().spawn(async move {
                    // A minimized window cannot present; queued close actions must
                    // still be drained by the event loop's task wakeup.
                    voidui::tasks::time::sleep(Duration::from_millis(200))
                        .await
                        .unwrap();
                    for context in contexts {
                        context.request(voidui::WindowAction::Close);
                    }
                });
            }
            Ok(())
        })
        .run()
}
