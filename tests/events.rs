//! General event routing and drag ownership run against the retained tree with
//! real layout, but do not require a native window or an async I/O backend.
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
    time::Instant,
};
use voidui::{
    ClickEvent, ClickSource, DragEvent, DragPhase, EventHandler, EventResponse as R, IntoElement,
    KeyEvent, KeyEventType, MouseButton as B, MouseButtons, MouseEvent, MouseEventType as M,
    MouseWheel, TaskRuntime, WheelUnit, component,
    core::{
        event::{Key, ModifiersState as Mods, NamedKey},
        geometry::Point,
        layout::{AvailableSpace, Size},
        widget::WidgetId,
        widget_tree::WidgetTree,
    },
    div,
    render::{ParleyTextSystem, TextLayoutCache, TextSystem},
    style::{css::Stylesheet, selection::Cursor},
    tasks::time,
};
use winit::window::CursorIcon;

fn cache() -> TextLayoutCache {
    let fonts = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    fonts
        .add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!(
            "../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
        ))])
        .unwrap();
    TextLayoutCache::new(Arc::new(TextSystem::new(Arc::new(fonts))))
}

fn layout(tree: &mut WidgetTree) {
    tree.layout(
        Size {
            width: AvailableSpace::Definite(400.0),
            height: AvailableSpace::Definite(200.0),
        },
        &cache(),
    );
}
fn build(view: impl IntoElement) -> WidgetTree {
    let mut tree = WidgetTree::new();
    tree.build_root(view);
    layout(&mut tree);
    tree
}
fn id(tree: &WidgetTree, name: &str) -> WidgetId {
    tree.find_by_id(name).unwrap()
}
fn point(x: f32, y: f32) -> Option<Point<f32>> {
    Some(Point::new(x, y))
}
fn pump(runtime: &TaskRuntime) {
    for _ in 0..20 {
        if !runtime.has_ready_tasks() {
            return;
        }
        runtime.tick();
    }
    panic!("event tasks did not settle");
}
fn canvas(child: impl IntoElement) -> impl IntoElement {
    div().width(400.0).height(200.0).flex().child(child).child(
        div()
            .id("other")
            .width(100.0)
            .height(100.0)
            .cursor(Cursor::Icon(CursorIcon::Crosshair)),
    )
}

#[test]
fn all_mouse_methods_share_one_builder_and_sync_calls_create_no_tasks() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let listener = || {
        let seen = seen.clone();
        move |e: MouseEvent| {
            seen.borrow_mut().push(e);
        }
    };
    let source = div()
        .id("source")
        .width(100.0)
        .height(100.0)
        .on_mouse_enter(listener())
        .on_mouse_leave(listener())
        .on_mouse_down(listener())
        .on_mouse_up(listener())
        .on_mouse_move(listener())
        .on_mouse_scroll(listener())
        .child(div().width(20.0).height(20.0));
    let mut tree = build(canvas(source));
    tree.pointer_moved(point(50.0, 50.0));
    tree.dispatch_mouse_button(B::Right, true, Mods::SHIFT);
    tree.dispatch_mouse_scroll(
        MouseWheel {
            x: 2.0,
            y: -3.0,
            unit: WheelUnit::Lines,
        },
        Mods::SHIFT,
    );
    tree.dispatch_mouse_button(B::Right, false, Mods::SHIFT);
    tree.pointer_moved(point(150.0, 50.0));
    let seen = seen.borrow();
    assert_eq!(
        seen.iter().map(|e| e.state).collect::<Vec<_>>(),
        [
            M::Entered,
            M::Moved,
            M::Pressed,
            M::Scrolled,
            M::Released,
            M::Left
        ]
    );
    assert_eq!(seen[2].button, Some(B::Right));
    assert!(seen[2].buttons.contains(MouseButtons::RIGHT));
    assert_eq!(seen[3].wheel.unit, WheelUnit::Lines);
    assert_eq!(seen[3].wheel.y, -3.0);
    assert!(!seen[4].buttons.contains(MouseButtons::RIGHT));
    assert_eq!(seen[4].modifiers, Mods::SHIFT);
    assert_eq!(tree.task_runtime().stats().spawned, 0);
}

