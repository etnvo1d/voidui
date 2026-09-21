//! Public composition APIs exercised through the headless retained runtime.
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use voidui::{
    Callback, Children, Read, State, component, core::widget_tree::WidgetTree, div,
    provide_context, state, text, try_context, use_context,
};

type Handle<T> = Rc<RefCell<Option<State<T>>>>;
fn save<T: 'static>(out: &Handle<T>, value: State<T>) {
    *out.borrow_mut() = Some(value);
}
fn handle<T: 'static>(out: &Handle<T>) -> State<T> {
    out.borrow().unwrap()
}
fn label(tree: &WidgetTree, id: &str) -> String {
    tree.text_content(tree.find_by_id(id).unwrap())
        .unwrap()
        .to_owned()
}

#[component]
fn panel(
    title: Read<String>,
    #[prop(default = 1)] level: usize,
    #[prop(default)] on_select: Callback<usize>,
    children: Children,
) {
    div()
        .child(text(format!("{title}:{level}")).id("heading"))
        .on_click(move || on_select.call(level))
        .children(children)
}

#[test]
fn named_properties_children_and_modifiers_work_in_any_order() {
    let output = Rc::new(Cell::new(0));
    let calls = output.clone();
    let mut tree = WidgetTree::new();
    let view = panel("Files")
        .id("panel")
        .level(3)
        .child(|| text("first").id("first"))
        .on_select(move |n| calls.set(n))
        .class("frame")
        .child(|| text("second").id("second"));
    tree.build_root(view);
    assert_eq!(label(&tree, "heading"), "Files:3");
    assert_eq!(label(&tree, "first"), "first");
    tree.click(tree.find_by_id("panel").unwrap());
    assert_eq!(output.get(), 3);
    tree.reconcile_root(panel("Default"));
    assert_eq!(label(&tree, "heading"), "Default:1");
    assert!(tree.find_by_id("first").is_none());
}

#[component(memo)]
fn generic_panel<T: Clone + PartialEq + ToString + 'static, const N: usize>(
    value: T,
    #[prop(default = N)] count: usize,
    children: Children,
) {
    div()
        .child(text(format!("{}:{count}", value.to_string())).id("value"))
        .children(children)
}
#[component]
fn anonymous_input(value: impl ToString, #[prop(default = 0)] offset: usize) {
    text(format!("{}:{offset}", value.to_string()))
}
#[test]
fn generic_and_anonymous_inputs_keep_their_types() {
    let mut tree = WidgetTree::new();
    tree.build_root(
        generic_panel::<_, 4>("x")
            .count(7)
            .child(anonymous_input(9).offset(2)),
    );
    assert_eq!(label(&tree, "value"), "x:7");
}

#[derive(Clone, Copy, PartialEq)]
struct Sidebar(State<bool>);
#[component]
fn scope(out: Handle<bool>, children: Children, #[prop(default = true)] default_open: bool) {
    let open = state(|| default_open);
    save(&out, open);
    provide_context(Sidebar(open));
    children.single()
}
#[component(memo)]
fn trigger() {
    let Sidebar(open) = use_context();
    div()
        .tag("button")
        .on_click(move || open.update(|value| *value = !*value))
}
#[component(memo)]
fn content(renders: Rc<Cell<usize>>) {
    renders.set(renders.get() + 1);
    let Sidebar(open) = use_context();
    text(if open.get() { "open" } else { "closed" })
}
#[component(memo)]
fn nested(renders: Rc<Cell<usize>>) {
    div()
        .child(trigger().id("toggle"))
        .child(content(renders).id("content"))
}
#[test]
fn scoped_state_updates_only_readers_and_survives_container_renders() {
    let out = Handle::default();
    let renders = Rc::new(Cell::new(0));
    let view = scope(out.clone()).child(nested(renders.clone()));
    let mut tree = WidgetTree::new();
    let root = tree.build_root(view.clone());
    tree.click(tree.find_by_id("toggle").unwrap());
    assert_eq!(tree.flush_updates(), 1);
    assert_eq!(label(&tree, "content"), "closed");
    assert_eq!(renders.get(), 2);
    tree.reconcile_root(view.default_open(false));
    assert_eq!(tree.root(), Some(root));
    assert_eq!(label(&tree, "content"), "closed");
    assert_eq!(renders.get(), 2);
    assert_eq!(tree.flush_updates(), 0);
    let stale = handle(&out);
    tree.build_root(div());
    assert!(!stale.is_mounted());
}

