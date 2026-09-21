//! Component state and reconciliation run without native windows or a GPU.
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use voidui::{
    State, component,
    core::{
        element::{Element, IntoElement},
        widget_tree::WidgetTree,
    },
    div, state, text,
};

type Handle<T> = Rc<RefCell<Option<State<T>>>>;
fn handle<T>() -> Handle<T> {
    Rc::default()
}
fn read<T>(slot: &Handle<T>) -> State<T> {
    slot.borrow().as_ref().unwrap().clone()
}

#[component]
fn counter(
    name: impl Into<String>,
    out: Handle<usize>,
    renders: Rc<Cell<usize>>,
) -> impl IntoElement {
    renders.set(renders.get() + 1);
    let count = state(|| 0usize);
    *out.borrow_mut() = Some(count.clone());
    text(format!("{}:{}", name.into(), count.get()))
}
fn label(tree: &WidgetTree, id: voidui::core::widget::WidgetId) -> String {
    tree.text_content(id).unwrap().to_owned()
}

#[test]
fn deferred_local_batched_updates_keep_widget_ids() {
    let (left, right) = (handle(), handle());
    let (a, b) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
    let mut tree = WidgetTree::new();
    let wake = Rc::new(Cell::new(0));
    let w = wake.clone();
    tree.set_update_waker(move || w.set(w.get() + 1));
    let pending = div()
        .child(counter("left", left.clone(), a.clone()).id("left"))
        .child(counter("right", right.clone(), b.clone()).id("right"));
    assert_eq!(a.get(), 0);
    tree.build_root(pending);
    let left_id = tree.find_by_id("left").unwrap();
    let right_id = tree.find_by_id("right").unwrap();
    for _ in 0..100 {
        read(&left).update(|n| *n += 1);
    }
    assert_eq!(wake.get(), 1);
    assert_eq!(tree.flush_updates(), 1);
    assert_eq!((a.get(), b.get()), (2, 1));
    assert_eq!(label(&tree, left_id), "left:100");
    assert_eq!(label(&tree, right_id), "right:0");
    assert_eq!(tree.find_by_id("left"), Some(left_id));
    assert_eq!(tree.flush_updates(), 0);
    assert!(!read(&left).set_if_changed(100));
    assert!(!tree.has_pending_updates());
    assert_eq!(
        std::mem::size_of::<State<usize>>(),
        std::mem::size_of::<usize>()
    );
}

#[test]
fn keyed_reorder_new_inputs_unmount_and_reset() {
    let handles = [handle(), handle(), handle()];
    let renders = Rc::new(Cell::new(0));
    let view = |order: &[usize], suffix: &str| {
        div().children(order.iter().map(|&i| {
            counter(format!("{i}{suffix}"), handles[i].clone(), renders.clone())
                .key(i.to_string())
                .id(i.to_string())
        }))
    };
    let mut tree = WidgetTree::new();
    let root = tree.build_root(view(&[0, 1, 2], "a"));
    let ids = [0, 1, 2].map(|i| tree.find_by_id(&i.to_string()).unwrap());
    read(&handles[1]).set(9);
    tree.reconcile_root(view(&[2, 1, 0], "b"));
    assert_eq!(tree.root(), Some(root));
    assert_eq!(tree.children(root), &[ids[2], ids[1], ids[0]]);
    assert_eq!(label(&tree, ids[1]), "1b:9");
    assert_eq!(tree.flush_updates(), 0);
    let stale = read(&handles[1]);
    tree.reconcile_root(view(&[2], "c"));
    assert_eq!(tree.state_count(), 1);
    assert!(!stale.is_mounted());
    stale.update(|_| panic!("stale writes must not run"));
    assert!(!tree.has_pending_updates());
    tree.reconcile_root(view(&[1, 2], "d"));
    assert_eq!(read(&handles[1]).get(), 0);
    assert_ne!(tree.find_by_id("1"), Some(ids[1]));
    let old = read(&handles[2]);
    tree.build_root(view(&[2], "e"));
    assert!(!old.is_mounted());
}