#[test]
fn hover_transitions_use_common_ancestors_and_do_not_repeat_on_inner_motion() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let enter = |name| {
        let seen = seen.clone();
        move || {
            seen.borrow_mut().push((name, true));
        }
    };
    let leave = |name| {
        let seen = seen.clone();
        move || {
            seen.borrow_mut().push((name, false));
        }
    };
    let mut tree = build(
        div()
            .width(400.0)
            .height(200.0)
            .flex()
            .on_mouse_enter(enter("parent"))
            .on_mouse_leave(leave("parent"))
            .child(
                div()
                    .width(100.0)
                    .height(100.0)
                    .on_mouse_enter(enter("a"))
                    .on_mouse_leave(leave("a")),
            )
            .child(
                div()
                    .width(100.0)
                    .height(100.0)
                    .on_mouse_enter(enter("b"))
                    .on_mouse_leave(leave("b")),
            ),
    );
    tree.pointer_moved(point(50.0, 50.0));
    tree.pointer_moved(point(51.0, 51.0));
    tree.pointer_moved(point(150.0, 50.0));
    tree.pointer_moved(None);
    assert_eq!(
        *seen.borrow(),
        [
            ("parent", true),
            ("a", true),
            ("a", false),
            ("b", true),
            ("b", false),
            ("parent", false)
        ]
    );
}

#[test]
fn pointer_events_bubble_with_target_and_receiver_local_coordinates() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let listener = || {
        let seen = seen.clone();
        move |e: MouseEvent| {
            seen.borrow_mut().push(e);
        }
    };
    let mut tree = build(
        div()
            .id("parent")
            .width(400.0)
            .height(200.0)
            .padding(20.0)
            .on_mouse_down(listener())
            .child(
                div()
                    .id("child")
                    .width(100.0)
                    .height(100.0)
                    .on_mouse_down(listener()),
            ),
    );
    let child = id(&tree, "child");
    let parent = id(&tree, "parent");
    tree.pointer_moved(point(30.0, 35.0));
    tree.pointer_pressed(true);
    let seen = seen.borrow();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].target, child);
    assert_eq!(seen[1].target, child);
    assert_eq!(seen[0].current_target, child);
    assert_eq!(seen[1].current_target, parent);
    assert_eq!(seen[0].local_position, Point::new(10.0, 15.0));
    assert_eq!(seen[1].local_position, Point::new(30.0, 35.0));
}

#[test]
fn propagation_and_default_prevention_are_independent_synchronous_decisions() {
    let calls = Rc::new(Cell::new(0));
    let out = calls.clone();
    let mut tree = build(
        div()
            .width(400.0)
            .height(200.0)
            .on_mouse_down(move || {
                out.set(out.get() + 1);
            })
            .child(
                div()
                    .id("button")
                    .tag("button")
                    .width(100.0)
                    .height(100.0)
                    .on_mouse_down(|| R::PREVENT_DEFAULT),
            ),
    );
    tree.pointer_moved(point(50.0, 50.0));
    let response = tree.dispatch_mouse_button(B::Left, true, Mods::empty());
    assert!(response.response.prevent_default);
    assert!(!response.response.stop_propagation);
    assert_eq!(calls.get(), 1);
    assert_eq!(tree.focused(), None);
    tree.pointer_pressed(false);
    let out = calls.clone();
    tree.reconcile_root(
        div()
            .width(400.0)
            .height(200.0)
            .on_mouse_down(move || {
                out.set(out.get() + 1);
            })
            .child(
                div()
                    .id("button")
                    .tag("button")
                    .width(100.0)
                    .height(100.0)
                    .on_mouse_down(|| R::STOP_PROPAGATION),
            ),
    );
    layout(&mut tree);
    tree.pointer_moved(point(50.0, 50.0));
    tree.pointer_pressed(true);
    assert_eq!(calls.get(), 1);
    assert_eq!(tree.focused(), Some(id(&tree, "button")));
}

#[test]
fn unified_click_accepts_sync_async_and_future_returning_callbacks() {
    let seen = Rc::new(Cell::new(0));
    let a = seen.clone();
    let b = seen.clone();
    let c = seen.clone();
    let mut tree = build(
        div()
            .child(div().id("a").on_click(move || a.set(a.get() + 1)))
            .child(div().id("b").on_click(async move || {
                time::yield_now().await;
                b.set(b.get() + 10);
            }))
            .child(div().id("c").on_click(move || {
                let c = c.clone();
                async move {
                    c.set(c.get() + 100);
                }
            })),
    );
    for name in ["a", "b", "c"] {
        tree.click(id(&tree, name));
    }
    assert_eq!(seen.get(), 1);
    pump(tree.task_runtime());
    assert_eq!(seen.get(), 111);
}

