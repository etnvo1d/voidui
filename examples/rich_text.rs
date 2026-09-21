//! Run `cargo run --example rich_text`, or use `--smoke <directory>` for a
//! deterministic native rendering, formatting, IME, and undo verification.
use std::path::Path;
use voidui::{
    core::geometry::Size,
    editing::{Selection, SelectionSet, StylePatch},
    *,
};
fn color(value: &str) -> render::Hsla {
    value.parse::<style::color::Color>().unwrap().into()
}
fn snapshot(window: &mut AppWindow, path: &Path) -> anyhow::Result<()> {
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
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let output = match args.as_slice() {
        [] => None,
        [flag, path] if flag == "--smoke" => {
            std::fs::create_dir_all(path)?;
            Some(std::path::PathBuf::from(path))
        }
        _ => anyhow::bail!("usage: rich_text [--smoke directory]"),
    };
    let content: RichText = span("One paragraph, ")
        .child(span("many styles").bold().color(color("#7454bf")))
        .child(". Select text and use the toolbar.\n")
        .child(
            span("Shared baselines ")
                .font_size(28.0)
                .style(InlineStyle::new().line_height(38.0)),
        )
        .child(span("with small text").italic())
        .child(" and wrapping in the same paragraph.\n\n")
        .child("Unicode editing: 中文 · café · 👩‍💻 · العربية.\n")
        .child(
            span("Links are extensible metadata")
                .color(color("#187856"))
                .style(InlineStyle::new().metadata("href", "https://example.invalid")),
        )
        .child("; components decide how to activate them.")
        .into();
    let editor = Editor::from_rich(content);
    let mut toolbar = div().class("toolbar");
    for (label, patch) in [
        ("Bold", StylePatch::new().bold(true)),
        ("Italic", StylePatch::new().italic(true)),
        ("Accent", StylePatch::new().color(color("#7454bf"))),
        ("Clear", StylePatch::clear()),
    ] {
        let editor = editor.clone();
        toolbar = toolbar.child(div().tag("button").child(label).on_click(move || {
            if let Err(error) = editor.update(|s| s.format_selections(patch.clone())) {
                eprintln!("{error}");
            }
        }));
    }
    let undo = editor.clone();
    toolbar = toolbar.child(div().tag("button").child("Undo").on_click(move || {
        if let Err(error) = undo.update(|s| s.undo()) {
            eprintln!("{error}");
        }
    }));
    let root = div()
        .id("app")
        .child(
            rich_text(
                span("Rich text, ").child(span("one layout").italic().color(color("#7454bf"))),
            )
            .class("title"),
        )
        .child(text("Inline typography · atomic formatting · native input").class("subtitle"))
        .child(toolbar)
        .child(rich_editor(&editor).id("editor").rows(12))
        .child(
            text("Formatting and text share undo. Resize to rewrap without reshaping.")
                .class("footer"),
        );
    let app = Application::new().css(r#"
        * { font-family: system-ui; font-size: 17px; line-height: 26px; color: #262936; }
        #app { padding: 30px; background: #faf9f6; }
        .title { font-size: 30px; line-height: 40px; margin-bottom: 6px; }
        .subtitle { color: #737680; margin-bottom: 22px; }
        .toolbar { display: flex; gap: 8px; margin-bottom: 12px; }
        button { background: #eeeaf6; padding: 7px 14px; border-radius: 6px; cursor: pointer; }
        button:hover { background: #e0d8f2; }
        textarea { width: 100%; height: 355px; padding: 16px; background: white; border: 1px solid #d6d3dc; border-radius: 8px; caret-animation: manual; }
        textarea::selection { background: #d9ccef; color: #24173c; }
        .footer { font-size: 13px; color: #737680; margin-top: 14px; }
    "#)?.window(WindowOptions {title:"voidui — rich text".into(),size:Size::new(820.0,650.0),..Default::default()}, root);
    if let Some(output) = output {
        let mut stage = 0;
        let mut layout_passes = 0;
        let original = editor.text();
        app.on_frame(move |window| {
            match stage {
                0 => {
                    snapshot(window, &output.join("initial.png"))?;
                    layout_passes = window.stats().layout_passes;
                    editor.update(|s| {
                        s.select(SelectionSet::single(Selection::range(0, 13)))?;
                        s.format_selections(StylePatch::new().bold(true).color(color("#187856")))
                    })?;
                    stage = 1;
                }
                1 => {
                    anyhow::ensure!(
                        window.stats().layout_passes == layout_passes,
                        "formatting caused box layout"
                    );
                    snapshot(window, &output.join("formatted.png"))?;
                    editor.update(|s| s.undo())?;
                    editor.update(|s| s.select(SelectionSet::single(Selection::caret(0))))?;
                    let id = window.tree().find_by_id("editor").unwrap();
                    window.tree_mut().set_focused(Some(id));
                    window.dispatch_input(
                        id,
                        &core::input::InputEvent::Preedit("输入".into(), Some((6, 6))),
                    );
                    stage = 2;
                }
                2 => {
                    anyhow::ensure!(editor.text() == original, "preedit changed committed text");
                    snapshot(window, &output.join("preedit.png"))?;
                    let id = window.tree().find_by_id("editor").unwrap();
                    window.dispatch_input(id, &core::input::InputEvent::Commit("输入".into()));
                    stage = 3;
                }
                3 => {
                    anyhow::ensure!(editor.text().starts_with("输入"), "IME did not commit");
                    editor.update(|s| s.undo())?;
                    stage = 4;
                }
                _ => {
                    anyhow::ensure!(editor.text() == original, "undo did not restore document");
                    snapshot(window, &output.join("restored.png"))?;
                    println!(
                        "PASS rich inline rendering, formatting, IME, undo; {:?}",
                        window.stats()
                    );
                    window.close();
                }
            }
            Ok(())
        })
        .run()
    } else {
        app.run()
    }
}