#[component]
fn reader(value: State<usize>, active: bool, renders: Rc<Cell<usize>>) {
    renders.set(renders.get() + 1);
    text(if active {
        value.get().to_string()
    } else {
        "inactive".into()
    })
}
#[component]
fn provider(out: Handle<usize>, renders: Rc<Cell<usize>>, child: Rc<Cell<usize>>) {
    renders.set(renders.get() + 1);
    let value = state(|| 0usize);
    *out.borrow_mut() = Some(value.clone());
    div().child(reader(value, true, child).id("reader"))
}

#[test]
fn shared_state_schedules_readers_not_the_nonreading_owner() {
    let out = handle();
    let (parent, child) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
    let mut tree = WidgetTree::new();
    tree.build_root(provider(out.clone(), parent.clone(), child.clone()));
    read(&out).set(3);
    assert_eq!(tree.flush_updates(), 1);
    assert_eq!((parent.get(), child.get()), (1, 2));
    assert_eq!(label(&tree, tree.find_by_id("reader").unwrap()), "3");
    // A second tree can read the same handle: generational IDs are tree-local,
    // but dependency subscriptions must not collide across those trees.
    let mut second = WidgetTree::new();
    second.build_root(reader(read(&out), true, child.clone()));
    read(&out).set(4);
    assert_eq!(tree.flush_updates(), 1);
    assert_eq!(second.flush_updates(), 1);
    second.reconcile_root(reader(read(&out), false, child));
    read(&out).set(5);
    assert!(tree.has_pending_updates());
    assert!(!second.has_pending_updates());
}

#[component]
fn changing_root(out: Handle<bool>) -> Element {
    let value = state(|| false);
    *out.borrow_mut() = Some(value.clone());
    if value.get() {
        div().child("changed").into_element()
    } else {
        text("initial").into_element()
    }
}
#[component]
fn wrapper(out: Handle<bool>) {
    changing_root(out).class("inner")
}
#[component]
fn outer(out: Handle<bool>) {
    wrapper(out).class("middle")
}

#[test]
fn transparent_wrappers_survive_local_root_replacement() {
    for nested in [false, true] {
        let out = handle();
        let mut tree = WidgetTree::new();
        let view = outer(out.clone()).id("outer").class("outer");
        tree.build_root(if nested {
            div()
                .child("before")
                .child(view)
                .child("after")
                .into_element()
        } else {
            view.into_element()
        });
        let old = tree.find_by_id("outer").unwrap();
        read(&out).set(true);
        assert_eq!(tree.flush_updates(), 1);
        let new = tree.find_by_id("outer").unwrap();
        assert_ne!(old, new);
        assert_eq!(tree.component_count(), 3);
        assert_eq!(tree.state_count(), 1);
        assert_eq!(tree.attribute(new, "class").unwrap(), "inner middle outer");
        if nested {
            assert_eq!(tree.children(tree.root().unwrap())[1], new);
        }
        read(&out).set(false);
        tree.flush_updates();
        let final_id = tree.find_by_id("outer").unwrap();
        assert_eq!(label(&tree, final_id), "initial");
        assert_eq!(
            tree.attribute(final_id, "class").unwrap(),
            "inner middle outer"
        );
        tree.remove_subtree(final_id);
        assert_eq!(tree.component_count(), 0);
        assert!(!read(&out).is_mounted());
    }
}

#[test]
fn type_and_key_changes_reset_state_and_ordinary_nodes_stay_sparse() {
    let out = handle();
    let renders = Rc::new(Cell::new(0));
    let mut tree = WidgetTree::new();
    tree.build_root(counter("x", out.clone(), renders.clone()).key("a"));
    read(&out).set(7);
    tree.reconcile_root(counter("x", out.clone(), renders).key("b"));
    assert_eq!(read(&out).get(), 0);
    tree.reconcile_root(div().children((0..1000).map(|_| div())));
    assert_eq!(tree.component_count(), 0);
    assert_eq!(tree.state_count(), 0);
}

