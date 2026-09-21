//! Independent, recursive file trees with function components and shared state.
//! Run normally to interact, or with --smoke to verify native redraws and exit.
use std::{borrow::Cow, cell::RefCell, rc::Rc, sync::Arc};
use voidui::{
    Application, Callback, List, Read, Selection, State, Store, WindowOptions, capture, component,
    core::element::IntoElement, div, selection, state, store, text,
};

type EntryId = usize;
struct Entry {
    id: EntryId,
    name: String,
    children: Option<List<Entry>>,
}
type Entries = List<Entry>;
#[derive(Clone, Copy, PartialEq)]
struct TreeOptions {
    indent: f32,
}

#[component]
fn file_tree(
    entries: List<Entry>,
    scope: Read<String>,
    on_select: Callback<EntryId>,
    options: TreeOptions,
) -> impl IntoElement {
    // Expansion belongs to the tree, so collapsing a branch preserves nested choices.
    let expanded = store(|| std::iter::empty::<(EntryId, ())>());
    let selected = selection::<EntryId>();
    div()
        .class("file-tree")
        .children(entries.iter().map(|entry| {
            file_row(&entry, &scope, 0, options, expanded, selected, &on_select)
                .key(entry.id.to_string())
        }))
}

#[component(memo)]
fn file_row(
    entry: Read<Entry>,
    scope: Read<String>,
    depth: usize,
    options: TreeOptions,
    expanded: Store<EntryId, ()>,
    selected: Selection<EntryId>,
    on_select: Callback<EntryId>,
) -> impl IntoElement {
    let id = entry.id;
    let open = expanded.contains_key(&id);
    let mut row = div()
        .class("file-row")
        .padding_left(depth as f32 * options.indent);
    if selected.is_selected(&id) {
        row = row.class("selected");
    }
    if entry.children.is_some() {
        row = row.child(
            div()
                .tag("button")
                .class("toggle")
                .id(format!("{scope}:toggle:{id}"))
                .child(if open { "−" } else { "+" })
                .on_click(move || {
                    if expanded.remove(&id).is_none() {
                        expanded.insert(id, ());
                    }
                }),
        );
    } else {
        row = row.child(div().class("toggle").child(" "));
    }
    row = row.child(
        div()
            .tag("button")
            .class("entry")
            .id(format!("{scope}:select:{id}"))
            .child(text(entry.name.clone()))
            .on_click(capture!(on_select => move || {
                selected.select(id);
                on_select.call(id);
            })),
    );
    let mut branch = div().child(row);
    if open && let Some(children) = &entry.children {
        branch = branch.children(children.iter().map(|entry| {
            file_row(
                &entry,
                &scope,
                depth + 1,
                options,
                expanded,
                selected,
                &on_select,
            )
            .key(entry.id.to_string())
        }));
    }
    branch
}

#[component]
fn status(out: Rc<RefCell<Option<State<Option<EntryId>>>>>) -> impl IntoElement {
    let selected = state(|| None::<EntryId>);
    *out.borrow_mut() = Some(selected.clone());
    text(
        selected
            .get()
            .map(|id| format!("Selected entry: {id}"))
            .unwrap_or_else(|| "Select a file in either tree".into()),
    )
    .id("status")
}

