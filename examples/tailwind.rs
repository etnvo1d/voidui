//! Run `cargo run --example tailwind`, or add `--snapshot output.png` to capture
//! one native frame and exit. Both cards share the same utility definitions.
use std::{borrow::Cow, sync::Arc};
use voidui::{
    Application, Children, WindowOptions, component,
    core::geometry::Size,
    div,
    render::{ParleyTextSystem, TextSystem},
    style::tailwind,
    text,
};

/// A reusable component whose callers style its actual root and supply content.
#[component]
fn panel(children: Children) {
    div().children(children)
}

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let snapshot = match args.as_slice() {
        [] => None,
        [flag, path] if flag == "--snapshot" => Some(path.clone()),
        _ => anyhow::bail!("usage: tailwind [--snapshot output.png]"),
    };
    // A bundled font keeps the example and snapshots consistent across hosts.
    let fonts = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    fonts.add_fonts(vec![Cow::Borrowed(include_bytes!(
        "../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
    ))])?;
    let card = "flex flex-col flex-1 min-w-0 p-6 gap-4 rounded-xl bg-white border border-gray-200 shadow-sm";
    let button = "px-4 py-3 rounded-lg bg-indigo-600 hover:bg-indigo-700 text-white font-semibold cursor-pointer";
    let root = div().box_border().size_full().flex_col().p_8().gap_6().bg_gray_50().text_gray_900()
        .child(text("Tailwind utilities. Native Rust.").text_3xl().font_bold())
        .child(text("One style catalog, two ways to build your interface.").text_base().text_gray_500())
        .child(div().flex_row().gap_6()
            .child(panel().flex_col().flex_1().min_w_0().p_6().gap_4().rounded_xl()
                .bg_white().border_1().border_gray_200().shadow_sm()
                .child(|| text("Fluent methods").text_xl().font_semibold())
                .child(|| text("panel().w_full().border_r(1)\n    .border_l_2().bg_gray_50()").text_sm().text_gray_600())
                .child(|| div().box_border().w_full().border_r(1).border_l_2().border_indigo_500().bg_gray_50().p_4()
                    .child(text("Independent borders + shared theme").text_sm()))
                .child(|| text("No trait import or stylesheet required.").text_sm().text_gray_500()))
            .child(div().class(card)
                .child(text("CSS classes").text_xl().font_semibold())
                .child(text("Compile the classes once, then attach\nthem to any widget with .class(...).").text_sm().text_gray_600())
                .child(div().class(button).attr("tabindex", "0")
                    .child(text("Hover to change the background")))
                .child(text("Uses the existing CSS state and cascade engine.").text_sm().text_gray_500())))
        .child(text("Spacing, colors, typography, borders and shadows can be themed with CSS variables.")
            .text_sm().text_gray_500());
    let mut app = Application::new()
        .text_system(Arc::new(TextSystem::new(Arc::new(fonts))))
        .stylesheet(tailwind::stylesheet(&format!("{card} {button}"))?)
        .window(
            WindowOptions {
                title: "voidui — Tailwind utilities".into(),
                size: Size::new(1000.0, 480.0),
                ..Default::default()
            },
            root,
        );
    if let Some(path) = snapshot {
        app = app.on_frame(move |window| {
            let (size, pixels) = window.snapshot()?;
            let output = std::io::BufWriter::new(std::fs::File::create(&path)?);
            let mut encoder = png::Encoder::new(output, size.width, size.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.write_header()?.write_image_data(&pixels)?;
            println!("snapshot={path} stats={:?}", window.stats());
            window.close();
            Ok(())
        });
    }
    app.run()
}
