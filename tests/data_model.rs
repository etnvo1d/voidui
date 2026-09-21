//! Public API tests use ordinary domain data, independent of the file-tree example.
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};
use voidui::core::widget_tree::WidgetTree;
use voidui::{Callback, List, Read, State, capture, component, div, selection, state, store};

struct Model {
    title: String,
    drops: Arc<std::sync::atomic::AtomicUsize>,
}
impl Drop for Model {
    fn drop(&mut self) {
        self.drops
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

#[component]
fn model_view(model: Read<Model>, on_open: Callback<usize>) -> impl voidui::IntoElement {
    let count = state(|| 0);
    div()
        .child(model.title.clone())
        .child(div().id("open").on_click(move || {
            count.update(|n| *n += 1);
            on_open.call(count.get());
        }))
}
#[test]
fn plain_nonclone_inputs_and_closures_are_normalized_once() {
    let drops = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls = Rc::new(Cell::new(0));
    let output = calls.clone();
    let mut tree = WidgetTree::new();
    tree.build_root(model_view(
        Model {
            title: "Owned".into(),
            drops: drops.clone(),
        },
        move |n| output.set(n),
    ));
    let button = tree.find_by_id("open").unwrap();
    tree.click(button);
    tree.flush_updates();
    tree.click(button);
    tree.flush_updates();
    assert_eq!(calls.get(), 2);
    assert_eq!(drops.load(std::sync::atomic::Ordering::Relaxed), 0);
    drop(tree);
    assert_eq!(drops.load(std::sync::atomic::Ordering::Relaxed), 1);
}
#[test]
fn lists_can_cross_workers_and_retaining_one_item_releases_siblings() {
    let drops = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let output = drops.clone();
    let values: List<Model> = std::thread::spawn(move || {
        (0..10)
            .map(|i| Model {
                title: i.to_string(),
                drops: output.clone(),
            })
            .collect()
    })
    .join()
    .unwrap();
    let item = values.get(3).unwrap();
    drop(values);
    assert_eq!(drops.load(std::sync::atomic::Ordering::Relaxed), 9);
    assert_eq!(item.title, "3");
    drop(item);
    assert_eq!(drops.load(std::sync::atomic::Ordering::Relaxed), 10);
}
#[test]
fn copied_state_handles_cannot_address_reused_slots() {
    let slot = Rc::new(RefCell::new(None));
    let output = slot.clone();
    let mut tree = WidgetTree::new();
    tree.build_root(component(move || {
        let value = state(|| 1usize);
        *output.borrow_mut() = Some(value);
        div()
            .on_click(move || value.set(2))
            .child(div().on_click(move || value.set(3)))
    }));
    let stale: State<usize> = slot.borrow().unwrap();
    tree.build_root(div());
    assert!(!stale.is_mounted());
    let output = slot.clone();
    tree.build_root(component(move || {
        *output.borrow_mut() = Some(state(|| 7));
        div()
    }));
    let live = slot.borrow().unwrap();
    assert!(!stale.same_state(&live));
    stale.update(|_| panic!("stale closures must not execute"));
    assert_eq!(live.get(), 7);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| stale.get())).is_err());
}
#[test]
fn store_updates_only_the_key_reader_and_tracks_absence_and_structure() {
    let slot = Rc::new(RefCell::new(None));
    let output = slot.clone();
    let mut tree = WidgetTree::new();
    tree.build_root(component(move || {
        let values = store(|| (0..1000).map(|i| (i, i)));
        *output.borrow_mut() = Some(values);
        div()
            .children((0..1001).map(|i| {
                component(move || {
                    div().width(values.with(&i, |v| v.copied().unwrap_or_default()) as f32)
                })
            }))
            .child(component(move || div().height(values.len() as f32)))
    }));
    let values = slot.borrow().unwrap();
    values.insert(17, 19);
    assert_eq!(tree.flush_updates(), 1);
    values.insert(1000, 10);
    assert_eq!(tree.flush_updates(), 2);
    values.remove(&1000);
    assert_eq!(tree.flush_updates(), 2);
    values.remove(&1000);
    assert_eq!(tree.flush_updates(), 0);
    assert!(!values.set_if_changed(17, 19));
    assert_eq!(tree.flush_updates(), 0);
    tree.build_root(div());
    assert!(!values.is_mounted());
    assert!(values.insert(0, 1).is_none());
}
#[test]
fn selection_only_updates_previous_and_next_rows() {
    let slot = Rc::new(RefCell::new(None));
    let output = slot.clone();
    let mut tree = WidgetTree::new();
    tree.build_root(component(move || {
        let selected = selection::<usize>();
        *output.borrow_mut() = Some(selected);
        div().children(
            (0..1000).map(|i| {
                component(move || div().width(if selected.is_selected(&i) { 1 } else { 0 }))
            }),
        )
    }));
    let selected = slot.borrow().unwrap();
    selected.select(1);
    assert_eq!(tree.flush_updates(), 1);
    selected.select(999);
    assert_eq!(tree.flush_updates(), 2);
    selected.select(999);
    assert_eq!(tree.flush_updates(), 0);
    selected.clear();
    assert_eq!(tree.flush_updates(), 1);
}
#[component(memo)]
fn fixed(value: usize, calls: Read<Cell<usize>>) -> impl voidui::IntoElement {
    calls.set(calls.get() + 1);
    div().width(value as f32)
}
#[test]
fn memo_skips_unchanged_inputs_but_updates_changed_inputs_and_modifiers() {
    let calls = Read::new(Cell::new(0));
    let mut tree = WidgetTree::new();
    tree.build_root(fixed(1, &calls).id("old"));
    tree.reconcile_root(fixed(1, &calls).id("old"));
    assert_eq!(calls.get(), 1);
    tree.reconcile_root(fixed(2, &calls).id("new"));
    assert_eq!(calls.get(), 2);
    assert!(tree.find_by_id("new").is_some());
    tree.reconcile_root(fixed(2, &calls).class("changed"));
    assert_eq!(calls.get(), 3);
}
#[test]
fn capture_clones_once_and_preserves_original_binding() {
    let calls = Rc::new(Cell::new(0));
    let callback = capture!(calls => move || calls.set(calls.get() + 1));
    callback();
    callback();
    assert_eq!(calls.get(), 2);
    assert_eq!(Rc::strong_count(&calls), 2);
}
#[component(memo)]
fn subscribed(
    value: State<usize>,
    calls: Read<Cell<usize>>,
    on_call: Callback<usize>,
) -> impl voidui::IntoElement {
    calls.set(calls.get() + 1);
    div()
        .width(value.get() as f32)
        .id("subscribed")
        .on_click(move || on_call.call(value.get()))
}
#[test]
fn memo_keeps_state_dependencies() {
    let calls = Read::new(Cell::new(0));
    let reports = Rc::new(RefCell::new(Vec::new()));
    let slot = Rc::new(RefCell::new(None));
    let output = slot.clone();
    let renders = calls.clone();
    let capture = reports.clone();
    let mut tree = WidgetTree::new();
    tree.build_root(component(move || {
        let value = state(|| 0);
        *output.borrow_mut() = Some(value);
        let capture = capture.clone();
        subscribed(value, &renders, move |v| capture.borrow_mut().push(v))
    }));
    let value = slot.borrow().unwrap();
    value.set(2);
    assert_eq!(tree.flush_updates(), 1);
    assert_eq!(calls.get(), 2);
    tree.click(tree.find_by_id("subscribed").unwrap());
    assert_eq!(*reports.borrow(), [2]);
}
#[component(memo)]
fn list_view<T: ToString>(items: List<T>) -> impl voidui::IntoElement {
    div().children(items.iter().map(|item| div().child(item.to_string())))
}
#[test]
fn generic_list_parameters_accept_plain_vectors_and_arrays() {
    let mut tree = WidgetTree::new();
    tree.build_root(list_view::<usize>(vec![1, 2, 3]));
    tree.reconcile_root(list_view::<usize>([3, 4]));
}
#[test]
fn store_snapshots_are_immutable_and_do_not_keep_the_owner_alive() {
    struct Item(String);
    let slot = Rc::new(RefCell::new(None));
    let output = slot.clone();
    let mut tree = WidgetTree::new();
    tree.build_root(component(move || {
        let items = store(|| [(1, Item("old".into()))]);
        *output.borrow_mut() = Some(items);
        div()
    }));
    let items = slot.borrow().unwrap();
    let old = items.get(&1).unwrap();
    items.insert(1, Item("new".into()));
    assert_eq!(old.0, "old");
    assert_eq!(items.get(&1).unwrap().0, "new");
    tree.build_root(div());
    assert!(!items.is_mounted());
    assert_eq!(old.0, "old");
}
#[test]
fn store_edits_clone_only_shared_values_and_notify_after_panics() {
    struct Item {
        value: usize,
        clones: Rc<Cell<usize>>,
    }
    impl Clone for Item {
        fn clone(&self) -> Self {
            self.clones.set(self.clones.get() + 1);
            Self {
                value: self.value,
                clones: self.clones.clone(),
            }
        }
    }
    let clones = Rc::new(Cell::new(0));
    let tracked = clones.clone();
    let slot = Rc::new(RefCell::new(None));
    let output = slot.clone();
    let mut tree = WidgetTree::new();
    tree.build_root(component(move || {
        let items = store(|| {
            [(
                1,
                Item {
                    value: 0,
                    clones: tracked.clone(),
                },
            )]
        });
        *output.borrow_mut() = Some(items);
        div().width(items.with(&1, |item| item.unwrap().value) as f32)
    }));
    let items = slot.borrow().unwrap();
    items.update(&1, |item| item.value = 1);
    assert_eq!(clones.get(), 0);
    assert_eq!(tree.flush_updates(), 1);
    let snapshot = items.get(&1).unwrap();
    items.update(&1, |item| item.value = 2);
    assert_eq!(clones.get(), 1);
    assert_eq!(snapshot.value, 1);
    tree.flush_updates();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        items.update(&1, |item| {
            item.value = 3;
            panic!("partial edit");
        });
    }));
    assert!(panic.is_err());
    assert_eq!(tree.flush_updates(), 1);
    assert_eq!(items.with(&1, |item| item.unwrap().value), 3);
    assert!(!items.update(&2, |_| panic!("missing")));
    assert_eq!(tree.flush_updates(), 0);
}

