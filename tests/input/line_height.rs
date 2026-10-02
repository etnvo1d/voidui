//! Font fallback must not change a fixed-height input's scroll position.
use super::*;

#[test]
fn fallback_typing_and_preedit_keep_fixed_height_input_stationary() {
    let fonts = ParleyTextSystem::new_without_system_fonts("Ahem");
    fonts
        .add_fonts(vec![
            Cow::Borrowed(include_bytes!(
                "../../crates/voidui_gpui_wgpu/tests/fonts/Ahem.ttf"
            )),
            Cow::Borrowed(include_bytes!(
                "../../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
            )),
        ])
        .unwrap();
    let cache = TextLayoutCache::new(Arc::new(TextSystem::new(Arc::new(fonts))));
    let editor = Editor::default();
    let mut tree = WidgetTree::new();
    tree.build_root(input(&editor).id("edit"));
    tree.set_stylesheets(vec![
        Stylesheet::parse(
            "input { width:400px; height:32px; padding:0; border-width:0; \
         font-family:'Ahem','IBM Plex Sans'; font-size:14.5px; line-height:32px; \
         caret-animation:manual; }",
        )
        .unwrap(),
    ]);
    tree.layout(
        Size {
            width: AvailableSpace::Definite(500.0),
            height: AvailableSpace::Definite(500.0),
        },
        &cache,
    );
    let edit = id(&tree, "edit");
    tree.set_focused(Some(edit));
    let bounds = tree.bounds(edit);
    // Cyrillic uses the bundled fallback font on every host; the native-font
    // Chinese reproduction uses the same path without depending on OS fonts.
    let events = [
        InputEvent::Text("a".into()),
        InputEvent::Text("Ж".into()),
        InputEvent::Text("a".into()),
        InputEvent::Text("b".into()),
        InputEvent::Text("c".into()),
        InputEvent::Preedit("Ж".into(), Some((2, 2))),
        InputEvent::Preedit("Жa".into(), Some((3, 3))),
        InputEvent::CancelComposition,
        InputEvent::Text("d".into()),
        InputEvent::Preedit("Ж".into(), Some((2, 2))),
        InputEvent::Commit("Ж".into()),
        key(NamedKey::Backspace, ModifiersState::empty()),
    ];
    for event in events {
        tree.dispatch_input(edit, &event, &cache);
        // Repeated frames and IME geometry queries must agree with painting.
        for _ in 0..2 {
            tree.refresh_scroll_content(&cache);
            let scene = paint(&mut tree, &cache);
            let scroll = tree.scroll_metrics(edit).unwrap();
            assert_eq!(tree.bounds(edit), bounds, "{event:?}");
            assert_eq!(scroll.content.height, 32.0, "{event:?}");
            assert_eq!(scroll.offset.y, 0.0, "{event:?}");
            let caret = scene.quads.iter().find(|q| q.spatial_pad == 1).unwrap();
            assert_eq!(caret.bounds.origin.y.0, bounds.origin.y, "{event:?}");
            assert_eq!(caret.bounds.size.height.0, 32.0, "{event:?}");
            let anchor = tree.input_bounds_for_range(edit, 0..0, &cache).unwrap();
            assert_eq!(anchor.origin.y, bounds.origin.y, "{event:?}");
            assert_eq!(anchor.size.height, 32.0, "{event:?}");
        }
    }
}