#[derive(Clone, Copy, PartialEq)]
struct Number(usize);
#[component(memo)]
fn number_reader() {
    text(
        try_context::<Number>()
            .map_or(0, |value| value.0)
            .to_string(),
    )
    .id("number")
}
#[component(memo)]
fn middle() {
    div().child(number_reader())
}
#[component]
fn changing_provider(out: Handle<Option<usize>>) {
    let number = state(|| None::<usize>);
    save(&out, number);
    if let Some(number) = number.get() {
        provide_context(Number(number));
    }
    middle()
}
#[component]
fn outer(out: Handle<Option<usize>>) {
    provide_context(Number(10));
    changing_provider(out)
}
#[test]
fn adding_replacing_removing_context_reconnects_consumers_below_memo() {
    let out = Handle::default();
    let mut tree = WidgetTree::new();
    tree.build_root(outer(out.clone()));
    assert_eq!(label(&tree, "number"), "10");
    for (next, expected) in [(Some(20), "20"), (Some(30), "30"), (None, "10")] {
        handle(&out).set(next);
        tree.flush_updates();
        // Provider replacement reaches consumers even if an intermediate memo skips.
        assert_eq!(label(&tree, "number"), expected);
        assert!(!tree.has_pending_updates());
    }
}

#[component]
fn local_counter(out: Handle<usize>) {
    let value = state(|| 0);
    save(&out, value);
    text(value.get().to_string()).id("counter")
}
#[component]
fn conditional(out: Handle<bool>, children: Children) {
    let show = state(|| true);
    save(&out, show);
    let mut view = div();
    if show.get() {
        view = view.children(children);
    }
    view
}
#[test]
fn children_replay_after_unmount_with_fresh_state_and_new_inputs() {
    let show = Handle::default();
    let count = Handle::default();
    let mut tree = WidgetTree::new();
    let view = conditional(show.clone()).child(local_counter(count.clone()));
    tree.build_root(view.clone());
    let first = handle(&count);
    first.set(7);
    tree.flush_updates();
    assert_eq!(label(&tree, "counter"), "7");
    handle(&show).set(false);
    tree.flush_updates();
    assert!(!first.is_mounted());
    handle(&show).set(true);
    tree.flush_updates();
    assert_eq!(label(&tree, "counter"), "0");
    assert!(handle(&count) != first);
    tree.reconcile_root(conditional(show).child(|| text("replacement").id("counter")));
    assert_eq!(label(&tree, "counter"), "replacement");
}

#[test]
fn reusable_descriptions_mount_independent_state_in_two_trees() {
    let out = Handle::default();
    let child = local_counter(out.clone());
    let mut a = WidgetTree::new();
    let mut b = WidgetTree::new();
    a.build_root(child.clone());
    let first = handle(&out);
    b.build_root(child);
    let second = handle(&out);
    assert!(first != second);
    first.set(8);
    a.flush_updates();
    assert_eq!(label(&a, "counter"), "8");
    assert_eq!(label(&b, "counter"), "0");
}

#[test]
fn context_diagnostics_and_panic_cleanup() {
    assert!(std::panic::catch_unwind(try_context::<Number>).is_err());
    let mut tree = WidgetTree::new();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tree.build_root(component(|| {
                use_context::<Number>();
                div()
            }));
        }))
        .is_err()
    );
    tree.build_root(component(|| {
        assert!(try_context::<Number>().is_none());
        provide_context(Number(1));
        // Providers are visible to descendants, not to their own render.
        assert!(try_context::<Number>().is_none());
        number_reader()
    }));
    assert_eq!(label(&tree, "number"), "1");
}

#[component(memo)]
fn memo_defaults(
    renders: Read<Cell<usize>>,
    #[prop(default)] notify: Callback<()>,
    children: Children,
) {
    renders.set(renders.get() + 1);
    div().on_click(move || notify.call(())).children(children)
}
#[test]
fn omitted_callback_and_children_do_not_invalidate_memo() {
    let renders = Read::new(Cell::new(0));
    let mut tree = WidgetTree::new();
    tree.build_root(memo_defaults(&renders));
    tree.reconcile_root(memo_defaults(&renders));
    assert_eq!(renders.get(), 1);
    tree.reconcile_root(memo_defaults(&renders).child("added"));
    assert_eq!(renders.get(), 2);
}

