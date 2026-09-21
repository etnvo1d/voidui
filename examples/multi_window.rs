//! `cargo run --example multi_window [--smoke]` shares a GPU device and font system.
use std::{cell::Cell, rc::Rc};
use voidui::{Application, WindowOptions, core::geometry::Size, div, style::color::Rgba8, text};

fn main() -> anyhow::Result<()> {
    let smoke = match std::env::args().nth(1).as_deref() {
        None => false,
        Some("--smoke") => true,
        _ => anyhow::bail!("usage: multi_window [--smoke]"),
    };
    let mut app = Application::new();
    let titles = ["First window", "Second window"];
    for title in titles {
        app = app.window(
            WindowOptions {
                title: title.into(),
                size: Size::new(480.0, 240.0),
                ..WindowOptions::default()
            },
            div()
                .background(Rgba8::from_rgb8(230, 240, 245))
                .padding(24.0)
                .child(text(title).font_size(28.0))
                .child("This window has its own scene and shares the application's GPU and fonts."),
        );
    }
    let completed = Rc::new(Cell::new(0));
    if smoke {
        let completed = completed.clone();
        app = app.on_frame(move |window| {
            println!(
                "presented {:?}: {:?}",
                window.native_window().id(),
                window.stats()
            );
            completed.set(completed.get() + 1);
            window.close();
            Ok(())
        });
    }
    app.run()?;
    if smoke {
        anyhow::ensure!(
            completed.get() == titles.len(),
            "not every window presented"
        );
    }
    Ok(())
}