fn entries() -> Entries {
    let file = |id, name: &str| Entry {
        id,
        name: name.into(),
        children: None,
    };
    vec![
        Entry {
            id: 1,
            name: "src".into(),
            children: Some(
                vec![
                    file(2, "lib.rs"),
                    Entry {
                        id: 3,
                        name: "core".into(),
                        children: Some(vec![file(4, "state.rs")].into()),
                    },
                ]
                .into(),
            ),
        },
        file(5, "README.md"),
    ]
    .into()
}
fn main() -> anyhow::Result<()> {
    let smoke = std::env::args().any(|arg| arg == "--smoke");
    let selected: Rc<RefCell<Option<State<Option<EntryId>>>>> = Rc::default();
    let output = selected.clone();
    let on_select = Callback::new(move |id| {
        if let Some(state) = output.borrow().as_ref() {
            state.set_if_changed(Some(id));
        }
    });
    let options = TreeOptions { indent: 20.0 };
    let root = div()
        .class("app")
        .child(text("Component state").class("heading"))
        .child("Expand and select files. Each tree keeps its own choices.")
        .child(
            div()
                .class("trees")
                .child(file_tree(entries(), "left", on_select.clone(), options).key("left"))
                .child(file_tree(entries(), "right", on_select, options).key("right")),
        )
        .child(status(selected.clone()));
    let fonts = voidui::render::ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    fonts.add_fonts(vec![Cow::Borrowed(include_bytes!(
        "../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
    ))])?;
    let mut stage = 0;
    let mut scenes = 0;
    Application::new().text_system(Arc::new(voidui::render::TextSystem::new(Arc::new(fonts))))
        .window(WindowOptions { title: "Function component state".into(), ..Default::default() }, root)
        .css(".app { padding: 32px; font-family: 'IBM Plex Sans'; font-size: 16px; color: #dce4f0; background: #16202e; height: 100%; } .heading { font-size: 30px; margin-bottom: 12px; } .trees { display: flex; gap: 36px; margin-top: 24px; margin-bottom: 24px; } .file-tree { width: 280px; } .file-row { display: flex; height: 34px; } button { background: transparent; color: inherit; border-width: 0px; text-align: left; } button:hover { background: #33475e; } .toggle { width: 30px; } .entry { flex-grow: 1; } .selected { background: #244c65; }")?
        .on_frame(move |window| {
            if !smoke { return Ok(()); }
            match stage {
                0 => {
                    // Updating a retained handle directly must wake the native
                    // window without tree_mut(), a timer, or a manual redraw.
                    selected.borrow().as_ref().unwrap().set(Some(4));
                }
                1 => {
                    let id = window.tree().find_by_id("status").unwrap();
                    anyhow::ensure!(window.tree().text_content(id) == Some("Selected entry: 4"));
                    let toggle = window.tree().find_by_id("left:toggle:1").unwrap();
                    window.tree_mut().click(toggle);
                }
                2 => {
                    anyhow::ensure!(window.tree().find_by_id("left:select:2").is_some());
                    anyhow::ensure!(window.tree().find_by_id("right:select:2").is_none());
                    scenes = window.stats().scene_builds;
                    window.native_window().request_redraw();
                }
                _ => {
                    anyhow::ensure!(window.stats().scene_builds == scenes, "clean exposure rebuilt the scene");
                    println!("PASS state wakeup, independent trees, native layout/paint, retained scene: {:?}", window.stats());
                    window.close();
                }
            }
            stage += 1;
            Ok(())
        }).run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use voidui::core::widget_tree::WidgetTree;

    #[test]
    fn recursive_expansion_selection_collapse_and_keyed_reorder() {
        let selected = Rc::new(RefCell::new(Vec::new()));
        let calls = selected.clone();
        let callback = Callback::new(move |id| calls.borrow_mut().push(id));
        let make = |reverse: bool| {
            let scopes = if reverse {
                ["right", "left"]
            } else {
                ["left", "right"]
            };
            div().children(scopes.map(|scope| {
                file_tree(
                    entries(),
                    scope.to_string(),
                    callback.clone(),
                    TreeOptions { indent: 20.0 },
                )
                .key(scope)
            }))
        };
        let mut tree = WidgetTree::new();
        tree.build_root(make(false));
        for id in [
            "left:toggle:1",
            "left:toggle:3",
            "left:select:4",
            "left:toggle:1",
            "left:toggle:1",
        ] {
            let id = tree.find_by_id(id).unwrap();
            assert!(tree.click(id));
            tree.flush_updates();
        }
        assert_eq!(*selected.borrow(), [4]);
        let leaf = tree.find_by_id("left:select:4").unwrap();
        assert!(tree.find_by_id("right:select:2").is_none());
        tree.reconcile_root(make(true));
        assert_eq!(tree.find_by_id("left:select:4"), Some(leaf));
        assert_eq!(tree.state_count(), 6);
    }
}
