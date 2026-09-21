//! `cargo run --example media` displays image fitting, sampling and inline SVG.
//! `--snapshot <path>` verifies GPU pixels and writes one reproducible screenshot.
use anyhow::{Result, ensure};
use std::{borrow::Cow, sync::Arc};
use voidui::{
    Application, WindowOptions,
    core::geometry::Size,
    div, img,
    media::Image,
    render::{ParleyTextSystem, TextSystem},
    svg, svg_from_str, text,
};
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let snapshot = match args.as_slice() {
        [] => None,
        [flag, path] if flag == "--snapshot" => Some(std::path::PathBuf::from(path)),
        _ => anyhow::bail!("usage: media [--snapshot path]"),
    };
    let fonts = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    fonts.add_fonts(vec![Cow::Borrowed(include_bytes!(
        "../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
    ))])?;
    let mut pixels = Vec::new();
    for y in 0..12 {
        for x in 0..20 {
            pixels.extend_from_slice(if (x / 4 + y / 4) % 2 == 0 {
                &[44, 101, 232, 255]
            } else {
                &[255, 189, 89, 255]
            });
        }
    }
    let checker = Image::from_rgba(20, 12, pixels)?;
    let alpha = Image::from_rgba(1, 1, vec![255, 0, 0, 128])?;
    let graphic=svg_from_str(r##"<svg viewBox="0 0 260 120"><defs><linearGradient id="sky"><stop stop-color="#207be8"/><stop offset="1" stop-color="#7bcbb6"/></linearGradient></defs><rect width="260" height="120" rx="16" fill="url(#sky)"/><circle cx="204" cy="29" r="15" fill="#ffe5a8"/><path d="M0 95 L65 40 L120 100 L166 61 L260 120 H0 Z" fill="#163869"/><path d="M0 120 L96 70 L146 120 Z" fill="#a8e1d5"/></svg>"##)?.class("landscape");
    let mut grid = div().class("samples");
    for (mode, label) in [
        ("contain", "Contain"),
        ("cover", "Cover"),
        ("fill", "Fill"),
        ("none", "None"),
        ("scale-down", "Scale down"),
    ] {
        grid = grid.child(
            div()
                .class("sample")
                .child(img(checker.clone()).id(mode).class(mode))
                .child(text(label)),
        );
    }
    let content = div()
        .id("app")
        .child(text("Images & vector graphics").class("title"))
        .child(
            text("Standard CSS. Shared resources. Draw only when something changes.")
                .class("subtitle"),
        )
        .child(grid)
        .child(
            div().class("vector-row").child(graphic).child(
                div()
                    .class("icon-panel")
                    .child(
                        svg()
                            .id("check")
                            .class("icon")
                            .view_box(0, 0, 24, 24)
                            .child(svg::path().class("mark").d("M5 12 L10 17 L19 7")),
                    )
                    .child(text("Hover to recolor").class("hint")),
            ),
        )
        .child(
            div()
                .class("verification")
                .child(img(alpha).id("alpha"))
                .child(
                    svg()
                        .id("blue")
                        .view_box(0, 0, 10, 10)
                        .child(svg::rect().width(10).height(10).fill("blue")),
                ),
        )
        .child(
            text("PNG / JPEG / GIF / WebP / SVG • viewBox • currentColor • object-fit")
                .class("footer"),
        );
    let css = r#"
        #app {padding:32px;background:#f6f8fc;color:#192b45;font-family:'IBM Plex Sans';width:740px;gap:18px;display:flex;flex-direction:column}
        .title {font-size:30px;font-weight:600} .subtitle {color:#62728a;font-size:14px}
        .samples {display:flex;gap:12px} .sample {display:flex;flex-direction:column;gap:8px;color:#62728a;font-size:13px}
        .sample img {width:130px;height:96px;border-radius:10px;background:#e1e7f2;image-rendering:pixelated}
        .contain {object-fit:contain} .cover {object-fit:cover} .fill {object-fit:fill} .none {object-fit:none} .scale-down {object-fit:scale-down}
        .vector-row {display:flex;gap:24px;align-items:center;margin-top:8px} .landscape {width:420px;height:194px;border-radius:16px}
        .icon-panel {display:flex;flex-direction:column;gap:12px;align-items:center} .icon {width:72px;height:72px;color:#207be8;fill:none;stroke:currentColor;stroke-width:2;stroke-linecap:round;stroke-linejoin:round}
        .icon:hover .mark {stroke:#cf4272;stroke-dasharray:4 2} .hint {font-size:13px;color:#62728a}
        .verification {display:flex;gap:8px;background:white} #alpha,#blue {width:16px;height:16px} .footer {font-size:12px;color:#62728a}
    "#;
    let app = Application::new()
        .text_system(Arc::new(TextSystem::new(Arc::new(fonts))))
        .css(css)?
        .window(
            WindowOptions {
                title: "voidui — Images and SVG".into(),
                size: Size::new(804., 560.),
                ..Default::default()
            },
            content,
        );
    let mut stage = 0;
    let mut baseline = None;
    let app = if let Some(path) = snapshot {
        app.on_frame(move |window| {
            let (size, bytes) = window.snapshot()?;
            let scale = size.width as f32 / window.logical_size().width;
            let at = |id: &str| {
                let tree = window.tree();
                let b = tree.bounds(tree.find_by_id(id).unwrap());
                let x = ((b.origin.x + b.size.width * 0.5) * scale) as usize;
                let y = ((b.origin.y + b.size.height * 0.5) * scale) as usize;
                &bytes[(y * size.width as usize + x) * 4..][..4]
            };
            ensure!(
                at("blue")
                    .iter()
                    .zip([0u8, 0, 255, 255])
                    .all(|(a, b)| a.abs_diff(b) <= 1),
                "SVG GPU color mismatch: {:?}",
                at("blue")
            );
            ensure!(
                at("alpha")
                    .iter()
                    .zip([255u8, 127, 127, 255])
                    .all(|(a, b)| a.abs_diff(b) <= 2),
                "premultiplied image mismatch: {:?}",
                at("alpha")
            );
            let b = window
                .tree()
                .bounds(window.tree().find_by_id("fill").unwrap());
            let x = ((b.origin.x + b.size.width * 3.75 / 20.) * scale) as usize;
            let y = ((b.origin.y + b.size.height * 2.5 / 12.) * scale) as usize;
            let sampled = &bytes[(y * size.width as usize + x) * 4..][..4];
            ensure!(
                sampled
                    .iter()
                    .zip([44u8, 101, 232, 255])
                    .all(|(a, b)| a.abs_diff(b) <= 1),
                "pixelated sampling blended across a pixel interior: {sampled:?}"
            );
            if stage == 0 {
                let mut encoder = png::Encoder::new(
                    std::io::BufWriter::new(std::fs::File::create(&path)?),
                    size.width,
                    size.height,
                );
                encoder.set_color(png::ColorType::Rgba);
                encoder.set_depth(png::BitDepth::Eight);
                encoder.write_header()?.write_image_data(&bytes)?;
                baseline = Some(voidui::media::stats());
                stage = 1;
                window.request_redraw();
            } else {
                ensure!(
                    Some(voidui::media::stats()) == baseline,
                    "cached frame repeated media work"
                );
                println!(
                    "Media GPU verification passed; {:?}; {:?}",
                    voidui::media::stats(),
                    window.stats()
                );
                window.close();
            }
            Ok(())
        })
    } else {
        app
    };
    app.run()
}
