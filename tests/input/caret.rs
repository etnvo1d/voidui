//! Shape changes affect retained caret ink, not text metrics or IME anchors.
use super::*;
use voidui::style::text::CaretShape;

fn ink(tree: &mut WidgetTree, cache: &TextLayoutCache) -> Bounds<render::ScaledPixels> {
    let scene = paint(tree, cache);
    let carets: Vec<_> = scene.quads.iter().filter(|q| q.spatial_pad == 1).collect();
    assert_eq!(carets.len(), 1);
    carets[0].bounds
}

#[test]
fn caret_shapes_follow_glyph_width_without_moving_ime_anchors() {
    let editor = Editor::new("Wi");
    let make = |shape| textarea(&editor).id("edit").caret_shape(shape);
    let (mut tree, cache) = build(
        make(CaretShape::Bar),
        "textarea{width:200px;caret-animation:manual}",
    );
    let edit = id(&tree, "edit");
    tree.set_focused(Some(edit));
    let anchor = tree.input_bounds_for_range(edit, 0..0, &cache).unwrap();
    let next = tree.input_bounds_for_range(edit, 1..1, &cache).unwrap();
    let bar = ink(&mut tree, &cache);
    let shaped = cache.system().stats().paragraphs_shaped;
    for shape in [CaretShape::Block, CaretShape::Underline, CaretShape::Bar] {
        tree.reconcile_root(make(shape));
        let actual = ink(&mut tree, &cache);
        assert_eq!(
            tree.input_bounds_for_range(edit, 0..0, &cache),
            Some(anchor)
        );
        assert_eq!(actual.origin.x.0, anchor.origin.x);
        if shape == CaretShape::Bar {
            assert_eq!(actual, bar);
        } else {
            assert!((actual.size.width.0 - (next.origin.x - anchor.origin.x)).abs() < 0.01);
            if shape == CaretShape::Block {
                assert_eq!(actual.size.height.0, bar.size.height.0);
            } else {
                assert_eq!(actual.size.height.0, bar.size.width.0);
                assert_eq!(actual.bottom(), bar.bottom());
            }
        }
    }
    assert_eq!(cache.system().stats().paragraphs_shaped, shaped);
}

#[test]
fn caret_shape_css_inherits_and_inline_overrides_it() {
    let (tree, _) = build(
        div().child(input(&Editor::new("a")).id("inherited")).child(
            input(&Editor::new("b"))
                .id("explicit")
                .caret_shape(CaretShape::Underline),
        ),
        "div{caret-shape:block}input{caret-shape:inherit}",
    );
    assert_eq!(
        tree.text_style(id(&tree, "inherited")).caret_shape,
        CaretShape::Block
    );
    assert_eq!(
        tree.text_style(id(&tree, "explicit")).caret_shape,
        CaretShape::Underline
    );
    for (css, shape) in [
        ("auto", CaretShape::Bar),
        ("bar", CaretShape::Bar),
        ("block", CaretShape::Block),
        ("underscore", CaretShape::Underline),
        ("initial", CaretShape::Bar),
    ] {
        let (tree, _) = build(
            input(&Editor::new("a")).id("edit"),
            &format!("input{{caret-shape:{css}}}"),
        );
        assert_eq!(tree.text_style(id(&tree, "edit")).caret_shape, shape);
    }
}

#[test]
fn block_caret_covers_the_whole_combining_grapheme_and_selection_endpoint() {
    let editor = Editor::new("a\u{301}Wi");
    let (mut tree, cache) = build(
        textarea(&editor).id("edit").caret_shape(CaretShape::Block),
        "textarea{caret-animation:manual}",
    );
    let edit = id(&tree, "edit");
    tree.set_focused(Some(edit));
    let first = ink(&mut tree, &cache);
    let next = tree
        .input_bounds_for_range(edit, "a\u{301}".len().."a\u{301}".len(), &cache)
        .unwrap();
    assert!((first.size.width.0 - (next.origin.x - first.origin.x.0)).abs() < 0.01);
    editor
        .update(|s| s.select(SelectionSet::single(Selection::range(0, "a\u{301}".len()))))
        .unwrap();
    assert_eq!(ink(&mut tree, &cache), first);
    editor
        .update(|s| s.select(SelectionSet::single(Selection::range("a\u{301}".len(), 0))))
        .unwrap();
    assert_eq!(ink(&mut tree, &cache), first);
}

#[test]
fn wide_carets_handle_empty_lines_wrapping_and_preedit() {
    for source in ["", "\n", "Wi Wi Wi Wi"] {
        let editor = Editor::new(source);
        let (mut tree, cache) = build(
            textarea(&editor).id("edit").caret_shape(CaretShape::Block),
            "textarea{width:45px;caret-animation:manual}",
        );
        let edit = id(&tree, "edit");
        tree.set_focused(Some(edit));
        for at in source
            .char_indices()
            .map(|(i, _)| i)
            .chain(std::iter::once(source.len()))
        {
            editor
                .update(|s| s.select(SelectionSet::single(Selection::caret(at))))
                .unwrap();
            let caret = ink(&mut tree, &cache);
            assert!(caret.size.width.0 > 1.0 && caret.size.width.0 < 45.0);
            assert!(caret.size.height.0 > 1.0);
        }
        tree.dispatch_input(
            edit,
            &InputEvent::Preedit("ni".into(), Some((2, 2))),
            &cache,
        );
        let caret = ink(&mut tree, &cache);
        assert!(
            caret.size.width.0 <= 2.0,
            "preedit must use the insertion bar"
        );
        tree.dispatch_input(edit, &InputEvent::CancelComposition, &cache);
        assert!(ink(&mut tree, &cache).size.width.0 > 2.0);
    }
}