#[test]
fn hooks_validate_count_type_scope_and_render_time_writes() {
    assert!(std::panic::catch_unwind(|| state(|| 0)).is_err());
    let run = |first: u8, second: u8| {
        let mut tree = WidgetTree::new();
        let make = |mode| {
            component(move || {
                if mode != 0 {
                    if mode == 2 {
                        let _ = state(|| "different");
                    } else {
                        let value = state(|| 1);
                        if mode == 3 {
                            value.set(4);
                        }
                    }
                }
                div()
            })
        };
        tree.build_root(make(first));
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tree.reconcile_root(make(second))
        }))
    };
    assert!(run(1, 0).is_err());
    assert!(run(0, 1).is_err());
    assert!(run(1, 2).is_err());
    assert!(run(1, 3).is_err());
    assert!(std::panic::catch_unwind(|| state(|| 0)).is_err());
    assert!(
        std::panic::catch_unwind(
            || WidgetTree::new().build_root(div().child(div().key("x")).child(div().key("x")))
        )
        .is_err()
    );
    let mut clean = WidgetTree::new();
    clean.build_root(component(|| {
        let _ = state(|| 1);
        div()
    }));
    assert_eq!(clean.state_count(), 1);
}

#[test]
fn reusable_closure_can_retain_nonclone_inputs_and_generic_macro_works() {
    struct Owned(String);
    let value = Owned("owned".into());
    let out = handle();
    let slot = out.clone();
    let mut tree = WidgetTree::new();
    tree.build_root(component(move || {
        let state = state(|| 0usize);
        *slot.borrow_mut() = Some(state.clone());
        text(format!("{}:{}", value.0, state.get()))
    }));
    read(&out).set(1);
    tree.flush_updates();
    assert_eq!(label(&tree, tree.root().unwrap()), "owned:1");
    #[component]
    fn generic<T: ToString>(input: T) {
        text(input.to_string())
    }
    tree.reconcile_root(generic(42));
    assert_eq!(label(&tree, tree.root().unwrap()), "42");
}

#[component]
fn clickable(step: usize, out: Handle<usize>, reports: Rc<RefCell<Vec<usize>>>) {
    let value = state(|| 0usize);
    *out.borrow_mut() = Some(value.clone());
    div()
        .child(text(value.get().to_string()).id("label"))
        .on_click(move || {
            value.update(|n| *n += step);
            reports.borrow_mut().push(value.get());
        })
        .tag("button")
        .id("button")
}
#[test]
fn click_callbacks_use_new_inputs_and_respect_disabled_inert_and_stale_nodes() {
    let out = handle();
    let old = Rc::default();
    let new = Rc::new(RefCell::new(Vec::new()));
    let mut tree = WidgetTree::new();
    tree.build_root(clickable(1, out.clone(), old));
    let button = tree.find_by_id("button").unwrap();
    assert!(tree.click(tree.find_by_id("label").unwrap()));
    assert_eq!(tree.flush_updates(), 1);
    assert_eq!(read(&out).get(), 1);
    tree.reconcile_root(clickable(5, out.clone(), new.clone()));
    assert!(tree.click(button));
    tree.flush_updates();
    assert_eq!(*new.borrow(), [6]);
    tree.set_attribute(button, "disabled", Some(""));
    assert!(!tree.click(tree.find_by_id("label").unwrap()));
    tree.set_attribute(button, "disabled", None);
    tree.set_attribute(button, "inert", Some(""));
    assert!(!tree.click(button));
    tree.remove_subtree(button);
    assert!(!tree.click(button));
}

