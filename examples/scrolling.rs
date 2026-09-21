//! Standard CSS scrolling with Rust-only placement policy.
//! Run `cargo run --example scrolling`, or `--smoke /tmp/scrolling.png` for
//! native wheel/drag delivery, paint-only frame checks, idle checks and a snapshot.
use anyhow::Result;
use std::{borrow::Cow, sync::Arc, time::Duration};
use voidui::{
    Application, MouseButton, ScrollAxis, ScrollbarMode, WindowOptions,
    core::geometry::Size,
    div,
    render::{ParleyTextSystem, TextSystem},
    style::{css::Stylesheet, pct},
    text,
};

fn pane(
    id: &str,
    title: &str,
    description: &str,
    mode: ScrollbarMode,
    rows: usize,
) -> impl voidui::IntoElement {
    let mut content = div().class("rows");
    for row in 0..rows {
        content = content.child(
            div()
                .class("row")
                .child(text(format!("{:03}", row + 1)).class("number"))
                .child(text(format!(
                    "Retained row {} — scroll with wheel, trackpad or thumb.",
                    row + 1
                ))),
        );
    }
    div()
        .class("panel")
        .child(text(title).class("title"))
        .child(text(description).class("description"))
        .child(
            div()
                .id(id)
                .class("viewport")
                .attr("tabindex", "0")
                .scrollbar_mode(mode)
                .child(content),
        )
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let snapshot = match args.as_slice() {
        [] => None,
        [flag, path] if flag == "--smoke" => Some(path.clone()),
        _ => anyhow::bail!("usage: scrolling [--smoke output.png]"),
    };
    let fonts = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    fonts.add_fonts(vec![Cow::Borrowed(include_bytes!(
        "../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
    ))])?;
    let root=div().class("page").width(pct(100.0)).height(pct(100.0))
        .child(text("Scrolling, without continuous work").class("heading"))
        .child(text("CSS controls overflow, color, width and gutters. Rust selects classic or overlay scrollbars.").class("intro"))
        .child(div().class("panels")
            .child(pane("overlay", "Overlay", "No layout space reserved", ScrollbarMode::Overlay, 200))
            .child(pane("classic", "Classic", "Standard stable both-edges gutters", ScrollbarMode::Classic, 200)))
        .child(text("Click a panel for keyboard scrolling. Try Home, End, Page Up / Down and Shift + wheel.").class("footer"));
    let mut app=Application::new().text_system(Arc::new(TextSystem::new(Arc::new(fonts))))
        .window(WindowOptions { title:"VoidUI — scrolling".into(), size:Size::new(1000.,660.), ..Default::default() },root)
        .stylesheet(Stylesheet::parse(r#"
            .page {box-sizing:border-box; padding:28px; background:#101722; color:#d9e5f5; font-family:"IBM Plex Sans";font-size:15px;line-height:24px;}
            .heading{font-size:30px;line-height:42px;}
            .intro{color:#9daec6;margin-bottom:24px;}
            .panels{display:flex;gap:22px;}
            .panel{flex:1;min-width:0;}
            .title{font-size:21px;line-height:30px;color:#a3d9f7;}
            .description{color:#9daec6;margin-bottom:12px;}
            .viewport{height:390px;overflow:auto;background:#172332;border:1px solid #31445b;scrollbar-width:thin;scrollbar-color:#80c9de #24384a;}
            #classic{scrollbar-gutter:stable both-edges;}
            .rows{width:600px;}
            .row{height:38px;display:flex;align-items:center;gap:14px;padding:0 12px;}
            .row:nth-child(even){background:#1c2b3d;}
            .number{width:34px;color:#7494b7;font-size:13px;}
            .footer{color:#9daec6;margin-top:22px;font-size:13px;}
        "#)?);
    if let Some(path) = snapshot {
        app = smoke_test(app, path);
    }
    app.run()
}

/// The idle assertion observes a quiet native interval. Real OS/user activity
/// restarts the observation; scene changes without such activity still fail.
fn smoke_test(mut app: Application, path: String) -> Application {
    use std::{cell::Cell, rc::Rc};
    let activity = Rc::new(Cell::new(0usize));
    let events = activity.clone();
    app = app.on_window_event(move |event, _| {
        if !matches!(event, winit::event::WindowEvent::RedrawRequested) {
            events.set(events.get().wrapping_add(1));
        }
        Ok(())
    });
    let mut stage = 0;
    let mut baseline = None;
    let mut wakeups = None;
    let mut idle_activity = None;
    let check = move |window: &mut voidui::AppWindow| -> Result<()> {
        let id = window.tree().find_by_id("overlay").unwrap();
        let scale = window.native_window().scale_factor();
        let pointer = |x: f32, y: f32| {
            Some(winit::dpi::PhysicalPosition::new(
                f64::from(x) * scale,
                f64::from(y) * scale,
            ))
        };
        match stage {
            0 => {
                baseline = Some(window.stats());
                let b = window.tree().bounds(id);
                window.pointer_moved(pointer(b.origin.x + 30., b.origin.y + 30.));
                window.mouse_scroll(winit::event::MouseScrollDelta::PixelDelta(
                    winit::dpi::PhysicalPosition::new(0., -90. * scale),
                ));
                stage = 1;
            }
            1 => {
                anyhow::ensure!(
                    window.tree().scroll_metrics(id).unwrap().offset.y == 90.,
                    "native wheel did not reach the scrollport"
                );
                anyhow::ensure!(
                    window.stats().layout_passes == baseline.unwrap().layout_passes,
                    "wheel caused layout"
                );
                let g = window
                    .tree()
                    .scrollbar_geometry(id, ScrollAxis::Vertical)
                    .unwrap();
                let x = g.thumb.origin.x + g.thumb.size.width / 2.;
                let y = g.thumb.origin.y + g.thumb.size.height / 2.;
                window.pointer_moved(pointer(x, y));
                window.mouse_button(MouseButton::Left, true);
                anyhow::ensure!(
                    window.tree().pointer_capture() == Some(id),
                    "thumb did not capture pointer"
                );
                window.pointer_moved(pointer(x, y + 70.));
                window.mouse_button(MouseButton::Left, false);
                anyhow::ensure!(
                    window.tree().pointer_capture().is_none(),
                    "thumb did not release capture"
                );
                stage = 2;
            }
            2 => {
                anyhow::ensure!(
                    window.tree().scroll_metrics(id).unwrap().offset.y > 90.,
                    "native drag did not move content"
                );
                anyhow::ensure!(
                    window.stats().layout_passes == baseline.unwrap().layout_passes,
                    "drag caused layout"
                );
                let handle = window.native_window().clone();
                let (tx, rx) = std::sync::mpsc::channel();
                wakeups = Some(rx);
                // Bounded test-only wakeups; production scrollbars have no timer.
                std::thread::spawn(move || {
                    for tick in 0..6 {
                        std::thread::sleep(Duration::from_secs(1));
                        if tx.send(tick).is_err() {
                            break;
                        }
                        handle.request_redraw();
                    }
                });
                stage = 3;
            }
            3 => {
                if let Some(tick) = wakeups.as_ref().and_then(|rx| rx.try_recv().ok()) {
                    let epoch = activity.get();
                    if idle_activity != Some(epoch) {
                        anyhow::ensure!(
                            tick < 5,
                            "native activity prevented a quiet idle interval"
                        );
                        idle_activity = Some(epoch);
                        baseline = Some(window.stats());
                        return Ok(());
                    }
                    let before = baseline.unwrap();
                    let after = window.stats();
                    anyhow::ensure!(
                        after.layout_passes == before.layout_passes
                            && after.scene_builds == before.scene_builds,
                        "idle rebuilt content: before={before:?} after={after:?}"
                    );
                    anyhow::ensure!(
                        after.presented_frames == before.presented_frames + 1,
                        "unexpected idle frames"
                    );
                    let (size, pixels) = window.snapshot()?;
                    let file = std::io::BufWriter::new(std::fs::File::create(&path)?);
                    let mut encoder = png::Encoder::new(file, size.width, size.height);
                    encoder.set_color(png::ColorType::Rgba);
                    encoder.set_depth(png::BitDepth::Eight);
                    encoder.write_header()?.write_image_data(&pixels)?;
                    println!(
                        "PASS native wheel, thumb capture/drag/release, no scroll relayout, idle scene reuse; snapshot={path} stats={:?}",
                        window.stats()
                    );
                    window.close();
                }
            }
            _ => (),
        }
        Ok(())
    };
    app.on_frame(check)
}
