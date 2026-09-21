//! Widget gestures preserve source selection and remain owned until completion.
use super::*;
use editing::{BlockView, EditorViews, Projection, Replacement, ViewId, WidgetView};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use style::selection::Cursor;
use winit::window::CursorIcon;

#[test]
fn hosted_scrollbar_cursor_is_resolved_before_the_first_click_or_paint() {
    for transform in ["none", "translate(40px,20px) scale(1.5)"] {
        let editor = Editor::new(format!("before\n[view]\n{}", "after\n".repeat(20)));
        editor
            .update(|s| {
                s.set_projection(
                    s.revision(),
                    Projection::new().block_view(BlockView::new(ViewId(1), 7..13)),
                )
            })
            .unwrap();
        let moves = Rc::new(Cell::new(0));
        let seen = moves.clone();
        let views = EditorViews::new().register(ViewId(1), move || {
            let seen = seen.clone();
            Box::new(WidgetView::new(move || {
                let seen = seen.clone();
                div()
                    .width(120.0)
                    .height(60.0)
                    .cursor_text()
                    .overflow_x(Overflow::Auto)
                    .scrollbar_mode(ScrollbarMode::Classic)
                    .scrollbar_width(ScrollbarWidth::Thin)
                    .on_mouse_move(move || seen.set(seen.get() + 1))
                    .child(div().width(360.0).height(40.0).cursor_pointer())
                    .into_element()
            }))
        });
        let (mut tree, cache) = build(
            rich_editor(&editor).views(views).id("edit"),
            &format!(
                "textarea{{width:300px;height:200px;margin:20px;padding:10px;transform:{transform};transform-origin:0 0}}"
            ),
        );
        let edit = id(&tree, "edit");
        let generation = editor.with(|s| s.generation());
        for scroll in [0.0, 20.0] {
            tree.scroll_to(edit, Point::new(0.0, scroll));
            tree.refresh_scroll_content(&cache);
            let bounds = tree.input_bounds_for_range(edit, 7..7, &cache).unwrap();
            let scale = bounds.size.height / 60.0;
            for (point, cursor) in [
                (
                    Point::new(
                        bounds.origin.x + 10.0 * scale,
                        bounds.origin.y + 58.0 * scale,
                    ),
                    CursorIcon::Default,
                ),
                (
                    Point::new(
                        bounds.origin.x + 10.0 * scale,
                        bounds.origin.y + 20.0 * scale,
                    ),
                    CursorIcon::Pointer,
                ),
            ] {
                tree.dispatch_mouse_move(Some(point), ModifiersState::empty());
                assert_eq!(tree.pointer_cursor(), Cursor::Icon(cursor));
                assert_eq!(editor.with(|s| s.generation()), generation);
                assert_eq!(tree.focused(), None);
                assert_eq!(moves.get(), 0, "cursor queries must not manufacture input");
            }
            let text = tree.input_bounds_for_range(edit, 14..14, &cache).unwrap();
            let text_point = Point::new(
                text.origin.x + scale,
                text.origin.y + text.size.height / 2.0,
            );
            assert_eq!(
                tree.pointer_cursor_at(Some(text_point), None),
                Cursor::Icon(CursorIcon::Text)
            );
        }
    }
}