#[component]
fn owner_and_reader(out: Handle<usize>, calls: Rc<Cell<usize>>) {
    calls.set(calls.get() + 1);
    let value = state(|| 0usize);
    *out.borrow_mut() = Some(value.clone());
    div()
        .id(value.get().to_string())
        .child(reader(value, true, calls))
}
#[test]
fn parent_and_child_dirty_work_is_coalesced_and_unmounted_values_are_dropped() {
    let out = handle();
    let calls = Rc::new(Cell::new(0));
    let mut tree = WidgetTree::new();
    tree.build_root(owner_and_reader(out.clone(), calls.clone()));
    read(&out).set(1);
    assert_eq!(tree.flush_updates(), 2);
    assert_eq!(calls.get(), 4);
    let drops = Rc::new(Cell::new(0));
    struct Value(Rc<Cell<usize>>);
    impl Drop for Value {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let handle = handle();
    let slot = handle.clone();
    let count = drops.clone();
    tree.build_root(component(move || {
        let value = state(|| Value(count.clone()));
        value.with(|_| ());
        *slot.borrow_mut() = Some(value);
        div()
    }));
    let stale = read(&handle);
    tree.build_root(div());
    assert_eq!(drops.get(), 1);
    assert!(!stale.is_mounted());
    drop(tree);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| stale.with(|_| ()))).is_err());
}

fn cache() -> voidui::render::TextLayoutCache {
    use std::{borrow::Cow, sync::Arc};
    use voidui::render::{ParleyTextSystem, TextLayoutCache, TextSystem};
    let fonts = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    fonts
        .add_fonts(vec![Cow::Borrowed(include_bytes!(
            "../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
        ))])
        .unwrap();
    TextLayoutCache::new(Arc::new(TextSystem::new(Arc::new(fonts))))
}
fn available() -> voidui::core::layout::Size<voidui::core::layout::AvailableSpace> {
    use voidui::core::layout::{AvailableSpace, Size};
    Size {
        width: AvailableSpace::Definite(500.0),
        height: AvailableSpace::MaxContent,
    }
}
#[test]
fn transparent_css_layout_focus_selection_and_pointer_activation() {
    use voidui::core::selection::SelectionPoint;
    let out = handle();
    let mut tree = WidgetTree::new();
    tree.build_root(div().child(clickable(1, out.clone(), Rc::default())));
    tree.set_stylesheets(vec![
        voidui::style::css::Stylesheet::parse(
            "div > button { width: 200px; height: 40px; } button > text { color: red; }",
        )
        .unwrap(),
    ]);
    let cache = cache();
    tree.layout(available(), &cache);
    let button = tree.find_by_id("button").unwrap();
    let label_id = tree.find_by_id("label").unwrap();
    assert_eq!(tree.bounds(button).size.width, 200.0);
    tree.set_focused(Some(button));
    tree.set_selection(
        SelectionPoint::text(label_id, 0),
        SelectionPoint::text(label_id, 1),
    )
    .unwrap();
    let point = voidui::core::geometry::Point::new(
        tree.bounds(button).origin.x + 4.0,
        tree.bounds(button).origin.y + 4.0,
    );
    tree.pointer_moved(Some(point));
    tree.pointer_pressed(true);
    tree.pointer_pressed(false);
    assert_eq!(read(&out).get(), 1);
    tree.layout(available(), &cache);
    assert_eq!(tree.find_by_id("button"), Some(button));
    assert_eq!(tree.focused(), Some(button));
    assert_eq!(tree.find_by_id("label"), Some(label_id));
    assert_eq!(label(&tree, label_id), "1");
}
#[test]
fn a_render_with_equal_output_preserves_layout_and_selection() {
    use voidui::core::selection::{Selection, SelectionPoint};
    let out = handle();
    let slot = out.clone();
    let mut tree = WidgetTree::new();
    tree.build_root(component(move || {
        let value = state(|| 0);
        value.with(|_| ());
        *slot.borrow_mut() = Some(value);
        text("unchanged")
    }));
    tree.layout(available(), &cache());
    let root = tree.root().unwrap();
    let selection = Selection {
        anchor: SelectionPoint::text(root, 1),
        focus: SelectionPoint::text(root, 4),
    };
    tree.set_selection(selection.anchor, selection.focus)
        .unwrap();
    read(&out).set(1);
    assert_eq!(tree.flush_updates(), 1);
    assert_eq!(
        tree.update_styles(std::time::Instant::now()),
        Default::default()
    );
    assert_eq!(tree.selection(), Some(selection));
    assert_eq!(tree.selected_text(), "nch");
}
#[test]
fn root_replacement_is_safe_through_subtree_layout_and_wrapped_input_changes() {
    let out = handle();
    let mut tree = WidgetTree::new();
    let old = tree.build_root(outer(out.clone()).id("old").class("old"));
    read(&out).set(true);
    tree.layout_subtree(old, available(), Default::default(), &cache());
    assert_ne!(tree.root(), Some(old));
    tree.reconcile_root(outer(out).id("new").class("new"));
    let root = tree.find_by_id("new").unwrap();
    assert_eq!(tree.attribute(root, "class").unwrap(), "inner middle new");
    assert!(tree.find_by_id("old").is_none());
}
#[test]
fn append_mounts_components_and_validates_sibling_keys() {
    let mut tree = WidgetTree::new();
    let root = tree.build_root(div());
    let out = handle();
    let id = tree
        .append_child(root, counter("added", out.clone(), Rc::default()).key("a"))
        .unwrap();
    read(&out).set(2);
    tree.flush_updates();
    assert_eq!(label(&tree, id), "added:2");
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || tree.append_child(root, div().key("a"))
        ))
        .is_err()
    );
    tree.remove_subtree(root);
    assert_eq!(tree.component_count(), 0);
}

