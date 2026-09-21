//! Ordinary async code with file I/O, CPU work, and native-window lifecycle checks.
//! Run: cargo run --example async_tasks -- [--smoke] <text-file>
use anyhow::Context;
use std::{
    borrow::Cow,
    cell::{Cell, RefCell},
    hash::{Hash, Hasher},
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::Duration,
};
use voidui::{
    Application, State, TaskRuntime, WindowOptions, component, div, files, on_mount,
    render::SharedString,
    state,
    tasks::{time, workers},
    text,
};

#[derive(Default)]
struct Probe {
    status: RefCell<Option<State<SharedString>>>,
    renders: Cell<usize>,
    mounted: Cell<bool>,
    hidden_done: Cell<bool>,
    idle_done: Cell<bool>,
    error: RefCell<Option<String>>,
}

#[component]
fn demo(path: PathBuf, probe: Rc<Probe>) -> impl voidui::IntoElement {
    let status = state(|| SharedString::from("Loading..."));
    probe.renders.set(probe.renders.get() + 1);
    *probe.status.borrow_mut() = Some(status.clone());
    let initial_path = path.clone();
    let initial_status = status.clone();
    on_mount(async move || -> anyhow::Result<()> {
        let contents = files::read_text(initial_path).await?;
        initial_status.set(contents);
        probe.mounted.set(true);
        Ok(())
    });
    let read_status = status.clone();
    let compute_status = status.clone();
    let compute_path = path.clone();
    div().class("app")
        .child(text("Scoped async tasks").class("heading"))
        .child("File I/O and computation run in the background. State updates resume on the UI thread.")
        .child(div().class("actions")
            .child(div().tag("button").child("Read file").on_click(async move || -> anyhow::Result<()> {
                read_status.set(files::read_text(&path).await?);
                Ok(())
            }))
            .child(div().tag("button").child("Compute checksum").on_click(async move || -> anyhow::Result<()> {
                let bytes = files::read(&compute_path).await?;
                let checksum = workers::compute(move || {
                    // This demonstration checksum is not a cryptographic digest.
                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                    bytes.hash(&mut hasher);
                    hasher.finish()
                }).await?;
                compute_status.set(format!("Checksum: {checksum:016x}").into());
                Ok(())
            }))
        )
        .child(text(status.get()).id("status"))
}

fn main() -> anyhow::Result<()> {
    let mut smoke = false;
    let mut path = None;
    for arg in std::env::args().skip(1) {
        if arg == "--smoke" {
            smoke = true;
        } else {
            anyhow::ensure!(path.is_none(), "Usage: async_tasks [--smoke] <text-file>");
            path = Some(PathBuf::from(arg));
        }
    }
    let path = path.context("Usage: async_tasks [--smoke] <text-file>")?;
    let probe = Rc::new(Probe::default());
    let runtime = TaskRuntime::default();
    let errors = probe.clone();
    runtime.set_error_handler(move |error| {
        eprintln!("{error}");
        *errors.error.borrow_mut() = Some(error.to_string());
    });
    let fonts = voidui::render::ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    fonts.add_fonts(vec![Cow::Borrowed(include_bytes!(
        "../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
    ))])?;
    let mut stage = 0;
    let mut idle_frames = None;
    // Explicit diagnostic deadlines; ordinary applications do not install this timer.
    let pause = Duration::from_millis(300);
    let mut app = Application::new()
        .task_runtime(runtime.clone())
        .text_system(Arc::new(voidui::render::TextSystem::new(Arc::new(fonts))))
        .window(WindowOptions { title: "Async tasks".into(), ..Default::default() }, demo(path, probe.clone()))
        .css(".app { padding: 24px; color: #dce4f0; background: #16202e; font-family: 'IBM Plex Sans'; font-size: 15px; height: 100%; } .heading { font-size: 26px; margin-bottom: 12px; } .actions { display: flex; gap: 12px; margin-top: 16px; margin-bottom: 16px; } button { padding: 10px; background: #29445f; color: inherit; } button:hover { background: #386383; }")?;
    if smoke {
        app = app.on_frame(move |window| {
            if let Some(error) = probe.error.borrow().as_ref() { anyhow::bail!("async smoke failed: {error}"); }
            match stage {
                0 if probe.mounted.get() => {
                    stage = 1;
                    let native = window.native_window().clone();
                    let probe = probe.clone();
                    native.set_minimized(true);
                    window.task_scope().spawn(async move {
                        let result = async {
                            time::sleep(pause).await?;
                            let renders = probe.renders.get();
                            probe.status.borrow().as_ref().unwrap().set("Updated while minimized".into());
                            time::yield_now().await;
                            anyhow::ensure!(probe.renders.get() > renders, "component did not update while minimized");
                            probe.hidden_done.set(true);
                            Ok::<_, anyhow::Error>(())
                        }.await;
                        if let Err(error) = result { *probe.error.borrow_mut() = Some(error.to_string()); }
                        native.set_minimized(false);
                        native.request_redraw();
                    });
                }
                1 if probe.hidden_done.get() => {
                    let id = window.tree().find_by_id("status").unwrap();
                    anyhow::ensure!(window.tree().text_content(id) == Some("Updated while minimized"));
                    stage = 2;
                    idle_frames = Some(window.stats());
                    let native = window.native_window().clone();
                    let probe = probe.clone();
                    let runtime = runtime.clone();
                    window.task_scope().spawn(async move {
                        let before = runtime.stats().polls;
                        let result = time::sleep(pause).await;
                        // Only this timer's initial poll should have completed during
                        // the interval; its resumed poll is counted after this body.
                        if result.is_err() || runtime.stats().polls != before + 1 {
                            *probe.error.borrow_mut() = Some("idle interval polled unrelated tasks".into());
                        }
                        probe.idle_done.set(true);
                        native.request_redraw();
                    });
                }
                2 if probe.idle_done.get() => {
                    let before = idle_frames.unwrap(); let after = window.stats();
                    anyhow::ensure!(before.layout_passes == after.layout_passes, "idle interval performed layout");
                    anyhow::ensure!(before.scene_builds == after.scene_builds, "idle interval rebuilt the scene");
                    println!("PASS async file loading, minimized updates, UI resumption, idle scheduling: {:?}", runtime.stats());
                    window.close();
                }
                _ => {}
            }
            Ok(())
        });
    }
    app.run()
}