#[test]
fn click_bubbles_and_descriptions_keep_their_original_widget_type() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let listen = || {
        let events = events.clone();
        move |e: ClickEvent| {
            events.borrow_mut().push(e);
        }
    };
    let mut tree = build(
        div()
            .on_click(listen())
            .id("parent")
            .child(div().id("child").on_click(listen())),
    );
    let child = id(&tree, "child");
    tree.click(child);
    assert_eq!(events.borrow().len(), 2);
    assert_eq!(events.borrow()[0].source, ClickSource::Programmatic);
    tree.reconcile_root(div().id("parent").child(div().id("child")));
    assert_eq!(id(&tree, "child"), child);
    assert!(!tree.click(child));
}

#[test]
fn general_async_events_are_deferred_owned_and_cancelled_with_the_node() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let out = seen.clone();
    let mut tree = build(canvas(
        div().id("source").width(100.0).height(100.0).on_mouse_move(
            async move |event: MouseEvent| {
                time::yield_now().await;
                out.borrow_mut().push(event.position);
            },
        ),
    ));
    tree.pointer_moved(point(50.0, 50.0));
    tree.pointer_moved(point(60.0, 50.0));
    assert!(seen.borrow().is_empty());
    pump(tree.task_runtime());
    assert_eq!(
        *seen.borrow(),
        [Point::new(50.0, 50.0), Point::new(60.0, 50.0)]
    );
    tree.pointer_moved(point(70.0, 50.0));
    tree.task_runtime().tick();
    tree.remove_subtree(id(&tree, "source"));
    pump(tree.task_runtime());
    assert_eq!(seen.borrow().len(), 2);
    assert_eq!(tree.task_runtime().stats().active, 0);
}

#[test]
fn reusable_handler_requires_no_widget_or_event_specific_async_implementation() {
    let runtime = TaskRuntime::default();
    let values = Rc::new(RefCell::new(Vec::new()));
    let output = values.clone();
    let mut handler = EventHandler::<String>::new(async move |value: String| {
        time::yield_now().await;
        output.borrow_mut().push(value);
    });
    let mut other = handler.clone();
    handler.dispatch("cancelled".into(), &runtime);
    other.dispatch("kept".into(), &runtime);
    runtime.tick();
    handler.cancel();
    pump(&runtime);
    assert_eq!(*values.borrow(), ["kept"]);
    runtime.shutdown();
}

#[test]
fn key_down_and_up_bubble_from_focus_including_repeats_and_modifiers() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let listen = || {
        let seen = seen.clone();
        move |event: KeyEvent| {
            seen.borrow_mut().push(event);
        }
    };
    let mut tree = build(
        div()
            .id("root")
            .on_key_down(listen())
            .on_key_up(listen())
            .child(
                div()
                    .id("focus")
                    .attr("tabindex", "0")
                    .on_key_down(listen())
                    .on_key_up(listen()),
            ),
    );
    let focus = id(&tree, "focus");
    tree.set_focused(Some(focus));
    tree.dispatch_key(
        Key::Named(NamedKey::ArrowLeft),
        KeyEventType::Pressed,
        Mods::SHIFT,
        true,
    );
    tree.dispatch_key(
        Key::Named(NamedKey::ArrowLeft),
        KeyEventType::Released,
        Mods::SHIFT,
        false,
    );
    let seen = seen.borrow();
    assert_eq!(seen.len(), 4);
    assert_eq!(seen[0].target, focus);
    assert_eq!(seen[0].current_target, focus);
    assert_eq!(seen[1].current_target, id(&tree, "root"));
    assert!(seen[0].repeat);
    assert_eq!(seen[0].modifiers, Mods::SHIFT);
    assert_eq!(seen[2].state, KeyEventType::Released);
}

#[test]
fn key_default_prevention_works_and_async_key_handlers_share_the_adapter() {
    let seen = Rc::new(Cell::new(false));
    let out = seen.clone();
    let mut tree = build(
        div()
            .on_key_down(|| R::HANDLED)
            .on_key_up(async move |e: KeyEvent| {
                time::yield_now().await;
                out.set(e.state == KeyEventType::Released);
            }),
    );
    assert_eq!(
        tree.dispatch_key(
            Key::Named(NamedKey::Tab),
            KeyEventType::Pressed,
            Mods::empty(),
            false
        ),
        R::HANDLED
    );
    tree.dispatch_key(
        Key::Named(NamedKey::Tab),
        KeyEventType::Released,
        Mods::empty(),
        false,
    );
    assert!(!seen.get());
    pump(tree.task_runtime());
    assert!(seen.get());
}

