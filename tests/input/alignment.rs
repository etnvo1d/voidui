//! Single-line alignment shares geometry across paint, pointer input and IME.
use super::*;

fn near(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.02, "{actual} != {expected}");
}

#[test]
fn compact_rename_field_centers_a_normal_line_height() {
    let editor = Editor::new("Note");
    let (mut tree, cache) = build(
        input(&editor).id("edit"),
        "#edit{box-sizing:border-box;width:150px;height:24px;padding:0 6px;\
         border-width:1px;font-size:13px;line-height:normal}",
    );
    let edit = id(&tree, "edit");
    let content = tree.content_bounds(edit);
    let caret = tree.input_bounds_for_range(edit, 0..0, &cache).unwrap();
    near(content.size.height, 22.0);
    near(tree.text_style(edit).line_height.resolve(13.0), 15.6);
    // Font ascent/descent may extend past a tight CSS line box. Center the
    // resulting caret extent too, rather than assuming it equals line-height.
    near(
        caret.origin.y - content.origin.y,
        (22.0 - caret.size.height) * 0.5,
    );
}

#[test]
fn flex_baseline_alignment_follows_the_centered_input_text() {
    let editor = Editor::new("A");
    let (mut tree, cache) = build(
        div()
            .child(input(&editor).id("small"))
            .child(input(&editor).id("large")),
        "div{display:flex;align-items:baseline}input{width:100px;padding:3px 0 7px;\
         border-width:1px;font-size:13px;line-height:20px}\
         #small{height:40px}#large{height:100px}",
    );
    let scene = paint(&mut tree, &cache);
    assert_eq!(scene.monochrome_sprites.len(), 2);
    near(
        scene.monochrome_sprites[0].bounds.origin.y.0,
        scene.monochrome_sprites[1].bounds.origin.y.0,
    );
}

#[test]
fn single_lines_center_in_the_content_box_while_textareas_stay_at_the_top() {
    for multiline in [false, true] {
        for source in ["", "Ag", "aЖb"] {
            let editor = Editor::new(source);
            let field = if multiline {
                textarea(&editor)
            } else {
                input(&editor)
            };
            let (mut tree, cache) = build(
                field.id("edit"),
                "#edit{width:240px;height:90px;margin:11px;padding:3px 8px 7px;\
                 border:1px solid black;font-size:13px;line-height:20px;caret-animation:manual}",
            );
            let edit = id(&tree, "edit");
            tree.set_focused(Some(edit));
            let content = tree.content_bounds(edit);
            let expected_y = content.origin.y
                + if multiline {
                    0.0
                } else {
                    (content.size.height - 20.0) * 0.5
                };
            let anchor = tree.input_bounds_for_range(edit, 0..0, &cache).unwrap();
            near(anchor.origin.y, expected_y);
            near(anchor.size.height, 20.0);
            let scene = paint(&mut tree, &cache);
            let caret = scene.quads.iter().find(|q| q.spatial_pad == 1).unwrap();
            near(caret.bounds.origin.y.0, anchor.origin.y);
            near(caret.bounds.size.height.0, anchor.size.height);
            tree.refresh_scroll_content(&cache);
            near(tree.scroll_metrics(edit).unwrap().offset.y, 0.0);
        }
    }
}

#[test]
fn resizing_moves_glyphs_selection_and_range_anchors_together_without_reshaping() {
    let editor = Editor::new("Rename me");
    editor
        .update(|s| s.select(SelectionSet::single(Selection::range(0, 6))))
        .unwrap();
    let (mut tree, cache) = build(
        input(&editor).id("edit"),
        "#edit{width:220px;height:40px;padding:0;border-width:1px;font-size:13px;line-height:20px}\
         #edit.tall{height:100px}#edit::selection{background:red;color:black}",
    );
    let edit = id(&tree, "edit");
    tree.set_focused(Some(edit));
    let before = paint(&mut tree, &cache);
    let range = tree.input_bounds_for_range(edit, 0..6, &cache).unwrap();
    let shaped = cache.system().stats().paragraphs_shaped;
    tree.set_classes(edit, "tall");
    let after = paint(&mut tree, &cache);
    let moved_range = tree.input_bounds_for_range(edit, 0..6, &cache).unwrap();
    near(moved_range.origin.y - range.origin.y, 30.0);
    assert_eq!(cache.system().stats().paragraphs_shaped, shaped);
    assert!(!before.monochrome_sprites.is_empty());
    assert_eq!(
        before.monochrome_sprites.len(),
        after.monochrome_sprites.len()
    );
    for (a, b) in before
        .monochrome_sprites
        .iter()
        .zip(&after.monochrome_sprites)
    {
        near(b.bounds.origin.y.0 - a.bounds.origin.y.0, 30.0);
        near(b.bounds.origin.x.0, a.bounds.origin.x.0);
    }
    let red: render::Hsla = "red".parse::<style::color::Color>().unwrap().into();
    let selected = |scene: &Scene| {
        scene
            .quads
            .iter()
            .find(|q| q.background == red.into())
            .unwrap()
            .bounds
    };
    near(selected(&before).origin.y.0, range.origin.y);
    near(selected(&after).origin.y.0, moved_range.origin.y);
}