#[test]
fn cursor_queries_recurse_through_nested_editors_and_preserve_drag_capture() {
    let inner = Editor::new("[view]");
    inner
        .update(|s| {
            s.set_projection(
                s.revision(),
                Projection::new().replace(Replacement::object(ViewId(1), 0..6)),
            )
        })
        .unwrap();
    let inner_views = EditorViews::new().register(ViewId(1), || {
        Box::new(WidgetView::new(|| {
            div()
                .width(120.0)
                .height(60.0)
                .cursor_grab()
                .on_drag(|| {})
                .into_element()
        }))
    });
    let outer = Editor::new("before\n[view]\nafter");
    outer
        .update(|s| {
            s.set_projection(
                s.revision(),
                Projection::new().block_view(BlockView::new(ViewId(1), 7..13)),
            )
        })
        .unwrap();
    let views = EditorViews::new().register(ViewId(1), move || {
        let inner = inner.clone();
        let views = inner_views.clone();
        Box::new(WidgetView::new(move || {
            rich_editor(&inner)
                .views(views.clone())
                .width(200.0)
                .height(110.0)
                .into_element()
        }))
    });
    let (mut tree, cache) = build(
        rich_editor(&outer).views(views).id("edit"),
        "textarea{width:300px;height:200px;margin:20px;padding:10px}",
    );
    let edit = id(&tree, "edit");
    let bounds = tree.input_bounds_for_range(edit, 7..7, &cache).unwrap();
    let point = Point::new(bounds.origin.x + 10.0, bounds.origin.y + 20.0);
    assert_eq!(
        tree.pointer_cursor_at(Some(point), None),
        Cursor::Icon(CursorIcon::Grab)
    );
    tree.dispatch_input(
        edit,
        &pointer(PointerPhase::Down, point, ModifiersState::empty(), 1),
        &cache,
    );
    let outside = Point::new(450.0, 300.0);
    tree.dispatch_input(
        edit,
        &pointer(PointerPhase::Move, outside, ModifiersState::empty(), 1),
        &cache,
    );
    for position in [Some(outside), None] {
        assert_eq!(
            tree.pointer_cursor_at(position, Some(edit)),
            Cursor::Icon(CursorIcon::Grab)
        );
    }
    tree.dispatch_input(
        edit,
        &pointer(PointerPhase::Cancel, outside, ModifiersState::empty(), 1),
        &cache,
    );
    assert_eq!(tree.pointer_cursor_at(Some(outside), None), Cursor::Auto);
    assert_eq!(
        outer.with(|s| s.selections().primary()),
        Selection::caret(0)
    );
    // Removing a captured preview must not leave its cursor attached to the host.
    tree.dispatch_input(
        edit,
        &pointer(PointerPhase::Down, point, ModifiersState::empty(), 1),
        &cache,
    );
    outer
        .update(|s| s.set_projection(s.revision(), Projection::new()))
        .unwrap();
    tree.refresh_scroll_content(&cache);
    // The native host still owns the press until release; use its CSS cursor.
    assert_eq!(
        tree.pointer_cursor_at(None, Some(edit)),
        Cursor::Icon(CursorIcon::Text)
    );
    assert_eq!(tree.pointer_cursor_at(None, None), Cursor::Auto);
    assert_eq!(
        tree.pointer_cursor_at(Some(point), Some(edit)),
        Cursor::Icon(CursorIcon::Text)
    );
}

#[test]
fn event_policy_and_consumption_are_checked_before_source_selection() {
    struct Probe {
        ignore: bool,
        consume: bool,
        seen: Rc<RefCell<Vec<Selection>>>,
    }
    impl editing::EmbeddedView for Probe {
        fn measure(&mut self, _: f32, _: &TextLayoutCache) -> render::Result<editing::ViewMetrics> {
            Ok(editing::ViewMetrics::new(100.0, 40.0))
        }
        fn paint(
            &mut self,
            _: &mut Painter<'_>,
            _: core::geometry::Rect<f32>,
        ) -> render::Result<()> {
            Ok(())
        }
        fn ignore_event(&self, _: &InputEvent) -> bool {
            self.ignore
        }
        fn input(
            &mut self,
            _: &InputEvent,
            _: core::input::InputContext<'_>,
            editor: &Editor,
        ) -> bool {
            self.seen
                .borrow_mut()
                .push(editor.with(|s| s.selections().primary()));
            self.consume
        }
    }
    for (ignore, consume) in [(true, false), (true, true), (false, true), (false, false)] {
        let editor = Editor::new("a [view] z");
        let seen = Rc::new(RefCell::new(Vec::new()));
        let views = EditorViews::new().register(ViewId(1), {
            let seen = seen.clone();
            move || {
                Box::new(Probe {
                    ignore,
                    consume,
                    seen: seen.clone(),
                })
            }
        });
        editor
            .update(|s| {
                s.set_projection(
                    s.revision(),
                    Projection::new().replace(Replacement::object(ViewId(1), 2..8)),
                )
            })
            .unwrap();
        let (mut tree, cache) = build(
            rich_editor(&editor).views(views).id("edit"),
            "textarea{width:300px;height:150px}",
        );
        let edit = id(&tree, "edit");
        let bounds = tree.input_bounds_for_range(edit, 2..2, &cache).unwrap();
        let position = Point::new(bounds.origin.x + 10.0, bounds.origin.y + 15.0);
        let generation = editor.with(|s| s.generation());
        for phase in [PointerPhase::Down, PointerPhase::Move, PointerPhase::Up] {
            tree.dispatch_input(
                edit,
                &pointer(phase, position, ModifiersState::empty(), 1),
                &cache,
            );
        }
        assert!(
            seen.borrow().iter().all(|s| *s == Selection::caret(0)),
            "a widget observed premature source selection"
        );
        assert_eq!(
            editor.with(|s| s.selections().primary()),
            if ignore || consume {
                Selection::caret(0)
            } else {
                Selection::range(2, 8)
            }
        );
        if ignore || consume {
            assert_eq!(seen.borrow().len(), 3);
            assert_eq!(editor.with(|s| s.generation()), generation);
        }
    }
}