#[test]
fn drag_captures_outside_element_and_viewport_and_locks_the_owner_cursor() {
    let drag = Rc::new(RefCell::new(Vec::new()));
    let out = drag.clone();
    let ups = Rc::new(RefCell::new(Vec::new()));
    let up = ups.clone();
    let moves = Rc::new(RefCell::new(Vec::new()));
    let movement = moves.clone();
    let mut tree = build(canvas(
        div()
            .id("source")
            .width(100.0)
            .height(100.0)
            .cursor(Cursor::Icon(CursorIcon::Grab))
            .on_drag(move |e: DragEvent| {
                out.borrow_mut().push(e);
            })
            .on_mouse_up(move |e: MouseEvent| {
                up.borrow_mut().push(e);
            })
            .on_mouse_move(move |e: MouseEvent| {
                movement.borrow_mut().push(e);
            }),
    ));
    let owner = id(&tree, "source");
    tree.pointer_moved(point(50.0, 50.0));
    tree.pointer_pressed(true);
    assert_eq!(tree.pointer_capture(), Some(owner));
    tree.pointer_moved(point(150.0, 50.0));
    assert_eq!(tree.pointer_cursor(), Cursor::Icon(CursorIcon::Grab));
    tree.pointer_moved(None);
    assert_eq!(tree.pointer_capture(), Some(owner));
    assert_eq!(tree.pointer_cursor(), Cursor::Icon(CursorIcon::Grab));
    tree.pointer_moved(point(500.0, -20.0));
    tree.pointer_pressed(false);
    assert_eq!(tree.pointer_capture(), None);
    let drag = drag.borrow();
    assert_eq!(
        drag.iter().map(|e| e.phase).collect::<Vec<_>>(),
        [
            DragPhase::Start,
            DragPhase::Move,
            DragPhase::Move,
            DragPhase::End
        ]
    );
    assert!(drag.iter().all(|e| e.target == owner));
    assert_eq!(drag[2].position, Point::new(500.0, -20.0));
    assert_eq!(drag[2].delta, Point::new(350.0, -70.0));
    assert_eq!(drag[2].total_delta, Point::new(450.0, -70.0));
    assert_eq!(ups.borrow()[0].target, owner);
    assert_eq!(ups.borrow()[0].position, Point::new(500.0, -20.0));
    assert_eq!(moves.borrow().last().unwrap().target, owner);
}

#[test]
fn capture_cursor_tracks_owner_active_style_and_restores_underlying_hover_on_release() {
    let mut tree = build(canvas(
        div().id("source").width(100.0).height(100.0).on_drag(|| {}),
    ));
    tree.set_stylesheets(vec![
        Stylesheet::parse("#source {cursor: grab} #source:active {cursor: grabbing}").unwrap(),
    ]);
    layout(&mut tree);
    tree.pointer_moved(point(50.0, 50.0));
    tree.pointer_pressed(true);
    tree.update_styles(Instant::now());
    tree.pointer_moved(point(150.0, 50.0));
    assert_eq!(tree.pointer_cursor(), Cursor::Icon(CursorIcon::Grabbing));
    let owner = id(&tree, "source");
    tree.style_mut(owner).cursor = Cursor::None.into();
    tree.update_styles(Instant::now());
    assert_eq!(tree.pointer_cursor(), Cursor::None);
    layout(&mut tree);
    tree.pointer_pressed(false);
    tree.update_styles(Instant::now());
    layout(&mut tree);
    assert_eq!(tree.pointer_cursor(), Cursor::Icon(CursorIcon::Crosshair));
}

#[test]
fn capture_survives_pending_layout_and_keeps_absolute_deltas_stable() {
    let values = Rc::new(RefCell::new(Vec::new()));
    let out = values.clone();
    let mut tree = build(canvas(
        div()
            .id("source")
            .width(100.0)
            .height(100.0)
            .on_drag(move |e: DragEvent| {
                out.borrow_mut().push(e);
            }),
    ));
    let owner = id(&tree, "source");
    tree.pointer_moved(point(10.0, 10.0));
    tree.pointer_pressed(true);
    tree.style_mut(owner).layout.size.width = voidui::core::layout::length(130);
    tree.pointer_moved(point(180.0, 150.0));
    assert_eq!(tree.pointer_capture(), Some(owner));
    assert_eq!(
        values.borrow().last().unwrap().total_delta,
        Point::new(170.0, 140.0)
    );
    tree.pointer_pressed(false);
    assert_eq!(tree.pointer_capture(), None);
}