#[test]
fn memo_refreshes_callback_identity_and_wrapper_events() {
    #[component(memo)]
    fn button(callback: Callback<()>) -> impl voidui::IntoElement {
        div().id("button").on_click(move || callback.call(()))
    }
    let calls = Rc::new(Cell::new(0));
    let old = calls.clone();
    let mut tree = WidgetTree::new();
    let callback = Callback::new(move |()| old.set(old.get() + 1));
    tree.build_root(button(&callback));
    let id = tree.find_by_id("button").unwrap();
    tree.reconcile_root(button(&callback));
    tree.click(id);
    assert_eq!(calls.get(), 1);
    let new = calls.clone();
    tree.reconcile_root(button(move |()| new.set(new.get() + 10)));
    assert_eq!(
        tree.find_by_id("button"),
        Some(id),
        "input conversion must not change component identity"
    );
    tree.click(id);
    assert_eq!(calls.get(), 11);
    let external = calls.clone();
    tree.reconcile_root(button(&callback).on_click(move || external.set(99)));
    tree.click(id);
    assert_eq!(calls.get(), 99);
}
#[test]
fn normalized_inputs_keep_named_component_identity_across_call_site_types() {
    #[component]
    fn view(
        label: Read<String>,
        callback: Callback<()>,
        output: Read<RefCell<Option<State<usize>>>>,
    ) -> impl voidui::IntoElement {
        let count = state(|| 0);
        *output.borrow_mut() = Some(count);
        div().child(label.to_string()).on_click(move || {
            count.update(|v| *v += 1);
            callback.call(());
        })
    }
    let output = Read::new(RefCell::new(None));
    let mut tree = WidgetTree::new();
    let callback = Callback::new(|()| {});
    let first = tree.build_root(view("first", &callback, &output));
    let count = output.borrow().unwrap();
    count.set(7);
    tree.flush_updates();
    tree.reconcile_root(view(String::from("second"), |()| {}, output.clone()));
    assert_eq!(tree.root(), Some(first));
    assert_eq!(output.borrow().unwrap().get(), 7);
    tree.reconcile_root(view(Read::new(String::from("third")), callback, &output));
    assert_eq!(tree.root(), Some(first));
    assert!(count.same_state(&output.borrow().unwrap()));
}
#[test]
fn borrowed_input_is_owned_before_the_callers_buffer_is_dropped() {
    #[component]
    fn label(value: Read<String>) -> impl voidui::IntoElement {
        div().child(value.to_string())
    }
    let value = String::from("temporary");
    let description = label(value.as_str());
    drop(value);
    let mut tree = WidgetTree::new();
    tree.build_root(description);
}

#[test]
fn memoized_parent_does_not_discard_pending_descendant_updates() {
    #[component(memo)]
    fn parent(output: Read<RefCell<Option<State<usize>>>>) -> impl voidui::IntoElement {
        component(move || {
            let value = state(|| 0);
            *output.borrow_mut() = Some(value);
            div().child(value.get().to_string())
        })
    }
    let output = Read::new(RefCell::new(None));
    let mut tree = WidgetTree::new();
    tree.build_root(parent(&output));
    output.borrow().unwrap().set(9);
    tree.reconcile_root(parent(&output));
    assert_eq!(tree.flush_updates(), 1);
}