#[test]
fn scrollbar_drag_preserves_selection_and_capture_across_other_widgets() {
    // Exercise both dragging the thumb and paging by pressing its track.
    for (start_x, finish) in [
        (10.0, PointerPhase::Up),
        (10.0, PointerPhase::Cancel),
        (110.0, PointerPhase::Up),
        (110.0, PointerPhase::Cancel),
    ] {
        let editor = Editor::new("before\n[view]\n[other]\nafter");
        let original = SelectionSet::new(
            vec![
                Selection::range(0, 2),
                Selection::caret(editor.text().len()),
            ],
            1,
        )
        .unwrap();
        editor
            .update(|s| {
                s.select(original.clone())?;
                s.set_projection(
                    s.revision(),
                    Projection::new()
                        .block_view(BlockView::new(ViewId(1), 7..13))
                        .block_view(BlockView::new(ViewId(2), 14..21)),
                )
            })
            .unwrap();
        let clicks = Rc::new(Cell::new(0));
        let positions = Rc::new(RefCell::new(Vec::new()));
        let other_presses = Rc::new(Cell::new(0));
        let views = EditorViews::new()
            .register(ViewId(1), {
                let clicks = clicks.clone();
                let positions = positions.clone();
                move || {
                    let clicks = clicks.clone();
                    let positions = positions.clone();
                    Box::new(WidgetView::new(move || {
                        let clicks = clicks.clone();
                        let positions = positions.clone();
                        div()
                            .width(120.0)
                            .height(60.0)
                            .overflow_x(Overflow::Auto)
                            .scrollbar_mode(ScrollbarMode::Classic)
                            .scrollbar_width(ScrollbarWidth::Thin)
                            .on_click(move || clicks.set(clicks.get() + 1))
                            .child(
                                div()
                                    .width(360.0)
                                    .height(40.0)
                                    .flex_shrink(0.0)
                                    .on_mouse_scroll(move |event: MouseEvent| {
                                        positions.borrow_mut().push(event.local_position.x)
                                    }),
                            )
                            .into_element()
                    }))
                }
            })
            .register(ViewId(2), {
                let presses = other_presses.clone();
                move || {
                    let presses = presses.clone();
                    Box::new(WidgetView::new(move || {
                        let presses = presses.clone();
                        div()
                            .width(120.0)
                            .height(60.0)
                            .cursor_pointer()
                            .on_mouse_down(move || presses.set(presses.get() + 1))
                            .into_element()
                    }))
                }
            });
        let (mut tree, cache) = build(
            rich_editor(&editor).views(views).id("edit"),
            "textarea{width:300px;height:250px;margin:20px;padding:10px}",
        );
        let edit = id(&tree, "edit");
        tree.set_focused(Some(edit));
        // No paint before this press: input must place the hosted tree itself.
        let bounds = tree.input_bounds_for_range(edit, 7..7, &cache).unwrap();
        let other = tree.input_bounds_for_range(edit, 14..14, &cache).unwrap();
        let start = Point::new(bounds.origin.x + start_x, bounds.origin.y + 58.0);
        let end = Point::new(bounds.origin.x + 70.0, other.origin.y + 20.0);
        for (phase, position) in [
            (PointerPhase::Down, start),
            (PointerPhase::Move, end),
            (finish, end),
        ] {
            tree.dispatch_input(
                edit,
                &pointer(phase, position, ModifiersState::empty(), 1),
                &cache,
            );
            assert_eq!(editor.with(|s| s.selections().clone()), original);
            paint(&mut tree, &cache);
            let captured = matches!(phase, PointerPhase::Down | PointerPhase::Move);
            assert_eq!(
                tree.pointer_cursor_at(Some(position), captured.then_some(edit)),
                Cursor::Icon(if captured {
                    CursorIcon::Default
                } else {
                    CursorIcon::Pointer
                })
            );
            if captured {
                assert_eq!(
                    tree.pointer_cursor_at(None, Some(edit)),
                    Cursor::Icon(CursorIcon::Default)
                );
            }
        }
        assert_eq!(clicks.get(), 0);
        assert_eq!(other_presses.get(), 0);
        // Read actual content displacement through child-local coordinates.
        tree.dispatch_mouse_move(
            Some(Point::new(bounds.origin.x + 10.0, bounds.origin.y + 20.0)),
            ModifiersState::empty(),
        );
        tree.dispatch_mouse_scroll(
            MouseWheel {
                x: 0.0,
                y: 0.0,
                unit: WheelUnit::Pixels,
            },
            ModifiersState::empty(),
        );
        assert!(
            positions.borrow().last().is_some_and(|x| *x > 100.0),
            "content did not scroll, or capture did not end: {:?}",
            positions.borrow()
        );
        assert_eq!(editor.with(|s| s.selections().clone()), original);
        assert!(!editor.with(|s| s.can_undo()));
        assert_eq!(
            tree.text_input(edit).unwrap().selection_range(),
            original.primary().text_range()
        );
        // Passive scrolling must not intercept subsequent source editing.
        tree.dispatch_input(edit, &InputEvent::Text("!".into()), &cache);
        assert!(editor.text().ends_with('!'));
    }
}

