//! Native-free tests for the shared chrome policy, hit geometry, actions and
//! window-scoped reactive state. OS integration is exercised by title_bar --smoke.
use std::{cell::Cell, rc::Rc, sync::Arc};
use voidui::core::{
    geometry::{Point, Rect},
    layout::{AvailableSpace, Size},
    widget_tree::WidgetTree,
};
use voidui::render::{ParleyTextSystem, TextLayoutCache, TextSystem};
use voidui::style::css::Stylesheet;
use voidui::{
    EventResponse, IntoElement, MouseButton, WindowAction as Action, WindowButton as Button,
    WindowButtonLayout, WindowControlArea as Area, WindowDecorations, WindowState, component, div,
    text, title_bar, window_context,
};
use winit::keyboard::ModifiersState;
fn layout(tree: &mut WidgetTree) {
    let fonts = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    fonts
        .add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!(
            "../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
        ))])
        .unwrap();
    let cache = TextLayoutCache::new(Arc::new(TextSystem::new(Arc::new(fonts))));
    tree.layout(
        Size {
            width: AvailableSpace::Definite(400.0),
            height: AvailableSpace::Definite(200.0),
        },
        &cache,
    );
}
fn build(view: impl IntoElement, css: &str) -> WidgetTree {
    let mut tree = WidgetTree::new();
    tree.set_window_state(WindowState {
        decorations: WindowDecorations::Custom,
        ..Default::default()
    });
    tree.set_stylesheets(vec![Stylesheet::parse(css).unwrap()]);
    tree.build_root(view);
    layout(&mut tree);
    tree
}
fn press(tree: &mut WidgetTree, x: f32, y: f32, down: bool) {
    tree.pointer_moved(Some(Point::new(x, y)));
    tree.dispatch_mouse_button(MouseButton::Left, down, ModifiersState::empty());
}
#[test]
fn layouts_allow_ordered_sides_and_intentionally_missing_buttons() {
    let layout: WindowButtonLayout = "close:minimize,maximize".parse().unwrap();
    assert_eq!(layout.left, [Button::Close]);
    assert_eq!(layout.right, [Button::Minimize, Button::Maximize]);
    let empty: WindowButtonLayout = ":".parse().unwrap();
    assert!(empty.left.is_empty() && empty.right.is_empty());
    let menu: WindowButtonLayout = " appmenu : close ".parse().unwrap();
    assert!(menu.left.is_empty());
    assert_eq!(menu.right, [Button::Close]);
    for invalid in [
        "close",
        "a:b:c",
        "close:close",
        ":unknown",
        ":maximize,maximize",
    ] {
        assert!(invalid.parse::<WindowButtonLayout>().is_err(), "{invalid}");
    }
}
#[test]
fn native_titlebar_is_the_default_and_configuration_is_per_window() {
    let a = voidui::WindowOptions::default();
    let mut b = a.clone();
    b.decorations = WindowDecorations::Custom;
    b.titlebar.button_layout = Some("close:".parse().unwrap());
    assert_eq!(a.decorations, WindowDecorations::System);
    assert!(a.titlebar.button_layout.is_none());
}
#[test]
fn child_labels_inherit_drag_but_interactive_children_stop_it() {
    let tree = build(
        div()
            .window_control_area(Area::Drag)
            .child(div().id("label").height(30.0).child("Title"))
            .child(
                div()
                    .id("action")
                    .height(30.0)
                    .on_click(|| {})
                    .child("Action"),
            )
            .child(
                div()
                    .id("client")
                    .height(30.0)
                    .window_control_area(Area::Client),
            ),
        "",
    );
    assert_eq!(tree.window_control_at(Point::new(5.0, 10.0)), Area::Drag);
    assert_eq!(tree.window_control_at(Point::new(5.0, 40.0)), Area::Client);
    assert_eq!(tree.window_control_at(Point::new(5.0, 70.0)), Area::Client);
    assert_eq!(tree.window_control_at(Point::new(-1.0, 10.0)), Area::Client);
}
#[test]
fn z_order_clipping_and_pointer_events_are_shared_with_normal_input() {
    let mut tree = build(
        div()
            .id("root")
            .child(div().id("drag").window_control_area(Area::Drag))
            .child(div().id("cover")),
        "#root{position:relative;width:400px;height:200px}#drag{position:absolute;inset:0}#cover{position:absolute;left:0;top:0;width:100px;height:40px;z-index:2}",
    );
    assert_eq!(tree.window_control_at(Point::new(5.0, 5.0)), Area::Client);
    assert_eq!(tree.window_control_at(Point::new(150.0, 5.0)), Area::Drag);
    let cover = tree.find_by_id("cover").unwrap();
    tree.set_attribute(cover, "class", Some("pass"));
    tree.set_stylesheets(vec![Stylesheet::parse("#root{position:relative;width:400px;height:200px}#drag{position:absolute;inset:0}#cover{position:absolute;inset:0;z-index:2}.pass{pointer-events:none}").unwrap()]);
    layout(&mut tree);
    assert_eq!(tree.window_control_at(Point::new(5.0, 5.0)), Area::Drag);
    let tree = build(
        div()
            .id("clip")
            .child(div().id("drag").window_control_area(Area::Drag)),
        "#clip{width:40px;height:40px;overflow:hidden}#drag{width:100px;height:100px}",
    );
    assert_eq!(tree.window_control_at(Point::new(20.0, 20.0)), Area::Drag);
    assert_eq!(tree.window_control_at(Point::new(60.0, 20.0)), Area::Client);
}
#[test]
fn modal_blocks_underlying_drag_even_with_pointer_transparent_backdrop() {
    let mut tree = build(
        div()
            .window_control_area(Area::Drag)
            .child(div().tag("dialog").id("modal").child("Modal")),
        "dialog{width:80px;height:40px}dialog::backdrop{pointer-events:none}",
    );
    tree.show_modal(tree.find_by_id("modal").unwrap()).unwrap();
    layout(&mut tree);
    assert_eq!(tree.window_control_at(Point::new(5.0, 5.0)), Area::Client);
}
#[test]
fn control_actions_use_release_target_and_programmatic_activation() {
    let mut tree = build(
        div().width(400.0).height(200.0).child(
            div()
                .id("close")
                .width(80.0)
                .height(40.0)
                .window_control_area(Area::Close)
                .child(div().id("label").height(40.0)),
        ),
        "",
    );
    press(&mut tree, 5.0, 5.0, true);
    assert!(tree.take_window_actions().is_empty());
    press(&mut tree, 150.0, 70.0, false);
    assert!(tree.take_window_actions().is_empty());
    press(&mut tree, 5.0, 5.0, true);
    press(&mut tree, 5.0, 5.0, false);
    assert_eq!(tree.take_window_actions(), [Action::Close]);
    assert!(tree.click(tree.find_by_id("label").unwrap()));
    assert_eq!(tree.take_window_actions(), [Action::Close]);
}
#[test]
fn cancelled_disabled_and_prevented_controls_never_activate() {
    let mut tree = build(
        div()
            .id("close")
            .width(80.0)
            .height(40.0)
            .window_control_area(Area::Close),
        "",
    );
    press(&mut tree, 5.0, 5.0, true);
    tree.cancel_pointer_capture();
    press(&mut tree, 5.0, 5.0, false);
    assert!(tree.take_window_actions().is_empty());
    let close = tree.find_by_id("close").unwrap();
    tree.set_attribute(close, "disabled", Some(""));
    assert!(!tree.click(close));
    assert_eq!(tree.window_control_at(Point::new(5.0, 5.0)), Area::Client);
    for cancel in [false, true] {
        let button = div()
            .id("close")
            .width(80.0)
            .height(40.0)
            .window_control_area(Area::Close);
        let button = if cancel {
            button.on_mouse_down(|| EventResponse::PREVENT_DEFAULT)
        } else {
            button.on_click(|| EventResponse::PREVENT_DEFAULT)
        };
        let mut tree = build(button, "");
        press(&mut tree, 5.0, 5.0, true);
        press(&mut tree, 5.0, 5.0, false);
        assert!(tree.take_window_actions().is_empty());
    }
}
#[test]
fn window_state_is_reactive_scoped_and_noop_updates_stay_idle() {
    let renders = Rc::new(Cell::new(0));
    let count = renders.clone();
    let mut tree = build(
        component(move || {
            count.set(count.get() + 1);
            let state = window_context().state().unwrap();
            text(if state.maximized {
                "Restore"
            } else {
                "Maximize"
            })
        }),
        "",
    );
    let context = tree.window_context();
    let state = context.state().unwrap();
    tree.set_window_state(state.clone());
    assert_eq!(tree.flush_updates(), 0);
    let original = renders.get();
    tree.set_window_state(WindowState {
        maximized: true,
        ..state
    });
    assert!(tree.flush_updates() > 0);
    assert_eq!(renders.get(), original + 1);
    let other = WidgetTree::new();
    assert!(!other.window_context().state().unwrap().maximized);
    drop(tree);
    assert!(context.state().is_none());
    assert!(!context.request(Action::Close));
}
#[test]
fn window_context_action_wakes_a_headless_host() {
    let mut tree = WidgetTree::new();
    let calls = Rc::new(Cell::new(0));
    let out = calls.clone();
    tree.set_update_waker(move || out.set(out.get() + 1));
    assert!(tree.window_context().request(Action::Minimize));
    assert_eq!(calls.get(), 1);
    assert_eq!(tree.take_window_actions(), [Action::Minimize]);
}
#[test]
fn native_button_reservation_tracks_measured_geometry_without_remounting_content() {
    let mut tree = WidgetTree::new();
    let mut state = WindowState {
        decorations: WindowDecorations::Custom,
        native_controls: Some(Rect::from_xywh(12.0, 11.0, 58.0, 14.0)),
        ..Default::default()
    };
    tree.set_window_state(state.clone());
    tree.build_root(title_bar(div().id("content").height(20.0)));
    layout(&mut tree);
    let id = tree.find_by_id("content").unwrap();
    assert!(tree.bounds(id).origin.x >= 82.0);
    state.native_controls = Some(Rect::from_xywh(20.0, 11.0, 68.0, 14.0));
    tree.set_window_state(state);
    layout(&mut tree);
    assert_eq!(id, tree.find_by_id("content").unwrap());
    assert!(tree.bounds(id).origin.x >= 108.0);
}
#[test]
fn element_builder_retains_control_role_through_conversion() {
    let tree = build(
        div()
            .width(40.0)
            .height(40.0)
            .into_element()
            .window_control_area(Area::Max),
        "",
    );
    assert_eq!(tree.window_control_at(Point::new(4.0, 4.0)), Area::Max);
}

#[test]
fn popover_does_not_inherit_drag_from_its_dom_titlebar() {
    let mut tree = build(
        div()
            .window_control_area(Area::Drag)
            .child(div().id("popup").width(80.0).height(40.0)),
        "",
    );
    let id = tree.find_by_id("popup").unwrap();
    tree.show_popover(id).unwrap();
    layout(&mut tree);
    let bounds = tree.bounds(id);
    assert_eq!(
        tree.window_control_at(Point::new(bounds.origin.x + 1.0, bounds.origin.y + 1.0)),
        Area::Client
    );
}
