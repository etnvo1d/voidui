//! Scoped interaction, named properties, and reusable content with ordinary Rust.
//! Run with --smoke to verify the button-to-sidebar flow in a native window.
use std::{borrow::Cow, sync::Arc};
use voidui::{
    Application, Children, State, WindowOptions, component, div, provide_context, state, text,
    use_context,
};

#[derive(Clone, Copy, PartialEq)]
struct SidebarController {
    open: State<bool>,
}
impl SidebarController {
    /// Read visibility; callers rendering this value subscribe automatically.
    fn is_open(self) -> bool {
        self.open.get()
    }
    /// Change visibility from an event, including from deeply nested controls.
    fn toggle(self) {
        self.open.update(|open| *open = !*open);
    }
}

#[component]
fn sidebar_scope(#[prop(default = true)] default_open: bool, children: Children) {
    let open = state(|| default_open);
    provide_context(SidebarController { open });
    children.single()
}

#[component]
fn sidebar_trigger(#[prop(default)] controller: Option<SidebarController>, children: Children) {
    let sidebar = controller.unwrap_or_else(use_context);
    div()
        .tag("button")
        .class("toggle")
        .on_click(move || sidebar.toggle())
        .children(children)
}

#[component]
fn sidebar(
    #[prop(default = 240.0)] width: f32,
    #[prop(default)] controller: Option<SidebarController>,
    children: Children,
) {
    let sidebar = controller.unwrap_or_else(use_context);
    let open = sidebar.is_open();
    // The scope survives when content is removed, so the toolbar can reopen it.
    div()
        .class("sidebar")
        .width(if open { width } else { 0.0 })
        .when(open, |view| view.children(children))
}

#[component]
fn iconbar() {
    div().class("iconbar").child(
        sidebar_trigger()
            .id("sidebar-toggle")
            .child("Toggle sidebar"),
    )
}

#[component]
fn files() {
    div()
        .class("files")
        .child(text("Files").class("heading").id("file-heading"))
        .child("src/")
        .child("Cargo.toml")
        .child("README.md")
}

#[component]
fn workspace() {
    sidebar_scope().default_open(true).child(|| {
        div().class("workspace")
            .child(iconbar())
            .child(div().class("body")
                .child(sidebar().width(260.0).child(files()))
                .child(div().class("editor")
                    .child(text("Component composition").class("heading"))
                    .child("The toolbar and sidebar find their shared controller through the scope.")))
    })
}

fn main() -> anyhow::Result<()> {
    let smoke = std::env::args().any(|arg| arg == "--smoke");
    let fonts = voidui::render::ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    fonts.add_fonts(vec![Cow::Borrowed(include_bytes!(
        "../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
    ))])?;
    let mut stage = 0;
    Application::new()
        .text_system(Arc::new(voidui::render::TextSystem::new(Arc::new(fonts))))
        .window(WindowOptions { title: "Component composition".into(), ..Default::default() }, workspace())
        .css(".workspace { height: 100%; background: #16202e; color: #dce4f0; font-family: 'IBM Plex Sans'; font-size: 16px; } .iconbar { padding: 16px; } .toggle { padding: 10px 16px; background: #33475e; border-radius: 6px; } .toggle:hover { background: #466580; } .body { display: flex; } .sidebar { flex-shrink: 0; overflow: hidden; } .files { padding: 20px; background: #203045; } .heading { font-size: 24px; margin-bottom: 18px; } .editor { padding: 20px; flex-grow: 1; }")?
        .on_frame(move |window| {
            if !smoke { return Ok(()); }
            let visible = window.tree().find_by_id("file-heading").is_some();
            anyhow::ensure!(visible == (stage != 1), "sidebar visibility did not follow its trigger");
            if stage < 2 {
                let button = window.tree().find_by_id("sidebar-toggle").unwrap();
                window.tree_mut().click(button);
            } else {
                println!("PASS scoped sidebar close/reopen, named properties, reusable children");
                window.close();
            }
            stage += 1;
            Ok(())
        }).run()
}