#[test]
fn passive_widgets_opt_into_source_selection_and_text_drags_do_not_enter_widgets() {
    for ignored in [true, false] {
        let editor = Editor::new("a [view] z");
        let presses = Rc::new(Cell::new(0));
        let seen = presses.clone();
        let views = EditorViews::new().register(ViewId(1), move || {
            let seen = seen.clone();
            Box::new(
                WidgetView::new(move || {
                    let seen = seen.clone();
                    div()
                        .width(100.0)
                        .height(40.0)
                        .on_mouse_down(move || seen.set(seen.get() + 1))
                        .into_element()
                })
                .ignore_events(if ignored { |_| true } else { |_| false }),
            )
        });
        editor
            .update(|s| {
                s.set_projection(
                    s.revision(),
                    Projection::new().replace(Replacement::object(ViewId(1), 2..8)),
                )
            })
            .unwrap();
        let (mut tree, cache) = build(
            rich_editor(&editor).views(views).id("edit"),
            "textarea{width:300px;height:150px}",
        );
        let edit = id(&tree, "edit");
        let bounds = tree.input_bounds_for_range(edit, 2..2, &cache).unwrap();
        let inside = Point::new(bounds.origin.x + 10.0, bounds.origin.y + 15.0);
        for phase in [PointerPhase::Down, PointerPhase::Up] {
            tree.dispatch_input(
                edit,
                &pointer(phase, inside, ModifiersState::empty(), 1),
                &cache,
            );
        }
        assert_eq!(presses.get(), 1);
        assert_eq!(
            editor.with(|s| s.selections().primary()),
            if ignored {
                Selection::caret(0)
            } else {
                Selection::range(2, 8)
            }
        );
        let text = tree.input_bounds_for_range(edit, 0..0, &cache).unwrap();
        tree.style_mut(edit).cursor = Cursor::Icon(CursorIcon::Crosshair).into();
        tree.update_styles(Instant::now());
        let start = Point::new(text.origin.x, text.origin.y + text.size.height / 2.0);
        for (phase, point) in [
            (PointerPhase::Down, start),
            (PointerPhase::Move, inside),
            (PointerPhase::Up, inside),
        ] {
            tree.dispatch_input(
                edit,
                &pointer(phase, point, ModifiersState::empty(), 1),
                &cache,
            );
            if phase != PointerPhase::Up {
                assert_eq!(
                    tree.pointer_cursor_at(Some(inside), Some(edit)),
                    Cursor::Icon(CursorIcon::Crosshair)
                );
            }
        }
        assert_eq!(presses.get(), 1, "a text drag entered the widget");
        assert!(!editor.with(|s| s.selections().primary().is_caret()));
    }
}

#[test]
fn focus_loss_cancels_widget_click_without_changing_selection() {
    let editor = Editor::new("[view] tail");
    let clicks = Rc::new(Cell::new(0));
    let views = EditorViews::new().register(ViewId(1), {
        let clicks = clicks.clone();
        move || {
            let clicks = clicks.clone();
            Box::new(WidgetView::new(move || {
                let clicks = clicks.clone();
                div()
                    .width(100.0)
                    .height(40.0)
                    .on_click(move || clicks.set(clicks.get() + 1))
                    .into_element()
            }))
        }
    });
    editor
        .update(|s| {
            s.set_projection(
                s.revision(),
                Projection::new().replace(Replacement::object(ViewId(1), 0..6)),
            )
        })
        .unwrap();
    let (mut tree, cache) = build(
        rich_editor(&editor).views(views).id("edit"),
        "textarea{width:300px;height:150px}",
    );
    let edit = id(&tree, "edit");
    tree.set_focused(Some(edit));
    let bounds = tree.input_bounds_for_range(edit, 0..0, &cache).unwrap();
    let inside = Point::new(bounds.origin.x + 10.0, bounds.origin.y + 15.0);
    tree.dispatch_input(
        edit,
        &pointer(PointerPhase::Down, inside, ModifiersState::empty(), 1),
        &cache,
    );
    tree.set_focused(None);
    tree.set_focused(Some(edit));
    tree.dispatch_input(
        edit,
        &pointer(PointerPhase::Up, inside, ModifiersState::empty(), 1),
        &cache,
    );
    assert_eq!(clicks.get(), 0);
    for phase in [PointerPhase::Down, PointerPhase::Up] {
        tree.dispatch_input(
            edit,
            &pointer(phase, inside, ModifiersState::empty(), 1),
            &cache,
        );
    }
    assert_eq!(clicks.get(), 1);
    assert_eq!(
        editor.with(|s| s.selections().primary()),
        Selection::caret(0)
    );
}
