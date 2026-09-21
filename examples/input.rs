//! Run `cargo run --example input`. Use `--smoke <directory>` for native scene,
//! text-edit invalidation, and parked-caret verification without OS clipboard writes.
use anyhow::Result;
use std::{path::Path, time::Duration};
use voidui::{
    core::{geometry::Size, input::KeyInput},
    editing::{EditKind, Selection, SelectionSet},
    style::css::Stylesheet,
    *,
};
use winit::keyboard::{Key, NamedKey};

fn snapshot(window: &mut AppWindow, path: &Path) -> Result<()> {
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
    let output = match args.as_slice() {
        [] => None,
        [flag, path] if flag == "--smoke" => {
            std::fs::create_dir_all(path)?;
            Some(std::path::PathBuf::from(path))
        }
        _ => anyhow::bail!("usage: input [--smoke directory]"),
    };
    let code = Editor::new("alpha\nalpha");
    code.update(|state| {
        state.select(SelectionSet::new([Selection::caret(5), Selection::caret(11)], 1).unwrap())
    })?;
    let root = form(code.clone());
    let css = include_str!("styles/input.css");
    let app = Application::new().css(css)?.window(
        WindowOptions {
            title: "voidui — text editing".into(),
            size: Size::new(780.0, 750.0),
            ..Default::default()
        },
        root,
    );
    if let Some(output) = output {
        let mut stage = 0;
        let mut before = None;
        let mut idle = None;
        let mut wake = None;
        app.on_frame(move |window| {
            match stage {
                0 => {
                    snapshot(window, &output.join("initial.png"))?;
                    before = Some(window.stats());
                    let input = window.tree().find_by_id("search").unwrap();
                    window.dispatch_input(input, &voidui::core::input::InputEvent::Text("Parley + VoidUI".into()));
                    stage = 1;
                }
                1 => {
                    anyhow::ensure!(window.stats().layout_passes == before.unwrap().layout_passes, "typing caused box layout");
                    let id = window.tree().find_by_id("code").unwrap(); window.tree_mut().set_focused(Some(id));
                    code.update(|s| s.replace_selections("_completed", EditKind::Command))?;
                    stage = 2;
                }
                2 => {
                    anyhow::ensure!(code.text() == "alpha_completed\nalpha_completed", "multiple selections diverged");
                    snapshot(window, &output.join("edited.png"))?;
                    // CSS disables automatic caret animation without a private property.
                    window.set_stylesheets(vec![Stylesheet::parse(&format!("{css}\ntextarea {{ caret-animation: manual; }}"))?]);
                    stage = 3;
                }
                3 => {
                    idle = Some(window.stats());
                    let (tx, rx) = std::sync::mpsc::channel(); wake = Some(rx);
                    let native = window.native_window().clone();
                    std::thread::spawn(move || { std::thread::sleep(Duration::from_secs(2)); let _ = tx.send(()); native.request_redraw(); });
                    stage = 4;
                }
                4 if wake.as_ref().is_some_and(|r| r.try_recv().is_ok()) => {
                    let old = idle.unwrap(); let now = window.stats();
                    anyhow::ensure!(now.presented_frames == old.presented_frames + 1 && now.scene_builds == old.scene_builds && now.layout_passes == old.layout_passes,
                        "manual caret did not park: {old:?} -> {now:?}");
                    println!("PASS native multi-cursor editing, no typing box layout, 2s parked caret: {now:?}"); window.close();
                }
                _ => {}
            }
            Ok(())
        }).run()
    } else {
        app.run()
    }
}

#[component]
fn form(code: Editor) -> impl IntoElement {
    let name = state(String::new);
    let message = state(|| {
        String::from(
            "A small editor, with a reusable core.\n\nTry Unicode: 中文 · café · 👩‍💻 · abc אבג xyz.\nSelect text, paste, undo, or resize the window.",
        )
    });
    let readonly = state(|| String::from("This value can be selected and copied."));
    let disabled = state(|| String::from("Disabled input"));
    let clear = name.clone();
    div()
        .id("app")
        .child(text("Text editing").class("title"))
        .child(
            text("Shared transactions, selections, and native input. Styled with CSS.")
                .class("subtitle"),
        )
        .child(text("SEARCH").class("label"))
        .child(
            input_group()
                .class("search")
                .child(text("Search").class("addon"))
                .child(input(name).id("search").placeholder("Type a name…"))
                .child(
                    div()
                        .tag("button")
                        .class("clear")
                        .child("Clear")
                        .on_click(move || {
                            clear.set(String::new());
                        }),
                ),
        )
        .child(text("MESSAGE").class("label"))
        .child(
            input_group()
                .class("composer")
                .child(textarea(message).id("message").rows(5))
                .child(
                    div()
                        .class("footer")
                        .child("Enter adds a line · Tab moves focus · Cmd/Ctrl+Z undoes"),
                ),
        )
        .child(text("CUSTOM KEYMAP + TWO CURSORS").class("label"))
        .child(
            textarea(&code)
                .id("code")
                .rows(2)
                .on_key(|key: &KeyInput, editor| {
                    if key.key == Key::Named(NamedKey::Tab) {
                        editor
                            .update(|s| s.replace_selections("_completed", EditKind::Command))
                            .ok();
                        true
                    } else {
                        false
                    }
                }),
        )
        .child(
            text("Both cursors share one transaction. Tab accepts a demo completion.")
                .class("hint"),
        )
        .child(
            div()
                .class("states")
                .child(input(readonly).read_only(true))
                .child(input(disabled).disabled(true)),
        )
}
