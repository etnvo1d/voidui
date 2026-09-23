//! Native frame staging must respect commit-time writes and bounded update batches.
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
    time::Instant,
};
use voidui::core::{
    layout::{AvailableSpace, Size},
    widget_tree::WidgetTree,
};
use voidui::render::{ParleyTextSystem, TextLayoutCache, TextSystem};
use voidui::{State, TaskRuntime, component, div, resource, state};

fn layout(tree: &mut WidgetTree) {
    let cache = TextLayoutCache::new(Arc::new(TextSystem::new(Arc::new(
        ParleyTextSystem::new_without_system_fonts("unused"),
    ))));
    tree.layout_computed(
        Size {
            width: AvailableSpace::Definite(200.),
            height: AvailableSpace::Definite(100.),
        },
        &cache,
    );
}

#[test]
fn resource_restart_defers_layout_until_loading_state_commits() {
    let runtime = TaskRuntime::default();
    let mut tree = WidgetTree::with_task_runtime(runtime.clone());
    let input = Rc::new(RefCell::new(None));
    let captured = input.clone();
    tree.build_root(component(move || {
        let key = state(|| 0usize);
        *captured.borrow_mut() = Some(key);
        let query = resource(key.get(), async |key| Ok::<_, ()>(key));
        div().id(if query.is_loading() {
            "loading"
        } else {
            "ready"
        })
    }));
    runtime.tick();
    assert!(tree.prepare_frame(Instant::now()).is_some());
    layout(&mut tree);
    assert!(tree.find_by_id("ready").is_some());

    input.borrow().unwrap().set(1);
    assert!(tree.prepare_frame(Instant::now()).is_none());
    assert!(tree.has_pending_updates());
    // Consume only the follow-up UI batch: the async result need not be ready.
    assert!(tree.prepare_frame(Instant::now()).is_some());
    layout(&mut tree);
    assert!(tree.find_by_id("loading").is_some());
    runtime.tick();
    assert!(tree.prepare_frame(Instant::now()).is_some());
    layout(&mut tree);
    assert!(tree.find_by_id("ready").is_some());
}

#[test]
fn deferred_frame_keeps_a_paint_only_invalidation() {
    struct OnDrop(State<usize>);
    impl Drop for OnDrop {
        fn drop(&mut self) {
            if self.0.is_mounted() {
                self.0.set(1);
            }
        }
    }
    let show = Rc::new(RefCell::new(None));
    let captured = show.clone();
    let mut tree = WidgetTree::new();
    tree.build_root(component(move || {
        let visible = state(|| true);
        *captured.borrow_mut() = Some(visible);
        let count = state(|| 0usize);
        // Subscribe without changing the rendered output on the second batch.
        let _ = count.get();
        div().child(if visible.get() {
            component(move || {
                let _ = state(|| OnDrop(count));
                div()
            })
        } else {
            component(div)
        })
    }));
    assert!(tree.prepare_frame(Instant::now()).is_some());
    layout(&mut tree);
    let root = tree.root().unwrap();
    // An explicit repaint must survive a deferred batch even if styles/layout
    // produce no further paint change when that batch is consumed.
    tree.invalidator(root).repaint();
    show.borrow().unwrap().set(false);
    assert!(tree.prepare_frame(Instant::now()).is_none());
    assert!(tree.prepare_frame(Instant::now()).unwrap().paint);
    layout(&mut tree);
    assert!(!tree.prepare_frame(Instant::now()).unwrap().paint);
}

#[test]
fn commit_feedback_is_not_drained_in_an_unbounded_frame_loop() {
    struct Next {
        state: State<usize>,
        next: usize,
    }
    impl Drop for Next {
        fn drop(&mut self) {
            if self.state.is_mounted() && self.next <= 4 {
                self.state.set(self.next);
            }
        }
    }
    let input = Rc::new(RefCell::new(None));
    let captured = input.clone();
    let rendered = Rc::new(Cell::new(0));
    let counter = rendered.clone();
    let mut tree = WidgetTree::new();
    tree.build_root(component(move || {
        counter.set(counter.get() + 1);
        let value = state(|| 0usize);
        *captured.borrow_mut() = Some(value);
        let n = value.get();
        div().child(
            component(move || {
                let _ = state(|| Next {
                    state: value,
                    next: n + 2,
                });
                div()
            })
            .key(n.to_string()),
        )
    }));
    assert!(tree.prepare_frame(Instant::now()).is_some());
    layout(&mut tree);
    input.borrow().unwrap().set(1);
    for expected in 2..=4 {
        let before = rendered.get();
        assert!(tree.prepare_frame(Instant::now()).is_none());
        assert_eq!(rendered.get(), before + 1);
        assert_eq!(input.borrow().unwrap().get(), expected);
    }
    assert!(tree.prepare_frame(Instant::now()).is_some());
    layout(&mut tree);
}
