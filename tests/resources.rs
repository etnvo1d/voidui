//! Controlled completions exercise query lifetime without sleeps or native windows.
use futures_channel::oneshot;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use voidui::core::widget_tree::WidgetTree;
use voidui::{
    Resource, ResourceError, TaskError, TaskOptions, TaskRuntime, component, div, resource, state,
};
type Replies = Rc<RefCell<Vec<(usize, oneshot::Sender<Result<usize, String>>)>>>;
type Output = Rc<RefCell<Option<Resource<usize, String>>>>;
#[component]
fn query(input: usize, replies: Replies, output: Output) -> impl voidui::IntoElement {
    let result = resource(input, async move |input| {
        let (send, receive) = oneshot::channel();
        replies.borrow_mut().push((input, send));
        receive.await.unwrap()
    });
    *output.borrow_mut() = Some(result);
    div()
        .width(result.with(|value| value.copied().unwrap_or(0)) as f32)
        .child(if result.is_loading() {
            "Loading"
        } else {
            "Done"
        })
}
#[test]
fn input_changes_cancel_old_work_and_unrelated_renders_do_not_reload() {
    let runtime = TaskRuntime::default();
    let mut tree = WidgetTree::with_task_runtime(runtime.clone());
    let replies: Replies = Rc::default();
    let output: Output = Rc::default();
    tree.build_root(query(1, replies.clone(), output.clone()));
    assert!(replies.borrow().is_empty());
    runtime.tick();
    let (_, old) = replies.borrow_mut().remove(0);
    tree.reconcile_root(query(2, replies.clone(), output.clone()));
    runtime.tick();
    let (input, new) = replies.borrow_mut().remove(0);
    assert_eq!(input, 2);
    assert!(old.send(Ok(1)).is_err());
    new.send(Ok(2)).unwrap();
    runtime.tick();
    tree.flush_updates();
    let result = output.borrow().unwrap();
    assert_eq!(result.with(|v| v.copied()), Some(2));
    assert!(!result.is_loading());
    for _ in 0..5 {
        tree.reconcile_root(query(2, replies.clone(), output.clone()));
        runtime.tick();
        tree.flush_updates();
    }
    assert!(replies.borrow().is_empty());
    assert!(!runtime.has_ready_tasks());
    result.reload();
    assert!(result.is_loading());
    assert_eq!(result.with(|v| v.copied()), Some(2));
    runtime.tick();
    replies
        .borrow_mut()
        .remove(0)
        .1
        .send(Err("offline".into()))
        .unwrap();
    runtime.tick();
    tree.flush_updates();
    assert_eq!(result.error().unwrap().to_string(), "offline");
    assert!(!result.is_loading());
    result.reload();
    runtime.tick();
    let (_, pending) = replies.borrow_mut().remove(0);
    tree.build_root(div());
    assert!(!result.is_mounted());
    assert!(pending.send(Ok(3)).is_err());
    result.reload();
    assert_eq!(runtime.stats().active, 0);
}
#[test]
fn state_input_is_tracked_and_nonclone_results_are_shared() {
    struct Document(String);
    let input = Rc::new(RefCell::new(None));
    let output = Rc::new(RefCell::new(None));
    let input_slot = input.clone();
    let output_slot = output.clone();
    let runtime = TaskRuntime::default();
    let mut tree = WidgetTree::with_task_runtime(runtime.clone());
    tree.build_root(component(move || {
        let name = state(|| "first".to_string());
        *input_slot.borrow_mut() = Some(name);
        let result = resource(name.get(), async |name| Ok::<_, String>(Document(name)));
        *output_slot.borrow_mut() = Some(result);
        div().child(result.with(|v| v.map_or(String::new(), |v| v.0.clone())))
    }));
    runtime.tick();
    tree.flush_updates();
    let result = output.borrow().unwrap();
    let first = result.data().unwrap();
    input.borrow().unwrap().set("second".into());
    tree.flush_updates();
    runtime.tick();
    tree.flush_updates();
    assert_eq!(first.0, "first");
    assert_eq!(result.data().unwrap().0, "second");
}
#[test]
fn failed_render_and_unmount_before_first_poll_do_not_start_loaders() {
    let calls = Rc::new(Cell::new(0));
    let output = calls.clone();
    let runtime = TaskRuntime::default();
    let mut tree = WidgetTree::with_task_runtime(runtime.clone());
    // A sibling panic occurs after the query has registered its commit callback.
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tree.build_root(
            div()
                .child(component(move || {
                    let calls = output.clone();
                    let _ = resource((), async move |()| {
                        calls.set(calls.get() + 1);
                        Ok::<_, String>(())
                    });
                    div()
                }))
                .child(component(|| -> voidui::Element { panic!("render failed") })),
        );
    }));
    assert!(failed.is_err());
    runtime.tick();
    assert_eq!(calls.get(), 0);
    let output = calls.clone();
    tree.build_root(component(move || {
        let calls = output.clone();
        let _ = resource((), async move |()| {
            calls.set(calls.get() + 1);
            Ok::<_, String>(())
        });
        div()
    }));
    tree.build_root(div());
    runtime.tick();
    assert_eq!(calls.get(), 0);
}
#[test]
fn capacity_panics_and_runtime_shutdown_publish_errors() {
    let runtime = TaskRuntime::new(TaskOptions {
        max_tasks: 1,
        ..Default::default()
    })
    .unwrap();
    let scope = runtime.scope();
    scope.spawn(std::future::pending::<()>());
    let output: Output = Rc::default();
    let replies: Replies = Rc::default();
    let mut tree = WidgetTree::with_task_runtime(runtime.clone());
    tree.build_root(query(1, replies.clone(), output.clone()));
    let result = output.borrow().unwrap();
    assert!(!result.is_loading());
    assert!(matches!(
        &*result.error().unwrap(),
        ResourceError::Task(TaskError::AtCapacity)
    ));
    scope.cancel_all();
    runtime.tick();
    result.reload();
    runtime.tick();
    assert!(result.is_loading());
    runtime.shutdown();
    assert!(!result.is_loading());
    assert!(matches!(
        &*result.error().unwrap(),
        ResourceError::Task(TaskError::Cancelled)
    ));
    let runtime = TaskRuntime::default();
    let slot = output.clone();
    let mut tree = WidgetTree::with_task_runtime(runtime.clone());
    tree.build_root(component(move || {
        *slot.borrow_mut() = Some(resource((), async |()| -> Result<usize, String> {
            panic!("loader panic")
        }));
        div()
    }));
    runtime.tick();
    assert!(
        matches!(&*output.borrow().unwrap().error().unwrap(), ResourceError::Task(TaskError::Panicked(message)) if message == "loader panic")
    );
}
#[test]
fn reload_uses_latest_committed_callback() {
    #[component]
    fn view(version: usize, output: Output) -> impl voidui::IntoElement {
        let result = resource((), async move |()| Ok::<_, String>(version));
        *output.borrow_mut() = Some(result);
        div()
    }
    let output: Output = Rc::default();
    let runtime = TaskRuntime::default();
    let mut tree = WidgetTree::with_task_runtime(runtime.clone());
    tree.build_root(view(1, output.clone()));
    runtime.tick();
    tree.reconcile_root(view(2, output.clone()));
    runtime.tick();
    let result = output.borrow().unwrap();
    assert_eq!(*result.data().unwrap(), 1);
    result.reload();
    runtime.tick();
    assert_eq!(*result.data().unwrap(), 2);
}
#[test]
fn resource_identity_loader_infers_input_type() {
    let mut tree = WidgetTree::new();
    tree.build_root(component(|| {
        let value = state(|| String::from("value"));
        let result = resource(value.get(), async |value| Ok::<_, String>(value));
        div().child(result.with(|value| value.cloned().unwrap_or_default()))
    }));
}

#[test]
fn failed_input_update_keeps_the_previous_committed_request() {
    let runtime = TaskRuntime::default();
    let mut tree = WidgetTree::with_task_runtime(runtime.clone());
    let replies: Replies = Rc::default();
    let output: Output = Rc::default();
    tree.build_root(div().child(query(1, replies.clone(), output.clone()).key("query")));
    runtime.tick();
    let (_, first) = replies.borrow_mut().remove(0);
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tree.reconcile_root(
            div()
                .child(query(2, replies.clone(), output.clone()).key("query"))
                .child(component(|| -> voidui::Element {
                    panic!("sibling failed");
                })),
        );
    }));
    assert!(failed.is_err());
    runtime.tick();
    assert!(replies.borrow().is_empty());
    first.send(Ok(1)).unwrap();
    runtime.tick();
    assert_eq!(*output.borrow().unwrap().data().unwrap(), 1);
    // As with any caught render failure, reset the tree before further rendering.
    tree.build_root(div());
}