#[test]
fn placeholder_centers_using_its_own_line_height() {
    let editor = Editor::default();
    let css = "#edit{width:220px;height:90px;padding:0;font-size:13px;line-height:20px}\
               #edit::placeholder{font-size:10px;line-height:16px}";
    let (mut single, cache) = build(input(&editor).id("edit").placeholder("Hint"), css);
    let (mut multi, multi_cache) = build(textarea(&editor).id("edit").placeholder("Hint"), css);
    let a = paint(&mut single, &cache);
    let b = paint(&mut multi, &multi_cache);
    assert!(!a.monochrome_sprites.is_empty());
    assert_eq!(a.monochrome_sprites.len(), b.monochrome_sprites.len());
    for (a, b) in a.monochrome_sprites.iter().zip(&b.monochrome_sprites) {
        near(
            a.bounds.origin.y.0 - b.bounds.origin.y.0,
            (90.0 - 16.0) * 0.5,
        );
    }
}

#[test]
fn clicks_drags_and_key_callback_anchors_use_the_centered_line() {
    use std::{cell::Cell, rc::Rc};
    let editor = Editor::new("abcdef");
    let reported = Rc::new(Cell::new(None));
    let callback = reported.clone();
    let (mut tree, cache) = build(
        input(&editor).id("edit").on_key(move |_, cx| {
            callback.set(cx.caret_bounds());
            true
        }),
        "#edit{width:220px;height:100px;padding:4px;border-width:1px;line-height:20px;font-size:13px}",
    );
    let edit = id(&tree, "edit");
    tree.set_focused(Some(edit));
    for (phase, byte) in [(PointerPhase::Down, 2), (PointerPhase::Move, 4)] {
        let caret = tree
            .input_bounds_for_range(edit, byte..byte, &cache)
            .unwrap();
        tree.dispatch_input(
            edit,
            &InputEvent::Pointer {
                phase,
                position: Point::new(
                    caret.origin.x + 0.1,
                    caret.origin.y + caret.size.height * 0.5,
                ),
                modifiers: ModifiersState::empty(),
                clicks: 1,
            },
            &cache,
        );
        assert_eq!(editor.with(|s| s.selections().primary().head), byte);
    }
    assert_eq!(editor.with(|s| s.selections().primary().text_range()), 2..4);
    tree.dispatch_input(edit, &key(NamedKey::F1, ModifiersState::empty()), &cache);
    assert_eq!(
        reported.get(),
        tree.input_bounds_for_range(edit, 4..4, &cache)
    );
}

#[test]
fn composition_and_horizontal_scroll_keep_vertical_alignment_stable() {
    let editor = Editor::default();
    let (mut tree, cache) = build(
        input(&editor).id("edit"),
        "#edit{width:65px;height:60px;padding:0;font-size:13px;line-height:20px;caret-animation:manual}",
    );
    let edit = id(&tree, "edit");
    tree.set_focused(Some(edit));
    for event in [
        InputEvent::Text("a long filename that scrolls".into()),
        InputEvent::Preedit("Жa".into(), Some((3, 3))),
        InputEvent::Commit("Жa".into()),
        InputEvent::Scroll(Point::new(0.0, 100.0)),
    ] {
        tree.dispatch_input(edit, &event, &cache);
        tree.refresh_scroll_content(&cache);
        let scene = paint(&mut tree, &cache);
        let caret = scene.quads.iter().find(|q| q.spatial_pad == 1).unwrap();
        near(caret.bounds.origin.y.0, 20.0);
        let scroll = tree.scroll_metrics(edit).unwrap();
        near(scroll.offset.y, 0.0);
        assert!(scroll.offset.x > 0.0);
        let range = tree.text_input(edit).unwrap().selection_range();
        let anchor = tree
            .input_bounds_for_range(edit, range.end..range.end, &cache)
            .unwrap();
        near(anchor.origin.y, caret.bounds.origin.y.0);
    }
}

#[test]
fn oversized_single_line_stays_centered_and_clipped_after_scroll_and_edits() {
    let editor = Editor::new("Ag");
    let (mut tree, cache) = build(
        input(&editor).id("edit"),
        "#edit{width:200px;height:16px;padding:0;line-height:30px;caret-animation:manual}",
    );
    let edit = id(&tree, "edit");
    tree.set_focused(Some(edit));
    for event in [
        InputEvent::Scroll(Point::new(0.0, 50.0)),
        InputEvent::Text("b".into()),
    ] {
        tree.dispatch_input(edit, &event, &cache);
        let anchor = tree.input_bounds_for_range(edit, 0..0, &cache).unwrap();
        near(anchor.origin.y, -7.0);
        let scene = paint(&mut tree, &cache);
        let caret = scene.quads.iter().find(|q| q.spatial_pad == 1).unwrap();
        near(caret.bounds.origin.y.0, anchor.origin.y);
        near(caret.content_mask.bounds.origin.y.0, 0.0);
        near(caret.content_mask.bounds.size.height.0, 16.0);
        tree.refresh_scroll_content(&cache);
        near(tree.scroll_metrics(edit).unwrap().offset.y, 0.0);
    }
}
