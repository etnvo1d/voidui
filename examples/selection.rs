//! `cargo run --example selection`: drag, double/triple-click, Shift-click,
//! Cmd/Ctrl+A and Cmd/Ctrl+C. `--smoke directory` and `--pixels` are diagnostics.
use anyhow::Result;
use std::{borrow::Cow, path::Path, sync::Arc, time::Duration};
use voidui::{
    Application, WindowOptions,
    core::{
        geometry::{Point, Size},
        selection::SelectionPoint as P,
    },
    div,
    render::{ParleyTextSystem, TextSystem},
    text,
};
fn snapshot(window: &mut voidui::AppWindow, path: &Path) -> Result<()> {
    let (size, pixels) = window.snapshot()?;
    let mut encoder = png::Encoder::new(
        std::io::BufWriter::new(std::fs::File::create(path)?),
        size.width,
        size.height,
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&pixels)?;
    Ok(())
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.as_slice() == ["--pixels"] {
        return pixels();
    }
    let output = match args.as_slice() {
        [] => None,
        [mode, path] if mode == "--smoke" => {
            std::fs::create_dir_all(path)?;
            Some(std::path::PathBuf::from(path))
        }
        _ => anyhow::bail!("usage: selection [--smoke directory | --pixels]"),
    };
    let content=div().id("app")
        .child(text("Select text, not widgets.").class("title ui"))
        .child(text("Drag across paragraphs. Double-click a word, triple-click a paragraph. Shift-click extends; Cmd/Ctrl+C copies.").class("instructions ui"))
        .child(text("Selection follows logical document order while its geometry comes from shaped glyphs. Soft wrapping does not insert extra newlines into copied text.").id("paragraph"))
        .child(div().id("none").child("user-select: none — this row cannot start or erase a selection."))
        .child(div().id("all").child(text("user-select: all — select this entire block atomically.").id("atomic")))
        .child(div().id("contain").child(text("user-select: contain — a drag started here stays inside this block.").id("contained")))
        .child(text("Unicode: 中文 · Cafe\u{301} · 👩‍💻 · abc אבג xyz").id("unicode"))
        .child(div().tag("button").id("copy").child("Copy selection"));
    let mut cursor = None;
    let app = Application::new()
        .css(include_str!("styles/selection.css"))?
        .window(
            WindowOptions {
                title: "voidui — text selection".into(),
                size: Size::new(820.0, 670.0),
                ..Default::default()
            },
            content,
        )
        .on_window_event(move |event, window| {
            use winit::event::{ElementState, MouseButton, WindowEvent};
            if let WindowEvent::CursorMoved { position, .. } = event {
                let scale = window.native_window().scale_factor();
                cursor = Some(Point::new(
                    (position.x / scale) as f32,
                    (position.y / scale) as f32,
                ));
            }
            if matches!(
                event,
                WindowEvent::MouseInput {
                    state: ElementState::Released,
                    button: MouseButton::Left,
                    ..
                }
            ) && let Some(voidui::core::top_layer::HitTarget::Element(mut id)) =
                cursor.and_then(|p| window.tree().hit_test(p))
            {
                loop {
                    if window.tree().attribute(id, "id").as_deref() == Some("copy") {
                        window.copy_selection()?;
                        break;
                    }
                    if let Some(parent) = window.tree().parent(id) {
                        id = parent;
                    } else {
                        break;
                    }
                }
            }
            Ok(())
        });
    let mut stage = 0;
    let mut before = None;
    let mut idle = None;
    let mut wake = None;
    let app = if let Some(output) = output {
        app.on_frame(move |window| {
            match stage {
                0 => {
                    before = Some(window.stats());
                    let id = window.tree().find_by_id("paragraph").unwrap();
                    window.tree_mut().set_selection(P::text(id, 0), P::text(id, 48))?;
                    stage = 1;
                }
                1 => {
                    anyhow::ensure!(window.stats().layout_passes == before.unwrap().layout_passes, "selection caused layout");
                    anyhow::ensure!(window.tree().selected_text() == "Selection follows logical document order while i", "incorrect selected substring");
                    snapshot(window, &output.join("selected.png"))?;
                    let id = window.tree().find_by_id("atomic").unwrap();
                    let mut p = window.tree().text_caret_position(id, 10).unwrap();
                    p.y += 10.0;
                    window.tree_mut().selection_pointer_down(p, false, 1);
                    window.tree_mut().end_selection_drag();
                    stage = 2;
                }
                2 => {
                    anyhow::ensure!(window.tree().selected_text().starts_with("user-select: all"), "all was not atomic");
                    snapshot(window, &output.join("atomic.png"))?;
                    idle = Some(window.stats());
                    let (tx, rx) = std::sync::mpsc::channel();
                    wake = Some(rx);
                    let native = window.native_window().clone();
                    // Only the smoke harness schedules a wakeup. A retained
                    // selection must not request any frames during this interval.
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_secs(2));
                        let _ = tx.send(());
                        native.request_redraw();
                    });
                    stage = 3;
                }
                3 if wake.as_ref().is_some_and(|rx| rx.try_recv().is_ok()) => {
                    let old = idle.unwrap();
                    let now = window.stats();
                    anyhow::ensure!(now.presented_frames == old.presented_frames + 1 && now.layout_passes == old.layout_passes && now.scene_builds == old.scene_builds, "selected idle content was rebuilt");
                    println!("PASS selected text, atomic selection, no selection layout, 2s idle: {now:?}");
                    window.close();
                }
                _ => {}
            }
            Ok(())
        })
    } else {
        app
    };
    app.run()
}
fn pixels() -> Result<()> {
    let fonts = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    fonts.add_fonts(vec![Cow::Borrowed(include_bytes!(
        "../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
    ))])?;
    let mut stage = 0;
    Application::new()
        .text_system(Arc::new(TextSystem::new(Arc::new(fonts))))
        .css("#app{width:300px;height:120px;background:white;color:black;font-family:'IBM Plex Sans';font-size:40px;line-height:60px;padding:20px}::selection{background:lime;color:transparent}")?
        .window(
            WindowOptions {
                title: "voidui selection pixels".into(),
                size: Size::new(340.0, 160.0),
                ..Default::default()
            },
            div().id("app").child(text("MMMM").id("a")),
        )
        .on_frame(move |window| {
            let id = window.tree().find_by_id("a").unwrap();
            match stage {
                0 => { window.tree_mut().set_selection(P::text(id, 0), P::text(id, 4))?; }
                1 => {
                    check_transparent_selection(window, id, false)?;
                    window.tree_mut().replace_text(id, 0..4, "ffi")?;
                }
                2 => { window.tree_mut().set_selection(P::text(id, 1), P::text(id, 2))?; }
                _ => {
                    check_transparent_selection(window, id, true)?;
                    println!("PASS GPU pixels: exact highlight, transparent foreground, partial ligature, unselected ink preserved");
                    window.close();
                }
            }
            stage += 1;
            Ok(())
        })
        .run()
}