#[test]
fn large_shared_fanout_and_conditional_subscriptions_stay_correct() {
    let out = handle();
    let slot = out.clone();
    let calls = Rc::new(Cell::new(0));
    let rendered = calls.clone();
    let mut tree = WidgetTree::new();
    tree.build_root(component(move || {
        let value = state(|| 0usize);
        *slot.borrow_mut() = Some(value.clone());
        div().children((0..2000).map(|_| reader(value.clone(), true, rendered.clone())))
    }));
    read(&out).set(1);
    assert_eq!(tree.flush_updates(), 2000);
    assert_eq!(calls.get(), 4000);
    let root = tree.root().unwrap();
    assert!(
        tree.children(root)
            .iter()
            .all(|&id| tree.text_content(id) == Some("1"))
    );
}

#[test]
fn bulk_unmount_preserves_live_boundaries_and_closes_overlays() {
    use voidui::core::selection::SelectionPoint;
    let mut tree = WidgetTree::new();
    let view = |count: usize| {
        div().children((0..count).map(|i| {
            component(move || {
                let _ = state(|| i);
                div().tag("dialog").child("entry")
            })
            .key(i.to_string())
        }))
    };
    let root = tree.build_root(view(1000));
    tree.layout(available(), &cache());
    let last = tree.children(root)[999];
    tree.show_modal(last).unwrap();
    tree.set_selection(
        SelectionPoint::children(root, 300),
        SelectionPoint::children(root, 999),
    )
    .unwrap();
    tree.reconcile_root(view(1));
    assert_eq!(tree.component_count(), 1);
    assert_eq!(tree.state_count(), 1);
    assert!(tree.active_modal().is_none());
    let range = tree.selection().unwrap();
    assert_eq!(range.anchor, SelectionPoint::children(root, 1));
    assert_eq!(range.focus, SelectionPoint::children(root, 1));
}