#[test]
fn nearest_drag_owner_wins_and_other_buttons_do_not_steal_or_release_capture() {
    let parent = Rc::new(Cell::new(0));
    let p = parent.clone();
    let child = Rc::new(Cell::new(0));
    let c = child.clone();
    let mut tree = build(
        div()
            .width(400.0)
            .height(200.0)
            .on_drag(move || p.set(p.get() + 1))
            .child(
                div()
                    .id("child")
                    .width(100.0)
                    .height(100.0)
                    .on_drag(move || c.set(c.get() + 1)),
            ),
    );
    tree.pointer_moved(point(50.0, 50.0));
    tree.pointer_pressed(true);
    let owner = id(&tree, "child");
    assert_eq!(tree.pointer_capture(), Some(owner));
    tree.dispatch_mouse_button(B::Right, true, Mods::empty());
    tree.dispatch_mouse_button(B::Right, false, Mods::empty());
    assert_eq!(tree.pointer_capture(), Some(owner));
    tree.pointer_pressed(false);
    assert_eq!(parent.get(), 0);
    assert_eq!(child.get(), 2);
}

#[test]
fn motion_suppresses_click_but_an_unmoved_drag_press_can_still_click() {
    let clicks = Rc::new(Cell::new(0));
    let out = clicks.clone();
    let mut tree = build(canvas(
        div()
            .id("source")
            .width(100.0)
            .height(100.0)
            .on_drag(|| {})
            .on_click(move || out.set(out.get() + 1)),
    ));
    tree.pointer_moved(point(50.0, 50.0));
    tree.pointer_pressed(true);
    tree.pointer_moved(point(60.0, 50.0));
    tree.pointer_pressed(false);
    assert_eq!(clicks.get(), 0);
    tree.pointer_pressed(true);
    tree.pointer_pressed(false);
    assert_eq!(clicks.get(), 1);
}

#[test]
fn cancellation_clears_capture_without_synthesizing_up_or_click() {
    let phases = Rc::new(RefCell::new(Vec::new()));
    let out = phases.clone();
    let clicks = Rc::new(Cell::new(0));
    let click = clicks.clone();
    let ups = Rc::new(Cell::new(0));
    let up = ups.clone();
    let mut tree = build(canvas(
        div()
            .id("source")
            .width(100.0)
            .height(100.0)
            .on_drag(move |e: DragEvent| {
                out.borrow_mut().push(e.phase);
            })
            .on_click(move || click.set(click.get() + 1))
            .on_mouse_up(move || up.set(up.get() + 1)),
    ));
    tree.pointer_moved(point(50.0, 50.0));
    tree.pointer_pressed(true);
    tree.pointer_moved(None);
    tree.cancel_pointer_capture();
    assert_eq!(*phases.borrow(), [DragPhase::Start, DragPhase::Cancel]);
    assert_eq!(tree.pointer_capture(), None);
    assert_eq!(clicks.get(), 0);
    assert_eq!(ups.get(), 0);
}

#[test]
fn unmount_disable_inert_and_hidden_styles_cancel_capture() {
    for condition in ["remove", "disabled", "inert", "hidden"] {
        let phases = Rc::new(RefCell::new(Vec::new()));
        let out = phases.clone();
        let mut tree = build(canvas(
            div()
                .id("source")
                .width(100.0)
                .height(100.0)
                .on_drag(move |e: DragEvent| {
                    out.borrow_mut().push(e.phase);
                }),
        ));
        let owner = id(&tree, "source");
        tree.pointer_moved(point(50.0, 50.0));
        tree.pointer_pressed(true);
        match condition {
            "remove" => {
                tree.remove_subtree(owner);
            }
            "disabled" => tree.set_attribute(owner, "disabled", Some("")),
            "inert" => tree.set_attribute(owner, "inert", Some("")),
            _ => {
                tree.set_stylesheets(vec![Stylesheet::parse("#source { display:none }").unwrap()]);
                tree.update_styles(Instant::now());
            }
        }
        assert_eq!(tree.pointer_capture(), None, "{condition}");
        assert_eq!(
            *phases.borrow(),
            [DragPhase::Start, DragPhase::Cancel],
            "{condition}"
        );
    }
}