#[test]
fn two_scopes_and_two_trees_do_not_share_context() {
    let left = Handle::default();
    let right = Handle::default();
    let make = |out| scope(out).child(content(Rc::new(Cell::new(0))));
    let mut a = WidgetTree::new();
    a.build_root(
        div()
            .child(make(left.clone()).id("left"))
            .child(make(right.clone()).id("right")),
    );
    let mut b = WidgetTree::new();
    b.build_root(number_reader());
    handle(&left).set(false);
    a.flush_updates();
    assert_eq!(label(&a, "left"), "closed");
    assert_eq!(label(&a, "right"), "open");
    assert_eq!(label(&b, "number"), "0");
    assert!(!b.has_pending_updates());
}

#[test]
fn conditional_builders_do_not_execute_unused_factories() {
    let mut tree = WidgetTree::new();
    tree.build_root(
        div()
            .when(false, |_| panic!("unused branch"))
            .when(true, |view| view.child(text("yes").id("branch"))),
    );
    assert_eq!(label(&tree, "branch"), "yes");
}

#[test]
fn duplicate_provider_and_initializer_access_are_rejected() {
    let mut tree = WidgetTree::new();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tree.build_root(component(|| {
                provide_context(Number(1));
                provide_context(Number(2));
                div()
            }));
        }))
        .is_err()
    );
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tree.build_root(component(|| {
                state(try_context::<Number>);
                div()
            }));
        }))
        .is_err()
    );
    tree.build_root(number_reader());
    assert_eq!(label(&tree, "number"), "0");
}

// A public builder can be used without importing any generated extension trait.
mod library {
    use voidui::{Children, Read, component, div};
    #[component]
    pub fn card(#[prop(default = "Card".into())] title: Read<String>, children: Children) {
        div().child(title.to_string()).children(children)
    }
}
#[test]
fn exported_builders_preserve_events_and_child_identity() {
    let calls = Rc::new(Cell::new(0));
    let captured = calls.clone();
    let mut tree = WidgetTree::new();
    tree.build_root(
        library::card()
            .on_click(move || captured.set(captured.get() + 1))
            .title("Public")
            .id("card")
            .children(["one", "two"]),
    );
    tree.click(tree.find_by_id("card").unwrap());
    assert_eq!(calls.get(), 1);
}

#[component]
fn keyed_content(out: Handle<usize>, name: Read<String>) {
    let value = state(|| 0);
    save(&out, value);
    text(value.get().to_string()).id(name.to_string())
}
#[component]
fn plain_container(children: Children) {
    div().children(children)
}
#[test]
fn child_keys_preserve_state_across_reordering() {
    let a = Handle::default();
    let b = Handle::default();
    let left = keyed_content(a.clone(), "a").key("a");
    let right = keyed_content(b, "b").key("b");
    let mut tree = WidgetTree::new();
    tree.build_root(plain_container().child(left.clone()).child(right.clone()));
    let id = tree.find_by_id("a").unwrap();
    handle(&a).set(9);
    tree.reconcile_root(plain_container().child(right).child(left));
    assert_eq!(tree.find_by_id("a"), Some(id));
    assert_eq!(label(&tree, "a"), "9");
}

#[test]
fn content_factories_are_deferred_and_receive_container_context() {
    let calls = Rc::new(Cell::new(0));
    let seen = calls.clone();
    let children = Children::new().child(move || {
        seen.set(seen.get() + 1);
        let Number(value) = use_context();
        text(value.to_string()).id("factory")
    });
    let copy = children.clone();
    assert_eq!(calls.get(), 0);
    let mut tree = WidgetTree::new();
    tree.build_root(component(move || {
        provide_context(Number(42));
        div().children(copy.clone())
    }));
    assert_eq!(calls.get(), 1);
    assert_eq!(label(&tree, "factory"), "42");
    tree.build_root(div());
    tree.build_root(component(move || {
        provide_context(Number(43));
        div().children(children.clone())
    }));
    assert_eq!(calls.get(), 2);
    assert_eq!(label(&tree, "factory"), "43");
}

#[component(memo)]
fn named_state(out: Handle<usize>, #[prop(default = "Initial".into())] title: Read<String>) {
    let count = state(|| 0);
    save(&out, count);
    text(format!("{title}:{}", count.get())).id("named")
}
#[test]
fn named_input_conversions_do_not_reset_mounted_state() {
    let out = Handle::default();
    let mut tree = WidgetTree::new();
    tree.build_root(named_state(out.clone()).title("first"));
    let count = handle(&out);
    count.set(6);
    let title = Read::new(String::from("second"));
    tree.reconcile_root(named_state(out.clone()).title(&title));
    assert!(handle(&out) == count);
    assert_eq!(label(&tree, "named"), "second:6");
    assert_eq!(tree.flush_updates(), 0);
}