fn check_transparent_selection(
    window: &mut voidui::AppWindow,
    id: voidui::core::widget::WidgetId,
    require_unselected_ink: bool,
) -> Result<()> {
    let rects = window.tree().selection_rectangles(id);
    anyhow::ensure!(!rects.is_empty(), "no highlight rectangles");
    let (size, pixels) = window.snapshot()?;
    let scale = size.width as f32 / window.logical_size().width;
    for rect in rects {
        // Exclude antialiased rectangle edges; every interior pixel must have
        // exactly the background color, with no original glyph underneath it.
        for y in ((rect.origin.y + 1.0) * scale) as usize
            ..((rect.origin.y + rect.size.height - 1.0) * scale) as usize
        {
            for x in ((rect.origin.x + 1.0) * scale) as usize
                ..((rect.origin.x + rect.size.width - 1.0) * scale) as usize
            {
                let p = &pixels[(y * size.width as usize + x) * 4..][..4];
                anyhow::ensure!(
                    p == [0, 255, 0, 255],
                    "old glyph leaked through transparent selected foreground: {p:?}"
                );
            }
        }
    }
    if require_unselected_ink {
        anyhow::ensure!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[0] < 100 && p[1] < 100 && p[2] < 100),
            "partial selection erased unselected ligature ink"
        );
    }
    Ok(())
}
