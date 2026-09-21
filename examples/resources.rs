//! Edit a path to load a text file. Input changes cancel the previous query.
//! Run: cargo run --example resources -- [--smoke] README.md
use std::path::PathBuf;
use voidui::{
    Application, IntoElement, Read, TaskRuntime, WindowOptions, component, div, files, input,
    resource, state, text,
};

#[component]
fn reader(initial_path: Read<String>) -> impl IntoElement {
    let path = state(|| initial_path.to_string());
    // Reads declare the dependency. The loader receives an owned input snapshot;
    // unrelated renders do not read the file again.
    let content = resource(path.get(), async |path| {
        files::read_text(PathBuf::from(path)).await
    });
    div()
        .padding(24)
        .child(input(path).placeholder("Text file path"))
        .child(
            div()
                .tag("button")
                .id("reload")
                .child("Reload")
                .on_click(move || content.reload()),
        )
        .child(
            text(if content.is_loading() {
                "Loading…"
            } else {
                "Ready"
            })
            .id("resource-status"),
        )
        .child(text(
            content
                .error()
                .map(|error| error.to_string())
                .unwrap_or_default(),
        ))
        .child(
            text(content.with(|value| value.cloned().unwrap_or_default())).id("resource-content"),
        )
}
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let first = args.next();
    let smoke = first.as_deref() == Some("--smoke");
    let path = (if smoke { args.next() } else { first }).unwrap_or_else(|| "README.md".into());
    let expected = if smoke {
        Some(std::fs::read_to_string(&path)?)
    } else {
        None
    };
    let runtime = TaskRuntime::default();
    let mut reloaded = false;
    Application::new()
        .task_runtime(runtime.clone())
        .window(
            WindowOptions {
                title: "Async resources".into(),
                ..Default::default()
            },
            reader(path),
        )
        .on_frame(move |window| {
            if let Some(expected) = &expected {
                let status = window.tree().find_by_id("resource-status").unwrap();
                if window.tree().text_content(status) != Some("Ready") {
                    return Ok(());
                }
                let content = window.tree().find_by_id("resource-content").unwrap();
                anyhow::ensure!(window.tree().text_content(content) == Some(expected.as_str()));
                if !reloaded {
                    reloaded = true;
                    let button = window.tree().find_by_id("reload").unwrap();
                    window.tree_mut().click(button);
                } else if runtime.stats().completed >= 2 {
                    println!(
                        "PASS resource loading, reload, and native rendering: {:?}",
                        runtime.stats()
                    );
                    window.close();
                }
            }
            Ok(())
        })
        .run()
}