#[test]
fn value_destructors_can_schedule_the_next_batch_without_losing_work() {
    struct WriteOnDrop(State<usize>);
    impl Drop for WriteOnDrop {
        fn drop(&mut self) {
            self.0.set(11);
        }
    }
    let show = handle();
    let count = handle();
    let show_slot = show.clone();
    let count_slot = count.clone();
    let mut tree = WidgetTree::new();
    tree.build_root(component(move || {
        let visible = state(|| true);
        *show_slot.borrow_mut() = Some(visible.clone());
        let count = state(|| 0usize);
        *count_slot.borrow_mut() = Some(count.clone());
        let mut root = div().child(reader(count.clone(), true, Rc::default()).id("reader"));
        if visible.get() {
            root = root.child(component(move || {
                let _ = state(|| WriteOnDrop(count.clone()));
                div()
            }));
        }
        root
    }));
    read(&show).set(false);
    tree.flush_updates();
    assert!(tree.has_pending_updates());
    assert_eq!(tree.flush_updates(), 1);
    assert_eq!(label(&tree, tree.find_by_id("reader").unwrap()), "11");
}

#[test]
fn macro_supports_static_borrows_const_generics_and_mutable_parameters() {
    #[component]
    fn borrowed(input: &str) -> impl IntoElement {
        text(input)
    }
    #[component]
    #[allow(clippy::needless_lifetimes)]
    fn explicit<'a>(input: &'a str) -> impl IntoElement {
        text(input)
    }
    #[component]
    fn generic<const N: usize>(mut input: [u8; N]) -> impl IntoElement {
        input.reverse();
        text(format!("{input:?}"))
    }
    let mut tree = WidgetTree::new();
    tree.build_root(borrowed("static"));
    tree.reconcile_root(explicit("still static"));
    tree.reconcile_root(generic([1, 2, 3]));
    assert_eq!(label(&tree, tree.root().unwrap()), "[3, 2, 1]");
    #[component]
    fn retained<T: ToString>(input: T) -> impl IntoElement {
        let saved = state(|| input);
        text(saved.get().to_string())
    }
    #[component]
    fn retained_borrow(input: &str) -> impl IntoElement {
        let saved = state(|| input);
        text(saved.get())
    }
    tree.reconcile_root(retained(String::from("retained generic")));
    assert_eq!(label(&tree, tree.root().unwrap()), "retained generic");
    tree.reconcile_root(retained_borrow("retained static borrow"));
    assert_eq!(label(&tree, tree.root().unwrap()), "retained static borrow");
}

#[test]
fn randomized_keyed_updates_keep_only_surviving_instances() {
    use std::collections::HashMap;
    let handles: Vec<_> = (0..32).map(|_| handle()).collect();
    let calls = Rc::new(Cell::new(0));
    let make = |order: &[usize]| {
        div().children(order.iter().map(|&i| {
            counter(i.to_string(), handles[i].clone(), calls.clone())
                .key(i.to_string())
                .id(i.to_string())
        }))
    };
    let mut tree = WidgetTree::new();
    let mut previous = HashMap::new();
    let mut random = 7u64;
    for round in 0..200 {
        let mut order = Vec::new();
        for i in 0..32 {
            random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
            if random >> 63 != 0 {
                order.push(i);
            }
        }
        if round % 2 == 0 {
            order.reverse();
        }
        tree.reconcile_root(make(&order));
        let mut next = HashMap::new();
        for i in order {
            let id = tree.find_by_id(&i.to_string()).unwrap();
            let value = read(&handles[i]);
            if let Some((old_id, old_value)) = previous.get(&i) {
                assert_eq!(id, *old_id);
                assert_eq!(value.get(), *old_value);
            } else {
                assert_eq!(value.get(), 0);
            }
            value.update(|n| *n += 1);
            next.insert(i, (id, value.get()));
        }
        assert_eq!(tree.flush_updates(), next.len());
        assert_eq!(tree.state_count(), next.len());
        previous = next;
    }
}
