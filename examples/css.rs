//! Static: cargo run --example css
//! Watch: cargo run --example css -- --watch
//! Smoke: cargo run --example css -- --smoke target/css-smoke
use anyhow::Result;
use std::{borrow::Cow, path::PathBuf, sync::Arc};
use voidui::{
    AppWindow, Application, WindowOptions,
    core::geometry::Size,
    div,
    render::{ParleyTextSystem, TextSystem},
    text,
};

fn snapshot(window: &mut AppWindow, path: &std::path::Path) -> Result<()> {
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
    let (mut file, mut watch, mut smoke) = (
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/styles/demo.css"),
        false,
        None,
    );
    match args.as_slice() {
        [] => {}
        [flag] if flag == "--watch" => watch = true,
        [flag, path] if flag == "--css" => file = path.into(),
        [flag, path] if flag == "--smoke" => {
            watch = true;
            smoke = Some(PathBuf::from(path));
        }
        _ => anyhow::bail!("usage: css [--watch | --css path.css | --smoke output-dir]"),
    }
    if let Some(dir) = &smoke {
        std::fs::create_dir_all(dir)?;
        file = dir.join("reload.css");
        std::fs::write(&file, include_str!("styles/demo.css"))?;
    }
    let fonts = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    fonts.add_fonts(vec![Cow::Borrowed(include_bytes!(
        "../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
    ))])?;
    let root=div().id("app")
        .child(text("CSS styles. Native pixels.").class("title"))
        .child(text("Compiled selectors, predictable cascade, and event-driven updates.").class("subtitle"))
        .child(div().class("cards")
            .child(div().class("card").attr("tabindex","0")
                .child(text("Styles stay separate").class("heading"))
                .child("Type, class, ID and relationship selectors style these widgets. Hover or press a card to update its border and background."))
            .child(div().class("card")
                .child(text("Quiet until a change").class("heading"))
                .child("CSS is compiled once. Static windows do not rematch selectors or poll files. Hot reload is off until explicitly enabled.")))
        .child(text("Edit examples/styles/demo.css with --watch to see live changes.").id("probe"))
        .child(text("Invalid CSS keeps the last valid stylesheet. Atomic saves are supported.").class("footer"));
    let mut app = Application::new()
        .text_system(Arc::new(TextSystem::new(Arc::new(fonts))))
        .css_file(&file)?
        .window(
            WindowOptions {
                title: "voidui — CSS".into(),
                size: Size::new(900.0, 560.0),
                ..WindowOptions::default()
            },
            root,
        );
    app = app.css_hot_reload(watch);
    if let Some(dir) = smoke {
        let (mut phase, mut signals) = (0, None);
        app = app.on_frame(move |window| {
            let root = window.tree().root().unwrap();
            let probe = window.tree().children(root)[3];
            match phase {
                0 => {
                    snapshot(window, &dir.join("initial.png"))?;
                    let handle = window.native_window().clone();
                    let target = file.clone();
                    let (tx, rx) = std::sync::mpsc::channel();
                    signals = Some(rx);
                    // Diagnostic driver only; normal apps never start this timer.
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        std::fs::write(target, "#probe {color:invalid-color;}").unwrap();
                        std::thread::sleep(std::time::Duration::from_secs(2));
                        let _ = tx.send(());
                        handle.request_redraw();
                    });
                    phase = 1;
                }
                1 if signals.as_ref().unwrap().try_recv().is_ok() => {
                    anyhow::ensure!(
                        window.tree().text_style(probe).color
                            == voidui::style::color::Rgba8::from_hex_rgb(0x70dbb0).into(),
                        "invalid edit replaced the good stylesheet"
                    );
                    let temp = file.with_extension("tmp");
                    std::fs::write(
                        &temp,
                        format!(
                            "{}\n#probe {{color: #ffbb55;}}",
                            include_str!("styles/demo.css")
                        ),
                    )?;
                    std::fs::rename(temp, &file)?;
                    phase = 2;
                    println!("PASS: rejected invalid CSS without replacing visible styles");
                }
                2 if window.tree().text_style(probe).color
                    == voidui::style::color::Rgba8::from_hex_rgb(0xffbb55).into() =>
                {
                    snapshot(window, &dir.join("reloaded.png"))?;
                    println!(
                        "PASS: atomic CSS save reloaded in native window; cascade={:?} frame={:?}",
                        window.tree().cascade_stats(),
                        window.stats()
                    );
                    window.close();
                }
                _ => {}
            }
            Ok(())
        });
    }
    app.run()
}
