//! Lazy disk tree: cargo run --example file_tree -- [directory]
use std::{fmt::Write, path::PathBuf};
use voidui::{
    Application, Callback, IntoElement, List, Read, Selection, Store, WindowOptions, capture,
    component, div, resource, selection, store, tasks::workers, text,
};

struct Node {
    path: PathBuf,
    name: String,
    key: String,
    is_dir: bool,
}

async fn read_directory(path: PathBuf) -> anyhow::Result<List<Node>> {
    workers::blocking(move || {
        let mut nodes = Vec::new();
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let name = entry.file_name();
            // Keys use native bytes so non-UTF-8 names cannot collide after display
            // conversion. file_type does not follow directory symlinks or cycles.
            let mut key = String::new();
            for byte in name.as_encoded_bytes() {
                write!(&mut key, "{byte:02x}").unwrap();
            }
            nodes.push(Node {
                path: entry.path(),
                name: name.to_string_lossy().into_owned(),
                key,
                is_dir: entry.file_type()?.is_dir(),
            });
        }
        nodes.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.path.cmp(&b.path)));
        Ok::<_, std::io::Error>(nodes.into())
    })
    .await?
    .map_err(Into::into)
}

#[derive(Clone, Copy, PartialEq)]
struct TreeState {
    expanded: Store<PathBuf, ()>,
    selected: Selection<PathBuf>,
}

#[component]
fn file_tree(root: PathBuf, on_open: Callback<PathBuf>) -> impl IntoElement {
    let tree = TreeState {
        expanded: store(std::iter::empty),
        selected: selection(),
    };
    directory(root, tree, on_open).class("file-tree")
}

#[component(memo)]
fn directory(path: PathBuf, tree: TreeState, on_open: Callback<PathBuf>) -> impl IntoElement {
    let entries = resource(path, async |path| read_directory(path).await);
    if entries.is_loading() {
        return div().child("Loading…");
    }
    if let Some(error) = entries.error() {
        return div().child(error.to_string()).child(
            div()
                .tag("button")
                .child("Retry")
                .on_click(move || entries.reload()),
        );
    }
    div().children(entries.with(|nodes| {
        nodes
            .into_iter()
            .flat_map(List::iter)
            .map(|node| file_row(&node, tree, &on_open).key(node.key.clone()))
            .collect::<Vec<_>>()
    }))
}

#[component(memo)]
fn file_row(node: Read<Node>, tree: TreeState, on_open: Callback<PathBuf>) -> impl IntoElement {
    let open = node.is_dir && tree.expanded.contains_key(&node.path);
    let selected = tree.selected.is_selected(&node.path);
    let row = div()
        .tag("button")
        .class(if selected {
            "tree-row selected"
        } else {
            "tree-row"
        })
        .child(if node.is_dir {
            if open { "▾ " } else { "▸ " }
        } else {
            "  "
        })
        .child(text(node.name.clone()))
        .on_click(capture!(node, on_open => move || {
            if node.is_dir {
                if tree.expanded.remove(&node.path).is_none() {
                    tree.expanded.insert(node.path.clone(), ());
                }
            } else {
                tree.selected.select(node.path.clone());
                on_open.call(node.path.clone());
            }
        }));
    let mut branch = div().child(row);
    if open {
        // Mounting this child starts the load. Collapsing unmounts it, cancels
        // pending work, and releases its data; reopening reads the directory again.
        branch = branch.child(directory(node.path.clone(), tree, on_open).class("tree-children"));
    }
    branch
}

fn main() -> anyhow::Result<()> {
    let root = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir()?);
    Application::new()
        .window(WindowOptions { title: "Files".into(), ..Default::default() },
            file_tree(root, |path: PathBuf| println!("Open: {}", path.display())))
        .css(".file-tree { padding: 16px; } .tree-row { display: flex; width: 100%; padding: 4px; text-align: left; } .tree-children { padding-left: 16px; } .selected { background: #b8d8ff; }")?
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::RefCell,
        rc::Rc,
        sync::mpsc,
        time::{Duration, Instant},
    };
    use voidui::{
        TaskRuntime,
        core::{widget::WidgetId, widget_tree::WidgetTree},
    };

    fn settle(tree: &mut WidgetTree, runtime: &TaskRuntime, wake: &mpsc::Receiver<()>) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            runtime.tick();
            tree.flush_updates();
            if runtime.stats().active == 0
                && !runtime.has_ready_tasks()
                && !tree.has_pending_updates()
            {
                break;
            }
            if !runtime.has_ready_tasks() && !tree.has_pending_updates() {
                wake.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .unwrap();
            }
            assert!(Instant::now() < deadline, "directory load timed out");
        }
    }
    fn named(tree: &WidgetTree, id: WidgetId, name: &str) -> Option<WidgetId> {
        if tree
            .children(id)
            .iter()
            .any(|&child| tree.text_content(child) == Some(name))
        {
            return Some(id);
        }
        tree.children(id)
            .iter()
            .find_map(|&child| named(tree, child, name))
    }

    #[test]
    fn lazy_expansion_selection_and_reopen_read_real_files() {
        struct Temp(PathBuf);
        impl Drop for Temp {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp =
            Temp(std::env::temp_dir().join(format!("voidui-tree-{}-{unique}", std::process::id())));
        std::fs::create_dir_all(temp.0.join("folder")).unwrap();
        std::fs::write(temp.0.join("folder/note.txt"), "Note").unwrap();
        let runtime = TaskRuntime::default();
        let (send, wake) = mpsc::channel();
        runtime.set_waker(move || {
            let _ = send.send(());
        });
        let opened = Rc::new(RefCell::new(Vec::new()));
        let output = opened.clone();
        let mut tree = WidgetTree::with_task_runtime(runtime.clone());
        tree.build_root(file_tree(temp.0.clone(), move |path| {
            output.borrow_mut().push(path)
        }));
        settle(&mut tree, &runtime, &wake);
        assert!(named(&tree, tree.root().unwrap(), "note.txt").is_none());
        let folder = named(&tree, tree.root().unwrap(), "folder").unwrap();
        tree.click(folder);
        settle(&mut tree, &runtime, &wake);
        let note = named(&tree, tree.root().unwrap(), "note.txt").unwrap();
        tree.click(note);
        settle(&mut tree, &runtime, &wake);
        assert_eq!(*opened.borrow(), [temp.0.join("folder/note.txt")]);
        tree.click(folder);
        settle(&mut tree, &runtime, &wake);
        assert!(named(&tree, tree.root().unwrap(), "note.txt").is_none());
        std::fs::write(temp.0.join("folder/new.txt"), "New").unwrap();
        tree.click(folder);
        settle(&mut tree, &runtime, &wake);
        assert!(named(&tree, tree.root().unwrap(), "new.txt").is_some());
        runtime.shutdown();
    }
}
