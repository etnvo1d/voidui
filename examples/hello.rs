//! Run `cargo run --example hello` for a native, demand-driven div/text window.
//! Use `--snapshot output.png` to capture one rendered frame and exit.
//! Use `--smoke output.png` to also test resize, DPI, scene reuse, and idle behavior.
use std::{borrow::Cow, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use voidui::{
    Application, WindowOptions,
    core::{
        geometry::Size,
        layout::{self, AlignItems, FlexWrap, fr},
    },
    div,
    render::{ParleyTextSystem, TextSystem},
    style::color::Rgba8,
    text,
};

fn rgb(value: u32) -> Rgba8 {
    Rgba8::from_hex_rgb(value)
}

fn chip(label: &str) -> impl voidui::core::element::IntoElement {
    div()
        .background(rgb(0x203442))
        .border_radius(8.0)
        .padding_x(12)
        .padding_y(7)
        .child(text(label).font_size(13.0).color(rgb(0xa3e6cb)))
}

fn content() -> impl voidui::core::element::IntoElement {
    div().width(voidui::style::pct(100.0)).height(voidui::style::pct(100.0)).background(rgb(0x101922))
        .color(rgb(0xe5edf4)).font("IBM Plex Sans").font_size(16.0).line_height(26.0)
        .box_sizing(layout::BoxSizing::BorderBox).padding(32.0)
        .child(div().flex_row().gap(12.0).align_items(AlignItems::CENTER)
            .child(div().width(12).height(12).border_radius(6.0).background(rgb(0x70dbb0)))
            .child(text("VOIDUI / NATIVE DESKTOP").font_size(13.0).color(rgb(0x97abbc))))
        .child(div().margin_top(24)
            .child(text("A real window. A quiet runtime.").font_size(32.0).line_height(44.0))
            .child(text("CSS layout, shaped text, and GPU rendering — connected.").color(rgb(0x9fb0c0))))
        .child(div().grid().gap(20.0).margin_top(28).grid_template_columns(vec![fr(1.0), fr(1.0)])
            .child(div().background(rgb(0x1b2a37)).border_color(rgb(0x345064)).border_radius(16.0)
                .padding(22.0).border_width(1.0)
                .child(text("div + text").font_size(23.0).line_height(34.0))
                .child("Backgrounds, rounded borders, padding, inherited typography, and width-dependent wrapping.")
                .child(div().flex_row().gap(8.0).flex_wrap(FlexWrap::Wrap).margin_top(20)
                    .child(chip("Block")).child(chip("Flexbox")).child(chip("Grid"))))
            .child(div().background(rgb(0x20342f)).border_color(rgb(0x3c6154)).border_radius(16.0)
                .padding(22.0).border_width(1.0)
                .child(text("Work only when needed").font_size(23.0).line_height(34.0))
                .child("Resize to reflow. Expose to reuse the scene. Leave this window still and the event loop sleeps.")
                .child(div().flex_col().margin_top(20)
                    .child(text("Shared GPU device / cached glyph atlas").font_size(13.0).color(rgb(0xa3e6cb)))
                    .child(text("No continuous redraw timer").font_size(13.0).color(rgb(0xa3e6cb))))))
        .child(div().margin_top(24)
            .child(text("macOS · Windows · Linux / Winit + WGPU").font_size(13.0).color(rgb(0x8b9fac))))
}

fn write_snapshot(window: &mut voidui::AppWindow, path: &str) -> Result<()> {
    let (size, pixels) = window.snapshot()?;
    let output = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(output, size.width, size.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&pixels)?;
    println!(
        "snapshot={path} physical={}x{} stats={:?}",
        size.width,
        size.height,
        window.stats()
    );
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let (snapshot, smoke) = match args.as_slice() {
        [] => (None, false),
        [mode, path] if mode == "--snapshot" => (Some(path.clone()), false),
        [mode, path] if mode == "--smoke" => (Some(path.clone()), true),
        _ => anyhow::bail!("usage: hello [--snapshot output.png | --smoke output.png]"),
    };
    // Bundle a known font for reproducible screenshots and fast headless startup.
    let fonts = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    fonts.add_fonts(vec![Cow::Borrowed(include_bytes!(
        "../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
    ))])?;
    let mut stage = 0;
    let mut idle_baseline = None;
    let mut wakeups = None;
    let app = Application::new()
        .text_system(Arc::new(TextSystem::new(Arc::new(fonts))))
        .window(
            WindowOptions {
                title: "voidui — div and text".into(),
                size: Size::new(860.0, 530.0),
                min_size: Some(Size::new(520.0, 440.0)),
                ..WindowOptions::default()
            },
            content(),
        );
    let app = if let Some(path) = snapshot {
        app.on_frame(move |window| {
            if !smoke { write_snapshot(window, &path)?; window.close(); return Ok(()); }
            match stage {
                0 => {
                    println!("first frame {:?}", window.stats());
                    let _ = window.native_window().request_inner_size(winit::dpi::LogicalSize::new(720.0, 620.0));
                    stage = 1;
                }
                1 if (window.logical_size().width - 720.0).abs() < 1.0 => {
                    anyhow::ensure!(window.stats().layout_passes >= 2, "resize must reflow layout");
                    let handle = window.native_window().clone();
                    let (tx, rx) = std::sync::mpsc::channel();
                    wakeups = Some(rx);
                    // Diagnostics only: wake twice, then compare counters over an
                    // idle interval. Production frame scheduling has no such thread.
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_secs(2));
                        if tx.send(1).is_ok() { handle.request_redraw(); }
                        std::thread::sleep(Duration::from_secs(2));
                        if tx.send(2).is_ok() { handle.request_redraw(); }
                    });
                    stage = 2;
                }
                2 => {
                    if let Some(rx) = &wakeups {
                        match rx.try_recv() {
                            Ok(1) => { idle_baseline = Some(window.stats()); }
                            Ok(2) => {
                                let baseline = idle_baseline.context("missing idle baseline")?;
                                let current = window.stats();
                                anyhow::ensure!(current.layout_passes == baseline.layout_passes, "idle redraw performed layout");
                                anyhow::ensure!(current.scene_builds == baseline.scene_builds, "idle redraw rebuilt scene");
                                anyhow::ensure!(current.presented_frames == baseline.presented_frames + 1, "unexpected frames while idle");
                                println!("PASS: 2s idle, zero layout/scene rebuilds, one requested presentation");
                                write_snapshot(window, &path)?;
                                window.close();
                            }
                            _ => {}
                        }
                    }
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
