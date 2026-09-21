//! Minimal native entry point. The larger demo is `cargo run --example hello`.
use voidui::{Application, WindowOptions, core::layout::BoxSizing, div, style::color::Rgba8, text};

fn main() -> anyhow::Result<()> {
    let foreground = Rgba8::from_rgb8(225, 235, 245);
    Application::new().window(WindowOptions { title: "voidui".into(), ..WindowOptions::default() },
        div().width(voidui::style::pct(100.0)).height(voidui::style::pct(100.0))
            .background(Rgba8::from_rgb8(16, 25, 34)).color(foreground)
            .box_sizing(BoxSizing::BorderBox).padding(32.0)
            .child(text("Hello, voidui").font_size(32.0).line_height(44.0))
            .child(div().background(Rgba8::from_rgb8(27, 42, 55)).border_radius(12.0)
                .padding(24.0).margin_top(20)
                .child("div and text are rendered in a native window.")
                .child("Resize the window to reflow. Unchanged content leaves the event loop asleep.")))
        .run()
}