#[test]
fn removing_drag_binding_cancels_but_reordering_and_replacing_callback_preserve_capture() {
    let phases = Rc::new(RefCell::new(Vec::new()));
    let make = |drag: bool, reverse: bool| {
        let out = phases.clone();
        let mut source = div().id("source").key("source").width(100.0).height(100.0);
        if drag {
            source = source.on_drag(move |e: DragEvent| {
                out.borrow_mut().push(e.phase);
            });
        }
        let other = div().key("other").width(100.0).height(100.0);
        div()
            .width(400.0)
            .height(200.0)
            .flex()
            .children(if reverse {
                vec![other, source]
            } else {
                vec![source, other]
            })
    };
    let mut tree = build(make(true, false));
    let owner = id(&tree, "source");
    tree.pointer_moved(point(50.0, 50.0));
    tree.pointer_pressed(true);
    tree.reconcile_root(make(true, true));
    layout(&mut tree);
    assert_eq!(tree.pointer_capture(), Some(owner));
    tree.reconcile_root(make(false, true));
    assert_eq!(tree.pointer_capture(), None);
    assert_eq!(*phases.borrow(), [DragPhase::Start, DragPhase::Cancel]);
}

#[test]
fn all_events_are_suppressed_by_disabled_ancestors_and_pointer_events_none() {
    let calls = Rc::new(Cell::new(0));
    let out = calls.clone();
    let mut tree = build(
        div().width(400.0).height(200.0).attr("disabled", "").child(
            div()
                .id("child")
                .width(100.0)
                .height(100.0)
                .on_mouse_move(move || out.set(out.get() + 1))
                .on_drag(|| {}),
        ),
    );
    tree.pointer_moved(point(50.0, 50.0));
    tree.pointer_pressed(true);
    assert_eq!(calls.get(), 0);
    assert_eq!(tree.pointer_capture(), None);
    tree.set_attribute(tree.root().unwrap(), "disabled", None);
    tree.set_stylesheets(vec![
        Stylesheet::parse("#child { pointer-events:none }").unwrap(),
    ]);
    layout(&mut tree);
    tree.pointer_moved(point(50.0, 50.0));
    assert_eq!(calls.get(), 0);
}

