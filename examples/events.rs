//! Run interactively, or with --native-test and real pointer/key/scroll input.
//! Drag from the handle onto the crosshair panel, release, press a key, then scroll.
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use voidui::{
    core::{
        event::{Key, NamedKey},
        geometry::Size,
    },
    render::SharedString,
    style::selection::Cursor,
    tasks::time,
    *,
};
use winit::window::CursorIcon;

#[derive(Default)]
struct Probe {
    #[cfg(feature = "editing")]
    input: RefCell<Option<State<String>>>,
    phases: RefCell<Vec<DragEvent>>,
    last_reported: Cell<usize>,
    captured_cursor: Cell<bool>,
    released_cursor: Cell<bool>,
    key_down: Cell<usize>,
    key_up: Cell<usize>,
    scrolls: Cell<usize>,
    mouse_down: Cell<usize>,
    mouse_up: Cell<usize>,
    quit: Cell<bool>,
}

#[component]
fn scene(probe: Rc<Probe>) -> impl IntoElement {
    #[cfg(feature = "editing")]
    let input_value = state(String::new);
    #[cfg(feature = "editing")]
    {
        *probe.input.borrow_mut() = Some(input_value.clone());
    }
    let status = state(|| SharedString::from("Drag the handle onto the crosshair panel."));
    let drag_status = status.clone();
    let drag_probe = probe.clone();
    let key_status = status.clone();
    let key_probe = probe.clone();
    let key_up_probe = probe.clone();
    let key_up_status = status.clone();
    let scroll_status = status.clone();
    let scroll_probe = probe.clone();
    let down_probe = probe.clone();
    let up_probe = probe.clone();
    let enter_status = status.clone();
    let leave_status = status.clone();
    let click_status = status.clone();
    let root = div()
        .class("app")
        .on_key_down(move |event: KeyEvent| {
            key_probe.key_down.set(key_probe.key_down.get() + 1);
            key_status.set(format!("Key down: {:?}", event.key).into());
            if event.key == Key::Named(NamedKey::Escape) {
                key_probe.quit.set(true);
            }
        })
        .on_key_up(async move |_: KeyEvent| {
            time::yield_now().await;
            key_up_probe.key_up.set(key_up_probe.key_up.get() + 1);
            key_up_status.set("Async key-up completed".into());
        })
        .on_mouse_scroll(move |event: MouseEvent| {
            scroll_probe.scrolls.set(scroll_probe.scrolls.get() + 1);
            scroll_status.set(
                format!(
                    "Scroll: {:?} ({}, {})",
                    event.wheel.unit, event.wheel.x, event.wheel.y
                )
                .into(),
            );
        })
        .child(text("Unified events and drag capture").class("heading"))
        .child(text(status.get()).id("status"))
        .child(
            div()
                .id("handle")
                .class("handle")
                .tag("button")
                .on_mouse_enter(move || enter_status.set("Pointer entered the handle".into()))
                .on_mouse_leave(move || {
                    leave_status.set("Pointer left the handle; capture continues while held".into())
                })
                .on_mouse_down(move || down_probe.mouse_down.set(down_probe.mouse_down.get() + 1))
                .on_mouse_up(move || up_probe.mouse_up.set(up_probe.mouse_up.get() + 1))
                .on_mouse_move(|_: MouseEvent| {})
                .on_drag(move |event: DragEvent| {
                    drag_probe.phases.borrow_mut().push(event);
                    drag_status.set(
                        format!(
                            "{:?}: ({:.0}, {:.0}), delta ({:.0}, {:.0})",
                            event.phase,
                            event.position.x,
                            event.position.y,
                            event.total_delta.x,
                            event.total_delta.y
                        )
                        .into(),
                    );
                })
                .on_click(async move || {
                    time::yield_now().await;
                    click_status.set("Async click completed".into());
                })
                .child("Drag outside this handle"),
        )
        .child(
            div()
                .id("other")
                .class("other")
                .child("Crosshair cursor here\nGrab cursor stays during dragging"),
        );
    #[cfg(feature = "editing")]
    let root = root.child(
        voidui::input(input_value)
            .id("input")
            .class("input")
            .on_key_down(|event: KeyEvent| {
                if event.key == Key::Named(NamedKey::Tab) {
                    EventResponse::PREVENT_DEFAULT
                } else {
                    EventResponse::CONTINUE
                }
            })
            .placeholder("Type 'a' here; Tab is intercepted before focus traversal"),
    );

    root
}
fn main() -> anyhow::Result<()> {
    let smoke = std::env::args().any(|arg| arg == "--smoke");
    let mut stage = 0;
    let native_test = std::env::args().any(|arg| arg == "--native-test");
    let probe = Rc::new(Probe::default());
    let inspect = probe.clone();
    Application::new().window(WindowOptions {
        title: "Voidui Events".into(), app_id: "org.voidui.events-test".into(),
        size: Size::new(680.0, 430.0), ..Default::default()
    }, scene(probe))
    .css(".app {position:relative; width:100%; height:100%; background:#16202e; color:#dce4f0; padding:24px; font-size:16px} .heading {font-size:26px; margin-bottom:12px} #status {font-size:14px} .handle {position:absolute;left:40px;top:130px;width:180px;height:120px;background:#315c85;color:#fff;cursor:grab;border-width:0;padding:18px} .handle:active{cursor:grabbing;background:#4277a7} .other{position:absolute;left:300px;top:130px;width:290px;height:120px;cursor:crosshair;background:#234c42;padding:18px} .input{position:absolute;left:40px;top:300px;width:550px;height:46px;caret-animation:manual}")?
    .on_frame(move |window| {
        let phases = inspect.phases.borrow();
        if phases.len() != inspect.last_reported.get() {
            let owner = window.tree().find_by_id("handle").unwrap();
            let other = window.tree().find_by_id("other").unwrap();
            let bounds = window.tree().bounds(other);
            if let Some(event) = phases.last() {
                let over_other = event.position.x >= bounds.origin.x && event.position.x < bounds.origin.x + bounds.size.width
                    && event.position.y >= bounds.origin.y && event.position.y < bounds.origin.y + bounds.size.height;
                if event.phase == DragPhase::Move && over_other {
                    anyhow::ensure!(window.tree().pointer_capture() == Some(owner), "drag lost capture over the sibling");
                    anyhow::ensure!(window.current_cursor() == Cursor::Icon(CursorIcon::Grabbing), "captured cursor changed: {:?}", window.current_cursor());
                    inspect.captured_cursor.set(true);
                }
                if event.phase == DragPhase::End && over_other {
                    anyhow::ensure!(window.tree().pointer_capture().is_none(), "release retained capture");
                    anyhow::ensure!(window.current_cursor() == Cursor::Icon(CursorIcon::Crosshair), "release failed to restore hover cursor: applied={:?}, resolved={:?}", window.current_cursor(), window.tree().pointer_cursor());
                    inspect.released_cursor.set(true);
                }
                println!("EVENT {:?} position={:?} capture={:?} cursor={:?}", event.phase, event.position, window.tree().pointer_capture(), window.current_cursor());
            }
            inspect.last_reported.set(phases.len());
        }
        if inspect.quit.get() { window.close(); }
        drop(phases);
        if smoke {
            let owner = window.tree().find_by_id("handle").unwrap();
            let other = window.tree().find_by_id("other").unwrap();
            let center = |id| {
                let b = window.tree().bounds(id);
                let scale = window.native_window().scale_factor();
                winit::dpi::PhysicalPosition::new(f64::from(b.origin.x + b.size.width / 2.0) * scale, f64::from(b.origin.y + b.size.height / 2.0) * scale)
            };
            let start = center(owner); let outside = center(other);
            match stage {
                0 => { window.pointer_moved(Some(start)); window.mouse_button(MouseButton::Left, true); }
                1 => { anyhow::ensure!(window.current_cursor() == Cursor::Icon(CursorIcon::Grabbing)); window.pointer_moved(Some(outside)); }
                2 => {
                    anyhow::ensure!(inspect.captured_cursor.get());
                    window.pointer_moved(None);
                    anyhow::ensure!(window.tree().pointer_capture() == Some(owner));
                    anyhow::ensure!(window.current_cursor() == Cursor::Icon(CursorIcon::Grabbing));
                    let size = window.native_window().inner_size();
                    window.pointer_moved(Some(winit::dpi::PhysicalPosition::new(f64::from(size.width) + 100.0, -30.0)));
                }
                3 => {
                    anyhow::ensure!(window.tree().pointer_capture() == Some(owner));
                    window.pointer_moved(Some(outside)); window.mouse_button(MouseButton::Left, false);
                }
                4 => {
                    anyhow::ensure!(inspect.released_cursor.get());
                    window.tree_mut().dispatch_key(Key::Named(NamedKey::ArrowDown), KeyEventType::Pressed, Default::default(), false);
                    window.tree_mut().dispatch_key(Key::Named(NamedKey::ArrowDown), KeyEventType::Released, Default::default(), false);
                    window.mouse_scroll(winit::event::MouseScrollDelta::LineDelta(0.0, 1.0));
                }
                _ if inspect.key_down.get() > 0 && inspect.key_up.get() > 0 && inspect.scrolls.get() > 0 => {
                    println!("PASS programmatic native-window input, capture beyond element/window bounds, physical DPI conversion, cursor retention/restoration, key down/up and scroll");
                    window.close();
                }
                // Rendering may precede an async callback. Its state write wakes
                // the next frame; the test does not poll by requesting redraws.
                _ => {}
            }
            stage += 1;
        }
        #[cfg(feature = "editing")]
        let editing_verified = inspect.input.borrow().as_ref().is_some_and(|value| value.get() == "a");
        #[cfg(not(feature = "editing"))]
        let editing_verified = true;
        if native_test && editing_verified && inspect.captured_cursor.get() && inspect.released_cursor.get()
            && inspect.key_down.get() > 0 && inspect.key_up.get() > 0 && inspect.scrolls.get() > 0
        {
            println!("PASS native capture, cursor retention/restoration, mouse down/up, key down/up, scroll: down={} up={} keys={}/{} scroll={}", inspect.mouse_down.get(), inspect.mouse_up.get(), inspect.key_down.get(), inspect.key_up.get(), inspect.scrolls.get());
            window.close();
        }
        Ok(())
    }).run()
}