#[test]
fn sync_handler_errors_and_panics_use_the_runtime_error_sink() {
    let runtime = TaskRuntime::default();
    let errors = Rc::new(RefCell::new(Vec::new()));
    let out = errors.clone();
    runtime.set_error_handler(move |error| out.borrow_mut().push(error));
    let mut tree = WidgetTree::with_task_runtime(runtime.clone());
    tree.build_root(div().on_click(|| -> Result<(), &'static str> { Err("sync failure") }));
    tree.click(tree.root().unwrap());
    tree.reconcile_root(div().on_click(|| -> () {
        panic!("sync panic");
    }));
    tree.click(tree.root().unwrap());
    assert_eq!(errors.borrow().len(), 2);
    assert_eq!(runtime.stats().spawned, 0);
}

#[test]
fn function_components_and_elements_accept_the_same_handlers_without_layout_wrappers() {
    #[component]
    fn leaf() -> impl IntoElement {
        div().id("leaf").width(100.0).height(100.0)
    }
    let calls = Rc::new(Cell::new(0));
    let out = calls.clone();
    let view = leaf()
        .on_click(move || out.set(out.get() + 1))
        .into_element()
        .on_mouse_enter(|| {});
    let mut tree = build(view);
    let root = tree.root().unwrap();
    tree.click(root);
    assert_eq!(calls.get(), 1);
    assert_eq!(id(&tree, "leaf"), root);
    assert_eq!(tree.children(root).len(), 0);
}

#[test]
fn refreshing_layout_does_not_reemit_mouse_move_or_drag_samples() {
    let phases = Rc::new(Cell::new(0));
    let drag = phases.clone();
    let moves = Rc::new(Cell::new(0));
    let movement = moves.clone();
    let mut tree = build(canvas(
        div()
            .id("source")
            .width(100.0)
            .height(100.0)
            .on_drag(move || drag.set(drag.get() + 1))
            .on_mouse_move(move || movement.set(movement.get() + 1)),
    ));
    tree.pointer_moved(point(50.0, 50.0));
    tree.pointer_pressed(true);
    for _ in 0..3 {
        layout(&mut tree);
        tree.update_styles(Instant::now());
        tree.refresh_pointer();
    }
    assert_eq!(phases.get(), 1);
    assert_eq!(moves.get(), 1);
}

#[test]
fn every_mouse_button_and_scroll_unit_reaches_the_handler() {
    let buttons = Rc::new(RefCell::new(Vec::new()));
    let out = buttons.clone();
    let mut tree = build(canvas(div().width(100.0).height(100.0).on_mouse_down(
        move |e: MouseEvent| {
            out.borrow_mut().push(e.button.unwrap());
        },
    )));
    tree.pointer_moved(point(50.0, 50.0));
    let all = [
        B::Left,
        B::Right,
        B::Middle,
        B::Back,
        B::Forward,
        B::Other(42),
    ];
    for button in all {
        tree.dispatch_mouse_button(button, true, Mods::empty());
        tree.dispatch_mouse_button(button, false, Mods::empty());
    }
    assert_eq!(*buttons.borrow(), all);
}

#[test]
fn async_drag_capture_is_established_before_the_callback_is_polled() {
    let phases = Rc::new(RefCell::new(Vec::new()));
    let out = phases.clone();
    let mut tree = build(canvas(
        div()
            .id("source")
            .width(100.0)
            .height(100.0)
            .on_drag(async move |e: DragEvent| {
                time::yield_now().await;
                out.borrow_mut().push(e.phase);
            }),
    ));
    tree.pointer_moved(point(50.0, 50.0));
    tree.pointer_pressed(true);
    assert_eq!(tree.pointer_capture(), Some(id(&tree, "source")));
    assert!(phases.borrow().is_empty());
    tree.pointer_moved(point(150.0, 50.0));
    tree.pointer_pressed(false);
    assert_eq!(tree.pointer_capture(), None);
    pump(tree.task_runtime());
    assert_eq!(
        *phases.borrow(),
        [DragPhase::Start, DragPhase::Move, DragPhase::End]
    );
}

#[test]
fn adding_or_reordering_other_handlers_preserves_in_flight_async_work() {
    let values = Rc::new(Cell::new(0));
    let make = |extra| {
        let out = values.clone();
        let mut node = div().id("node").width(100.0).height(100.0);
        if extra {
            node = node.on_mouse_move(|| {});
        }
        node.on_click(async move || {
            time::yield_now().await;
            out.set(out.get() + 1);
        })
    };
    let mut tree = build(make(false));
    tree.click(tree.root().unwrap());
    tree.task_runtime().tick();
    tree.reconcile_root(make(true));
    pump(tree.task_runtime());
    assert_eq!(values.get(), 1);
}

#[test]
fn component_handler_origins_preserve_the_surviving_outer_call() {
    #[component]
    fn wrapped(inner: bool, values: Rc<RefCell<Vec<&'static str>>>) -> impl IntoElement {
        let mut node = div();
        if inner {
            node = node.on_click(async move || {
                time::yield_now().await;
                values.borrow_mut().push("inner");
            });
        }
        node
    }
    let values = Rc::new(RefCell::new(Vec::new()));
    let make = |inner| {
        let out = values.clone();
        wrapped(inner, values.clone()).on_click(async move || {
            time::yield_now().await;
            out.borrow_mut().push("outer");
        })
    };
    let mut tree = build(make(true));
    tree.click(tree.root().unwrap());
    tree.task_runtime().tick();
    tree.reconcile_root(make(false));
    pump(tree.task_runtime());
    assert_eq!(*values.borrow(), ["outer"]);
}

#[test]
fn a_new_modal_cancels_background_drag_capture() {
    let phases = Rc::new(RefCell::new(Vec::new()));
    let out = phases.clone();
    let mut tree =
        build(
            div()
                .width(400.0)
                .height(200.0)
                .child(div().id("source").width(100.0).height(100.0).on_drag(
                    move |e: DragEvent| {
                        out.borrow_mut().push(e.phase);
                    },
                ))
                .child(div().tag("dialog").id("modal").width(100.0).height(100.0)),
        );
    tree.pointer_moved(point(50.0, 50.0));
    tree.pointer_pressed(true);
    tree.show_modal(id(&tree, "modal")).unwrap();
    assert_eq!(tree.pointer_capture(), None);
    assert_eq!(*phases.borrow(), [DragPhase::Start, DragPhase::Cancel]);
}

#[cfg(feature = "editing")]
#[test]
fn ancestor_drag_handler_does_not_steal_an_editable_inputs_pointer() {
    let mut tree = build(
        div().width(400.0).height(200.0).on_drag(|| {}).child(
            voidui::input(voidui::Editor::new(""))
                .id("input")
                .width(100.0)
                .height(50.0)
                .on_key_up(|| {}),
        ),
    );
    tree.pointer_moved(point(30.0, 25.0));
    tree.pointer_pressed(true);
    assert_eq!(tree.pointer_capture(), None);
    assert_eq!(tree.focused(), Some(id(&tree, "input")));
}

#[test]
fn capture_routes_additional_buttons_to_the_owner_without_releasing_it() {
    let received = Rc::new(RefCell::new(Vec::new()));
    let listener = || {
        let out = received.clone();
        move |e: MouseEvent| {
            out.borrow_mut().push((e.target, e.button, e.state));
        }
    };
    let mut tree = build(canvas(
        div()
            .id("source")
            .width(100.0)
            .height(100.0)
            .on_drag(|| {})
            .on_mouse_down(listener())
            .on_mouse_up(listener()),
    ));
    let owner = id(&tree, "source");
    tree.pointer_moved(point(50.0, 50.0));
    tree.pointer_pressed(true);
    tree.pointer_moved(point(150.0, 50.0));
    tree.dispatch_mouse_button(B::Right, true, Mods::empty());
    tree.dispatch_mouse_button(B::Right, false, Mods::empty());
    assert_eq!(tree.pointer_capture(), Some(owner));
    assert_eq!(received.borrow()[1], (owner, Some(B::Right), M::Pressed));
    assert_eq!(received.borrow()[2], (owner, Some(B::Right), M::Released));
    tree.pointer_pressed(false);
}

#[test]
fn stopping_bubbling_keeps_other_handlers_on_the_same_physical_node() {
    #[component]
    fn inner() -> impl IntoElement {
        div().id("inner").on_click(|| R::STOP_PROPAGATION)
    }
    let same = Rc::new(Cell::new(0));
    let same_out = same.clone();
    let ancestor = Rc::new(Cell::new(0));
    let ancestor_out = ancestor.clone();
    let mut tree = build(
        div()
            .on_click(move || ancestor_out.set(ancestor_out.get() + 1))
            .child(inner().on_click(move || same_out.set(same_out.get() + 1))),
    );
    tree.click(id(&tree, "inner"));
    assert_eq!(same.get(), 1);
    assert_eq!(ancestor.get(), 0);
}

#[test]
fn custom_widget_reuses_the_public_handler_for_async_pointer_delivery() {
    use voidui::core::{
        context::LayoutContext,
        event::Event,
        layout::{LayoutInput, LayoutOutput},
        widget::{Widget, WidgetBuilder},
    };
    struct Custom {
        movement: EventHandler<MouseEvent>,
    }
    impl Widget for Custom {
        fn accepts_events(&self) -> bool {
            true
        }
        fn on_event_with_tasks(&mut self, event: &Event, runtime: &TaskRuntime) -> R {
            if let Event::Mouse(event) = event
                && event.state == M::Moved
            {
                self.movement.dispatch(*event, runtime)
            } else {
                R::CONTINUE
            }
        }
        fn layout(&mut self, inputs: LayoutInput, cx: LayoutContext<'_, '_>) -> LayoutOutput {
            cx.layout_children(inputs)
        }
    }
    let value = Rc::new(Cell::new(None));
    let out = value.clone();
    let custom = Custom {
        movement: EventHandler::new(async move |event: MouseEvent| {
            time::yield_now().await;
            out.set(Some(event.position));
        }),
    };
    let mut tree = build(
        WidgetBuilder::from_widget(custom)
            .width(100.0)
            .height(100.0),
    );
    tree.pointer_moved(point(30.0, 20.0));
    assert_eq!(value.get(), None);
    pump(tree.task_runtime());
    assert_eq!(value.get(), Some(Point::new(30.0, 20.0)));
}

#[test]
fn wheel_consumption_preserves_the_other_axis_and_combines_across_handlers() {
    use voidui::core::event::{EventResponse, MouseWheel, ScrollAxes, WheelUnit};
    let wheel = MouseWheel {
        x: -0.25,
        y: -60.0,
        unit: WheelUnit::Pixels,
    };
    let horizontal = EventResponse::consume_scroll(ScrollAxes::HORIZONTAL);
    assert!(!horizontal.prevent_default);
    assert_eq!(
        horizontal.remaining_scroll(wheel),
        MouseWheel { x: 0.0, ..wheel }
    );
    let both = horizontal | EventResponse::consume_scroll(ScrollAxes::VERTICAL);
    assert_eq!(
        both.remaining_scroll(wheel),
        MouseWheel {
            x: 0.0,
            y: 0.0,
            ..wheel
        }
    );
}
