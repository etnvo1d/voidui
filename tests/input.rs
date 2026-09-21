//! Native-font CPU scene tests for input, IME, multi-caret navigation and CSS.
#![cfg(feature = "editing")]
use std::{
    borrow::Cow,
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Instant,
};
use voidui::{
    core::{
        event::EventResult,
        geometry::Point,
        input::{InputEvent, KeyInput, PointerPhase},
        layout::{AvailableSpace, Size},
        widget::WidgetId,
        widget_tree::WidgetTree,
    },
    editing::{Edit, EditKind, Selection, SelectionSet, Transaction},
    render::{
        self, AtlasKey, AtlasTextureId, AtlasTile, Bounds, DevicePixels, Painter, ParleyTextSystem,
        PlatformAtlas, Scene, TextLayoutCache, TextSystem, TileId, point, px, size,
    },
    style::css::Stylesheet,
    *,
};
use winit::keyboard::{Key, ModifiersState, NamedKey};
#[path = "input/embedded.rs"]
mod embedded;
fn cache() -> TextLayoutCache {
    let fonts = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    fonts
        .add_fonts(vec![Cow::Borrowed(include_bytes!(
            "../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
        ))])
        .unwrap();
    TextLayoutCache::new(Arc::new(TextSystem::new(Arc::new(fonts))))
}
fn build(root: impl IntoElement, css: &str) -> (WidgetTree, TextLayoutCache) {
    let cache = cache();
    let mut tree = WidgetTree::new();
    tree.build_root(root);
    tree.set_stylesheets(vec![
        Stylesheet::parse(&format!(
            "*{{font-family:'IBM Plex Sans';font-size:20px;line-height:30px}}{css}"
        ))
        .unwrap(),
    ]);
    tree.layout(
        Size {
            width: AvailableSpace::Definite(500.0),
            height: AvailableSpace::Definite(500.0),
        },
        &cache,
    );
    (tree, cache)
}
fn key(named: NamedKey, modifiers: ModifiersState) -> InputEvent {
    InputEvent::Key(KeyInput {
        key: Key::Named(named),
        modifiers,
        repeat: false,
    })
}
fn id(tree: &WidgetTree, id: &str) -> WidgetId {
    tree.find_by_id(id).unwrap()
}
#[derive(Default)]
struct CpuAtlas(Mutex<HashMap<AtlasKey, AtlasTile>>);
impl PlatformAtlas for CpuAtlas {
    fn get_or_insert_with<'a>(
        &self,
        key: &AtlasKey,
        build: &mut dyn FnMut() -> render::Result<
            Option<(render::Size<DevicePixels>, Cow<'a, [u8]>)>,
        >,
    ) -> render::Result<Option<AtlasTile>> {
        let mut tiles = self.0.lock().unwrap();
        if let Some(tile) = tiles.get(key) {
            return Ok(Some(*tile));
        }
        let Some((dimensions, _)) = build()? else {
            return Ok(None);
        };
        let tile = AtlasTile {
            texture_id: AtlasTextureId {
                index: 0,
                kind: key.texture_kind(),
            },
            tile_id: TileId(tiles.len() as u32),
            padding: 0,
            bounds: Bounds::new(point(DevicePixels(0), DevicePixels(0)), dimensions),
        };
        tiles.insert(key.clone(), tile);
        Ok(Some(tile))
    }
    fn remove(&self, key: &AtlasKey) {
        self.0.lock().unwrap().remove(key);
    }
}
fn paint(tree: &mut WidgetTree, cache: &TextLayoutCache) -> Scene {
    let changes = tree.update_styles(Instant::now());
    if changes.layout {
        tree.layout(
            Size {
                width: AvailableSpace::Definite(500.0),
                height: AvailableSpace::Definite(500.0),
            },
            cache,
        );
    }
    let mut scene = Scene::default();
    let atlas = CpuAtlas::default();
    let mut painter = Painter::new(
        &mut scene,
        &atlas,
        cache.system().clone(),
        size(px(500.0), px(500.0)),
        1.0,
    )
    .unwrap();
    tree.draw(&mut painter).unwrap();
    drop(painter);
    scene.finish();
    scene
}
#[test]
fn typing_changes_paint_not_box_layout_and_reuses_unchanged_paragraphs() {
    let editor = Editor::new("first\nsecond\nthird");
    let (mut tree, cache) = build(
        textarea(&editor).id("edit"),
        "textarea{width:200px;height:100px}",
    );
    let edit = id(&tree, "edit");
    paint(&mut tree, &cache);
    let before = cache.system().stats().paragraphs_shaped;
    let bounds = tree.bounds(edit);
    tree.dispatch_input(edit, &InputEvent::Text("X".into()), &cache);
    let changes = tree.update_styles(Instant::now());
    assert!(changes.paint);
    assert!(!changes.layout);
    assert_eq!(tree.bounds(edit), bounds);
    paint(&mut tree, &cache);
    assert_eq!(cache.system().stats().paragraphs_shaped - before, 1);
    let before = cache.system().stats().paragraphs_shaped;
    for _ in 0..5 {
        tree.dispatch_input(
            edit,
            &key(NamedKey::ArrowRight, ModifiersState::empty()),
            &cache,
        );
        paint(&mut tree, &cache);
    }
    assert_eq!(cache.system().stats().paragraphs_shaped, before);
}
#[test]
fn input_and_textarea_apply_newline_policy_and_readonly() {
    let one = Editor::default();
    let many = Editor::default();
    let (mut tree, cache) = build(
        div()
            .child(input(&one).id("one"))
            .child(textarea(&many).id("many")),
        "",
    );
    for name in ["one", "many"] {
        let edit = id(&tree, name);
        tree.dispatch_input(edit, &InputEvent::Paste("a\r\nb\rc\nd".into()), &cache);
    }
    assert_eq!(one.text(), "abcd");
    assert_eq!(many.text(), "a\nb\nc\nd");
    let edit = id(&tree, "one");
    tree.set_attribute(edit, "readonly", Some(""));
    tree.dispatch_input(edit, &InputEvent::Text("x".into()), &cache);
    assert_eq!(one.text(), "abcd");
    tree.dispatch_input(
        edit,
        &key(NamedKey::Backspace, ModifiersState::empty()),
        &cache,
    );
    assert_eq!(one.text(), "abcd");
}
#[test]
fn ime_keeps_committed_value_and_renders_without_history_until_commit() {
    let editor = Editor::new("abc");
    let (mut tree, cache) = build(input(&editor).id("edit"), "");
    let edit = id(&tree, "edit");
    tree.set_focused(Some(edit));
    tree.dispatch_input(
        edit,
        &InputEvent::Preedit("ni".into(), Some((2, 2))),
        &cache,
    );
    assert_eq!(editor.text(), "abc");
    assert!(tree.text_input(edit).unwrap().is_composing());
    paint(&mut tree, &cache);
    tree.reconcile_root(input(&editor).id("edit"));
    assert!(editor.with(|s| s.composition().is_some()));
    tree.dispatch_input(edit, &InputEvent::Commit("你".into()), &cache);
    assert_eq!(editor.text(), "你abc");
    editor.update(|s| s.undo()).unwrap();
    assert_eq!(editor.text(), "abc");
    tree.dispatch_input(edit, &InputEvent::Preedit("x".into(), Some((1, 1))), &cache);
    tree.set_focused(None);
    assert!(editor.with(|s| s.composition().is_none()));
    assert_eq!(editor.text(), "abc");
}
#[test]
fn standard_css_placeholder_caret_selection_and_readonly_selectors() {
    let editor = Editor::default();
    let (mut tree, cache) = build(
        input(&editor).id("edit").placeholder("Type here"),
        r#"
        input { padding: 4px; border: 2px solid black; width: 220px; box-sizing: border-box; caret-color: red; caret-animation: manual; }
        input::placeholder { color: rgb(0, 128, 0); font-size: 14px; }
        input:placeholder-shown { background-color: #eeeeee; }
        input:read-only { color: blue; }
        input::selection { color: white; background-color: red; }
    "#,
    );
    let edit = id(&tree, "edit");
    assert_eq!(tree.bounds(edit).size.width, 220.0);
    assert!(!tree.text_style(edit).caret_animation);
    let scene = paint(&mut tree, &cache);
    let green: render::Hsla =
        style::color::Color::from(style::color::Rgba8::from_rgb8(0, 128, 0)).into();
    assert!(scene.monochrome_sprites.iter().any(|g| g.color == green));
    tree.dispatch_input(edit, &InputEvent::Text("abc".into()), &cache);
    paint(&mut tree, &cache);
    tree.set_attribute(edit, "readonly", Some(""));
    tree.update_styles(Instant::now());
    let blue: style::color::Color = "blue".parse().unwrap();
    assert_eq!(tree.text_style(edit).color, blue);
}
#[test]
fn inputgroup_delegates_background_focus_but_preserves_button_focus() {
    let editor = Editor::default();
    let (mut tree, _cache) = build(
        input_group()
            .id("group")
            .child(text("USD").id("addon"))
            .child(input(&editor).id("edit"))
            .child(div().tag("button").on_click(|| {}).id("button")),
        "#group{padding:10px;gap:8px;width:400px}#button{width:20px;height:20px}input{width:200px}",
    );
    let addon = tree.bounds(id(&tree, "addon"));
    tree.pointer_moved(Some(Point::new(addon.origin.x + 2.0, addon.origin.y + 2.0)));
    tree.pointer_pressed(true);
    assert_eq!(tree.focused(), Some(id(&tree, "edit")));
    tree.pointer_pressed(false);
    let button = tree.bounds(id(&tree, "button"));
    tree.pointer_moved(Some(Point::new(
        button.origin.x + 2.0,
        button.origin.y + 2.0,
    )));
    tree.pointer_pressed(true);
    assert_eq!(tree.focused(), Some(id(&tree, "button")));
}
#[test]
fn visual_navigation_crosses_paragraphs_and_does_not_split_graphemes() {
    let editor = Editor::new("a\ne\u{301}\n👩‍💻x");
    let (mut tree, cache) = build(textarea(&editor).id("edit"), "");
    let edit = id(&tree, "edit");
    for _ in 0..25 {
        tree.dispatch_input(
            edit,
            &key(NamedKey::ArrowRight, ModifiersState::empty()),
            &cache,
        );
        let at = editor.with(|s| s.selections().primary().head);
        assert!(
            [0, 1, 2, 5, 6, 17, 18].contains(&at),
            "caret split a grapheme at {at}"
        );
    }
    assert_eq!(
        editor.with(|s| s.selections().primary().head),
        editor.text().len()
    );
    for _ in 0..25 {
        tree.dispatch_input(
            edit,
            &key(NamedKey::ArrowLeft, ModifiersState::empty()),
            &cache,
        );
    }
    assert_eq!(editor.with(|s| s.selections().primary().head), 0);
}
#[test]
fn two_cursors_type_and_custom_keymap_accepts_a_completion_transaction() {
    let editor = Editor::new("a a");
    editor
        .update(|s| {
            s.select(SelectionSet::new([Selection::caret(1), Selection::caret(3)], 1).unwrap())
        })
        .unwrap();
    let (mut tree, cache) = build(
        textarea(&editor).id("edit").on_key(|key, editor| {
            if key.key == Key::Named(NamedKey::Tab) {
                editor
                    .update(|s| s.replace_selections("pple", EditKind::Command))
                    .unwrap();
                true
            } else {
                false
            }
        }),
        "",
    );
    let edit = id(&tree, "edit");
    assert_eq!(
        tree.dispatch_input(edit, &key(NamedKey::Tab, ModifiersState::empty()), &cache),
        EventResult::Handled
    );
    assert_eq!(editor.text(), "apple apple");
    editor.update(|s| s.undo()).unwrap();
    assert_eq!(editor.text(), "a a");
}
#[test]
fn horizontal_scrolling_keeps_caret_inside_content_box() {
    let editor = Editor::default();
    let (mut tree, cache) = build(input(&editor).id("edit"), "input{width:60px;padding:5px}");
    let edit = id(&tree, "edit");
    tree.dispatch_input(
        edit,
        &InputEvent::Text("abcdefghijklmnopqrstuvwxyz".into()),
        &cache,
    );
    let range = tree.text_input(edit).unwrap().selection_range();
    let caret = tree.input_bounds_for_range(edit, range, &cache).unwrap();
    let bounds = tree.content_bounds(edit);
    assert!(caret.origin.x >= bounds.origin.x);
    assert!(caret.origin.x + caret.size.width <= bounds.origin.x + bounds.size.width + 0.01);
}
#[test]
fn external_edits_wake_only_mounted_views_and_do_not_reset_on_reconcile() {
    let editor = Editor::new("test");
    let (mut tree, cache) = build(input(&editor).id("edit"), "");
    let wake = std::rc::Rc::new(std::cell::Cell::new(0));
    let count = wake.clone();
    tree.set_update_waker(move || count.set(count.get() + 1));
    editor
        .update(|s| s.replace_selections("X", EditKind::Command))
        .unwrap();
    editor
        .update(|s| s.replace_selections("Y", EditKind::Command))
        .unwrap();
    assert_eq!(wake.get(), 1);
    paint(&mut tree, &cache);
    tree.reconcile_root(input(&editor).id("edit"));
    assert_eq!(editor.text(), "XYtest");
    tree.remove_subtree(id(&tree, "edit"));
    let before = wake.get();
    editor
        .update(|s| s.transact(Transaction::new(s.revision(), [Edit::new(0..0, "Z")])))
        .unwrap();
    assert_eq!(wake.get(), before);
}
#[test]
fn pointer_drag_and_disabled_input_do_not_leak_to_document_selection() {
    let editor = Editor::new("abcdef");
    let (mut tree, cache) = build(input(&editor).id("edit"), "input{width:200px}");
    let edit = id(&tree, "edit");
    let a = tree
        .input_bounds_for_range(edit, 1..1, &cache)
        .unwrap()
        .origin;
    let b = tree
        .input_bounds_for_range(edit, 4..4, &cache)
        .unwrap()
        .origin;
    for (phase, position) in [
        (PointerPhase::Down, a),
        (PointerPhase::Move, b),
        (PointerPhase::Up, b),
    ] {
        tree.dispatch_input(
            edit,
            &InputEvent::Pointer {
                phase,
                position: Point::new(position.x, position.y + 10.0),
                modifiers: ModifiersState::empty(),
                clicks: 1,
            },
            &cache,
        );
    }
    assert_eq!(tree.text_input(edit).unwrap().selected_text(), "bcd");
    assert_eq!(tree.selected_text(), "");
    tree.set_attribute(edit, "disabled", Some(""));
    assert_eq!(
        tree.dispatch_input(edit, &InputEvent::Text("x".into()), &cache),
        EventResult::Unhandled
    );
    assert_eq!(editor.text(), "abcdef");
}
#[test]
fn single_line_constraint_covers_external_edits_and_older_history() {
    let editor = Editor::new("a\nb");
    editor
        .update(|s| s.transact(Transaction::new(0, [Edit::new(1..2, "")])))
        .unwrap();
    let (mut tree, _cache) = build(input(&editor).id("edit"), "");
    assert!(editor.update(|s| s.undo()).is_err());
    assert_eq!(editor.text(), "ab");
    assert!(
        editor
            .update(|s| s.replace_selections("\n", EditKind::Command))
            .is_err()
    );
    tree.remove_subtree(id(&tree, "edit"));
    editor.update(|s| s.undo()).unwrap();
    assert_eq!(editor.text(), "a\nb");
}
#[test]
fn platform_queries_use_one_composed_utf8_coordinate_space() {
    let editor = Editor::new("a中z");
    editor
        .update(|s| s.select(SelectionSet::single(Selection::range(1, 4))))
        .unwrap();
    let (mut tree, cache) = build(input(&editor).id("edit"), "");
    let edit = id(&tree, "edit");
    tree.dispatch_input(
        edit,
        &InputEvent::Preedit("你好".into(), Some((3, 3))),
        &cache,
    );
    let client = tree.text_input(edit).unwrap();
    assert_eq!(client.marked_range(), Some(1..7));
    assert_eq!(client.selection_range(), 4..4);
    assert_eq!(client.text_for_range(0..8).as_deref(), Some("a你好z"));
    assert!(client.text_for_range(2..4).is_none());
    assert!(tree.input_bounds_for_range(edit, 4..4, &cache).is_some());
    assert_eq!(editor.text(), "a中z");
}
#[test]
fn caret_timer_only_runs_for_active_automatic_carets() {
    let editor = Editor::new("abc");
    let (mut tree, _cache) = build(input(&editor).id("edit"), "");
    let edit = id(&tree, "edit");
    assert!(
        tree.text_input(edit)
            .unwrap()
            .next_frame(Instant::now())
            .is_none()
    );
    tree.set_focused(Some(edit));
    assert!(
        tree.text_input(edit)
            .unwrap()
            .next_frame(Instant::now())
            .is_some()
    );
    editor.update(|s| s.select_all()).unwrap();
    assert!(
        tree.text_input(edit)
            .unwrap()
            .next_frame(Instant::now())
            .is_none()
    );
    editor
        .update(|s| s.select(SelectionSet::single(Selection::caret(1))))
        .unwrap();
    tree.set_stylesheets(vec![
        Stylesheet::parse("input{caret-animation:manual}").unwrap(),
    ]);
    tree.update_styles(Instant::now());
    assert!(
        tree.text_input(edit)
            .unwrap()
            .next_frame(Instant::now())
            .is_none()
    );
    tree.set_focused(None);
    assert!(
        tree.text_input(edit)
            .unwrap()
            .next_frame(Instant::now())
            .is_none()
    );
}
#[test]
fn changing_colors_and_width_does_not_reshape_editor_text() {
    let editor = Editor::new("a paragraph with words that wrap at a narrow width");
    let (mut tree, cache) = build(textarea(&editor).id("edit"), "textarea{width:200px}");
    paint(&mut tree, &cache);
    let before = cache.system().stats().paragraphs_shaped;
    tree.set_stylesheets(vec![Stylesheet::parse("*{font-family:'IBM Plex Sans';font-size:20px;line-height:30px}textarea{color:red;caret-color:green;width:150px}").unwrap()]);
    paint(&mut tree, &cache);
    assert_eq!(cache.system().stats().paragraphs_shaped, before);
}
#[test]
fn custom_keymap_can_use_visual_motion_and_completion_anchor() {
    let editor = Editor::new("abc\ndef");
    let (mut tree, cache) = build(
        textarea(&editor).id("edit").on_key(|key, context| {
            if key.key == Key::Character("j".into()) {
                context
                    .move_selection(editing::Motion::Down, false)
                    .unwrap();
                assert!(context.caret_bounds().is_some());
                true
            } else {
                false
            }
        }),
        "",
    );
    let edit = id(&tree, "edit");
    tree.dispatch_input(
        edit,
        &InputEvent::Key(KeyInput {
            key: Key::Character("j".into()),
            modifiers: ModifiersState::empty(),
            repeat: false,
        }),
        &cache,
    );
    assert_eq!(editor.with(|s| s.selections().primary().head), 4);
    assert_eq!(editor.text(), "abc\ndef");
}
#[test]
fn removing_rows_attribute_restores_intrinsic_defaults() {
    let editor = Editor::default();
    let (mut tree, cache) = build(textarea(&editor).id("edit").rows(5), "");
    let edit = id(&tree, "edit");
    assert_eq!(tree.bounds(edit).size.height, 150.0);
    tree.set_attribute(edit, "rows", None);
    tree.layout(
        Size {
            width: AvailableSpace::Definite(500.0),
            height: AvailableSpace::Definite(500.0),
        },
        &cache,
    );
    assert_eq!(tree.bounds(edit).size.height, 60.0);
}

type StringStateSlot = std::rc::Rc<std::cell::RefCell<Option<State<String>>>>;
#[component]
fn bound_form(
    out: StringStateSlot,
    renders: std::rc::Rc<std::cell::Cell<usize>>,
) -> impl IntoElement {
    renders.set(renders.get() + 1);
    let value = state(String::new);
    *out.borrow_mut() = Some(value.clone());
    div()
        .child(input(value.clone()).id("one").placeholder("Text"))
        .child(textarea(value).id("two"))
}
#[test]
fn string_state_binding_is_two_way_without_rerendering_its_parent() {
    let out = StringStateSlot::default();
    let renders = std::rc::Rc::new(std::cell::Cell::new(0));
    let (mut tree, cache) = build(bound_form(out.clone(), renders.clone()), "");
    let value = out.borrow().as_ref().unwrap().clone();
    let one = id(&tree, "one");
    let two = id(&tree, "two");
    tree.dispatch_input(one, &InputEvent::Text("abc".into()), &cache);
    paint(&mut tree, &cache);
    assert_eq!(value.get(), "abc");
    assert_eq!(
        tree.text_input(two)
            .unwrap()
            .text_for_range(0..3)
            .as_deref(),
        Some("abc")
    );
    assert_eq!(renders.get(), 1, "binding subscribed the parent component");
    value.set("external".into());
    paint(&mut tree, &cache);
    assert_eq!(
        tree.text_input(one)
            .unwrap()
            .text_for_range(0..8)
            .as_deref(),
        Some("external")
    );
    assert_eq!(renders.get(), 1);
    assert_eq!(id(&tree, "one"), one);
}
#[test]
fn state_echoes_preserve_ime_selection_and_undo() {
    let out = StringStateSlot::default();
    let renders = std::rc::Rc::new(std::cell::Cell::new(0));
    let (mut tree, cache) = build(bound_form(out.clone(), renders), "");
    let value = out.borrow().as_ref().unwrap().clone();
    let one = id(&tree, "one");
    tree.dispatch_input(one, &InputEvent::Text("a".into()), &cache);
    paint(&mut tree, &cache);
    tree.dispatch_input(one, &InputEvent::Preedit("ni".into(), Some((2, 2))), &cache);
    paint(&mut tree, &cache);
    assert_eq!(value.get(), "a");
    value.set("a".into());
    paint(&mut tree, &cache);
    assert!(tree.text_input(one).unwrap().is_composing());
    tree.dispatch_input(one, &InputEvent::Commit("你".into()), &cache);
    paint(&mut tree, &cache);
    assert_eq!(value.get(), "a你");
    let command = if cfg!(target_os = "macos") {
        ModifiersState::SUPER
    } else {
        ModifiersState::CONTROL
    };
    tree.dispatch_input(
        one,
        &InputEvent::Key(KeyInput {
            key: Key::Character("z".into()),
            modifiers: command,
            repeat: false,
        }),
        &cache,
    );
    paint(&mut tree, &cache);
    assert_eq!(value.get(), "a");
    assert_eq!(tree.text_input(one).unwrap().selection_range(), 1..1);
}
#[test]
fn explicit_component_readers_still_update_alongside_widget_bindings() {
    let out = StringStateSlot::default();
    let saved = out.clone();
    let (mut tree, cache) = build(
        component(move || {
            let value = state(String::new);
            *saved.borrow_mut() = Some(value.clone());
            div()
                .child(input(&value).id("edit"))
                .child(text(value.get()).id("label"))
        }),
        "",
    );
    let edit = id(&tree, "edit");
    tree.dispatch_input(edit, &InputEvent::Text("hello".into()), &cache);
    paint(&mut tree, &cache);
    assert_eq!(tree.text_content(id(&tree, "label")), Some("hello"));
    tree.dispatch_input(
        edit,
        &InputEvent::Preedit("ni".into(), Some((2, 2))),
        &cache,
    );
    paint(&mut tree, &cache);
    out.borrow().as_ref().unwrap().set("hello".into());
    paint(&mut tree, &cache);
    assert!(tree.text_input(edit).unwrap().is_composing());
}
#[test]
fn partial_grapheme_multiselections_do_not_double_blend_highlights() {
    let editor = Editor::new("e\u{301}");
    let (mut tree, cache) = build(
        input(&editor),
        "input::selection{background-color:rgba(20,80,200,0.4)}",
    );
    editor
        .update(|s| s.select(SelectionSet::single(Selection::range(0, 3))))
        .unwrap();
    let single = paint(&mut tree, &cache);
    editor
        .update(|s| {
            s.select(
                SelectionSet::new([Selection::range(0, 1), Selection::range(1, 3)], 0).unwrap(),
            )
        })
        .unwrap();
    let multiple = paint(&mut tree, &cache);
    assert_eq!(multiple.quads.len(), single.quads.len());
    assert_eq!(
        multiple.monochrome_sprites.len(),
        single.monochrome_sprites.len()
    );
}
#[test]
fn hidden_ancestors_block_text_input() {
    let editor = Editor::new("abc");
    let (mut tree, cache) = build(
        div().id("parent").child(input(&editor).id("edit")),
        "#parent{display:none}",
    );
    let edit = id(&tree, "edit");
    assert_eq!(
        tree.dispatch_input(edit, &InputEvent::Text("x".into()), &cache),
        EventResult::Unhandled
    );
    assert_eq!(editor.text(), "abc");
}

/// Hot reload replaces the whole sheet; a `::placeholder` declaration lives
/// outside the node's computed style, so only the input client can report it.
#[test]
fn placeholder_only_stylesheet_edit_invalidates_paint() {
    let editor = Editor::default();
    let sheet = |color: &str| {
        Stylesheet::parse(&format!(
            "*{{font-family:'IBM Plex Sans';font-size:20px;line-height:30px}}\
             .search-input input::placeholder{{color:{color}}}"
        ))
        .unwrap()
    };
    let hsla = |css: &str| -> render::Hsla { css.parse::<style::color::Color>().unwrap().into() };
    let (mut tree, cache) = build(
        div()
            .class("search-input")
            .child(input(&editor).id("edit").placeholder("Search")),
        ".search-input input::placeholder{color:#777b83}",
    );
    let scene = paint(&mut tree, &cache);
    assert!(
        scene
            .monochrome_sprites
            .iter()
            .any(|g| g.color == hsla("#777b83"))
    );
    // A settled tree must stay asleep.
    assert!(!tree.update_styles(Instant::now()).paint);
    assert!(!tree.set_stylesheets(vec![sheet("#777b83")]));
    assert!(!tree.update_styles(Instant::now()).paint);
    // Only the pseudo-element color differs from the installed sheet.
    assert!(tree.set_stylesheets(vec![sheet("#ff0000")]));
    let changes = tree.update_styles(Instant::now());
    assert!(changes.paint, "::placeholder color must invalidate paint");
    assert!(!changes.layout, "placeholder color is paint-only");
    let scene = paint(&mut tree, &cache);
    assert!(
        scene
            .monochrome_sprites
            .iter()
            .any(|g| g.color == hsla("#ff0000"))
    );
}

#[test]
fn rich_editor_styles_survive_inheritance_ime_and_history_in_the_real_view() {
    use voidui::editing::StylePatch;
    let red: render::Hsla = "red".parse::<style::color::Color>().unwrap().into();
    let blue: render::Hsla = "blue".parse::<style::color::Color>().unwrap().into();
    let editor = Editor::from_rich(span("plain ").child(span("rich").color(red)));
    let (mut tree, cache) = build(
        rich_editor(&editor).id("edit"),
        "textarea{width:300px;height:160px;color:blue;caret-animation:manual}",
    );
    let edit = id(&tree, "edit");
    let scene = paint(&mut tree, &cache);
    assert!(scene.monochrome_sprites.iter().any(|g| g.color == red));
    assert!(scene.monochrome_sprites.iter().any(|g| g.color == blue));
    let before = cache.system().stats().paragraphs_shaped;
    editor
        .update(|s| s.select(SelectionSet::single(Selection::caret(10))))
        .unwrap();
    paint(&mut tree, &cache);
    assert_eq!(cache.system().stats().paragraphs_shaped, before);
    tree.dispatch_input(
        edit,
        &InputEvent::Preedit("XY".into(), Some((2, 2))),
        &cache,
    );
    let scene = paint(&mut tree, &cache);
    assert_eq!(editor.text(), "plain rich");
    assert!(
        scene
            .monochrome_sprites
            .iter()
            .filter(|g| g.color == red)
            .count()
            >= 6
    );
    tree.dispatch_input(edit, &InputEvent::Commit("XY".into()), &cache);
    assert_eq!(editor.text(), "plain richXY");
    assert_eq!(
        editor.with(|s| s.document().spans()[0].range.clone()),
        6..12
    );
    editor.update(|s| s.undo()).unwrap();
    paint(&mut tree, &cache);
    assert_eq!(editor.text(), "plain rich");
    editor
        .update(|s| s.select(SelectionSet::single(Selection::range(6, 10))))
        .unwrap();
    editor
        .update(|s| {
            s.format_selections(StylePatch {
                font_size: Some(Some(40.0)),
                line_height: Some(Some(50.0)),
                ..Default::default()
            })
        })
        .unwrap();
    paint(&mut tree, &cache);
    let caret = tree.input_bounds_for_range(edit, 8..8, &cache).unwrap();
    assert!(
        caret.size.height >= 49.0,
        "caret must follow mixed typography: {caret:?}"
    );
    editor.update(|s| s.undo()).unwrap();
    paint(&mut tree, &cache);
    assert_eq!(
        editor.with(|s| s.document().spans()[0].style.font_size),
        None
    );
}

#[test]
fn automatic_highlighting_in_change_callback_does_not_create_undo_steps() {
    let editor = Editor::default();
    let red: render::Hsla = "red".parse::<style::color::Color>().unwrap().into();
    let (mut tree, cache) = build(
        rich_editor(&editor).id("edit").on_change(move |editor| {
            editor
                .update(|state| {
                    let spans = (!state.text().is_empty()).then(|| {
                        StyleSpan::new(0..state.text().len(), InlineStyle::new().color(red))
                    });
                    state.set_highlights(state.revision(), spans)
                })
                .unwrap();
        }),
        "textarea{width:200px;caret-animation:manual}",
    );
    let edit = id(&tree, "edit");
    tree.dispatch_input(edit, &InputEvent::Text("a".into()), &cache);
    tree.dispatch_input(edit, &InputEvent::Text("b".into()), &cache);
    let scene = paint(&mut tree, &cache);
    assert!(scene.monochrome_sprites.iter().any(|s| s.color == red));
    assert!(editor.with(|s| s.document().spans().is_empty()));
    // The callback runs on undo and redo too. Neither rehighlighting nor clearing
    // an empty highlight result may consume history or destroy the redo branch.
    let command = if cfg!(target_os = "macos") {
        ModifiersState::SUPER
    } else {
        ModifiersState::CONTROL
    };
    let history_key = |modifiers| {
        InputEvent::Key(KeyInput {
            key: Key::Character("z".into()),
            modifiers,
            repeat: false,
        })
    };
    tree.dispatch_input(edit, &history_key(command), &cache);
    assert_eq!(editor.text(), "");
    assert!(editor.with(|s| s.can_redo()));
    tree.dispatch_input(edit, &history_key(command | ModifiersState::SHIFT), &cache);
    assert_eq!(editor.text(), "ab");
    assert!(
        paint(&mut tree, &cache)
            .monochrome_sprites
            .iter()
            .any(|s| s.color == red)
    );
}

#[test]
fn external_highlight_updates_repaint_without_document_changes_or_caret_reshaping() {
    let editor = Editor::new("abc");
    let (mut tree, cache) = build(rich_editor(&editor).id("edit"), "textarea{color:blue}");
    paint(&mut tree, &cache);
    let red: render::Hsla = "red".parse::<style::color::Color>().unwrap().into();
    editor
        .update(|s| {
            s.set_highlights(
                s.revision(),
                [StyleSpan::new(0..3, InlineStyle::new().color(red))],
            )
        })
        .unwrap();
    let updates = tree.update_styles(Instant::now());
    assert!(updates.paint);
    assert!(!updates.layout);
    assert_eq!(editor.with(|s| s.revision()), 0);
    let scene = paint(&mut tree, &cache);
    assert!(scene.monochrome_sprites.iter().all(|g| g.color == red));
    let shapes = cache.system().stats().paragraphs_shaped;
    editor
        .update(|s| s.select(SelectionSet::single(Selection::caret(1))))
        .unwrap();
    paint(&mut tree, &cache);
    assert_eq!(cache.system().stats().paragraphs_shaped, shapes);
    editor.update(|s| s.clear_highlights());
    let scene = paint(&mut tree, &cache);
    assert!(scene.monochrome_sprites.iter().all(|g| g.color != red));
}

#[test]
fn projection_widgets_preserve_outer_selection_and_forward_embedded_input() {
    use editing::{EditorViews, Projection, Replacement, ViewId, WidgetView};
    let outer = Editor::new("a [control] z");
    let inner = Editor::new("inside");
    let child = inner.clone();
    outer
        .update(|s| s.select(SelectionSet::single(Selection::caret(13))))
        .unwrap();
    outer
        .update(|s| {
            s.set_projection(
                s.revision(),
                Projection::new().replace(Replacement::object(ViewId(1), 2..11)),
            )
        })
        .unwrap();
    let views = EditorViews::new().register(ViewId(1), move || {
        let child = child.clone();
        Box::new(WidgetView::new(move || {
            input(&child)
                .font("IBM Plex Sans")
                .width(100.0)
                .height(35.0)
                .into_element()
        }))
    });
    let (mut tree, cache) = build(
        rich_editor(&outer).views(views).id("edit"),
        "textarea{width:300px;height:150px;caret-animation:manual}",
    );
    paint(&mut tree, &cache);
    let edit = id(&tree, "edit");
    let bounds = tree.input_bounds_for_range(edit, 2..2, &cache).unwrap();
    let position = Point::new(bounds.origin.x + 10.0, bounds.origin.y + 15.0);
    tree.dispatch_input(
        edit,
        &InputEvent::Pointer {
            phase: PointerPhase::Down,
            position,
            modifiers: ModifiersState::empty(),
            clicks: 1,
        },
        &cache,
    );
    tree.dispatch_input(
        edit,
        &InputEvent::Pointer {
            phase: PointerPhase::Up,
            position,
            modifiers: ModifiersState::empty(),
            clicks: 1,
        },
        &cache,
    );
    tree.dispatch_input(edit, &InputEvent::Text("X".into()), &cache);
    assert!(inner.text().contains('X'));
    assert_eq!(outer.text(), "a [control] z");
    assert_eq!(
        outer.with(|s| s.selections().primary()),
        Selection::caret(13)
    );
    tree.dispatch_input(
        edit,
        &InputEvent::Preedit("中文".into(), Some((6, 6))),
        &cache,
    );
    assert!(tree.text_input(edit).unwrap().marked_range().is_some());
    assert!(tree.input_bounds_for_range(edit, 1..1, &cache).is_some());
    tree.dispatch_input(
        edit,
        &key(NamedKey::Escape, ModifiersState::empty()),
        &cache,
    );
    assert!(inner.with(|s| s.composition().is_none()));
    tree.dispatch_input(
        edit,
        &key(NamedKey::Backspace, ModifiersState::empty()),
        &cache,
    );
    assert_eq!(outer.text(), "a [control] ");
    outer.update(|s| s.undo()).unwrap();
    assert_eq!(outer.text(), "a [control] z");
}

#[test]
fn embedded_widget_receives_hovered_wheel_without_focus() {
    use editing::{EditorViews, Projection, Replacement, ViewId, WidgetView};
    use std::{cell::RefCell, rc::Rc};

    let editor = Editor::new(format!("[view]\n{}", "line\n".repeat(30)));
    editor
        .update(|s| {
            s.set_projection(
                s.revision(),
                Projection::new().replace(Replacement::object(ViewId(1), 0..6)),
            )
        })
        .unwrap();
    let received = Rc::new(RefCell::new(Vec::new()));
    let events = received.clone();
    let views = EditorViews::new().register(ViewId(1), move || {
        let events = events.clone();
        Box::new(WidgetView::new(move || {
            let events = events.clone();
            div()
                .width(120.0)
                .height(60.0)
                .on_mouse_scroll(move |event: MouseEvent| {
                    events.borrow_mut().push(event);
                    EventResponse::PREVENT_DEFAULT
                })
                .into_element()
        }))
    });
    let (mut tree, cache) = build(
        rich_editor(&editor).views(views).id("edit"),
        "textarea{width:300px;height:150px;margin:20px;padding:10px}",
    );
    let edit = id(&tree, "edit");
    let bounds = tree.input_bounds_for_range(edit, 0..0, &cache).unwrap();
    let position = Point::new(bounds.origin.x + 10.0, bounds.origin.y + 15.0);
    // Native wheel routing must work without a click, focus, or a preceding paint.
    tree.dispatch_mouse_move(Some(position), ModifiersState::empty());
    for unit in [WheelUnit::Lines, WheelUnit::Pixels] {
        let wheel = MouseWheel {
            x: 0.5,
            y: -2.0,
            unit,
        };
        let response = tree.dispatch_mouse_scroll(wheel, ModifiersState::SHIFT);
        assert!(response.prevent_default);
        let events = received.borrow();
        let event = events.last().expect("embedded wheel handler was called");
        assert_eq!(event.wheel, wheel);
        assert_eq!(event.modifiers, ModifiersState::SHIFT);
        assert_eq!(event.position, position);
        assert!((event.local_position.x - 10.0).abs() < 0.01);
        assert!((event.local_position.y - 15.0).abs() < 0.01);
        assert_eq!(tree.scroll_metrics(edit).unwrap().offset, Point::default());
        assert!(tree.focused().is_none());
    }
    // Hovering source text must not send wheel events to the embedded view.
    tree.dispatch_mouse_move(
        Some(Point::new(position.x, position.y + 75.0)),
        ModifiersState::empty(),
    );
    tree.dispatch_mouse_scroll(
        MouseWheel {
            x: 0.0,
            y: -20.0,
            unit: WheelUnit::Pixels,
        },
        ModifiersState::empty(),
    );
    assert_eq!(received.borrow().len(), 2);
    assert!(tree.scroll_metrics(edit).unwrap().offset.y > 0.0);
}

#[test]
fn embedded_scroll_defaults_chain_to_editor_only_at_the_boundary() {
    use editing::{EditorViews, Projection, Replacement, ViewId, WidgetView};
    use std::{cell::RefCell, rc::Rc};
    use style::scroll::{Overflow, OverscrollBehavior};

    for overscroll in [OverscrollBehavior::Auto, OverscrollBehavior::Contain] {
        let editor = Editor::new(format!("[view]\n{}", "line\n".repeat(30)));
        editor
            .update(|s| {
                s.set_projection(
                    s.revision(),
                    Projection::new().replace(Replacement::object(ViewId(1), 0..6)),
                )
            })
            .unwrap();
        let positions = Rc::new(RefCell::new(Vec::new()));
        let seen = positions.clone();
        let views = EditorViews::new().register(ViewId(1), move || {
            let seen = seen.clone();
            Box::new(WidgetView::new(move || {
                let seen = seen.clone();
                div()
                    .width(120.0)
                    .height(60.0)
                    .overflow(Overflow::Auto)
                    .overscroll_behavior(overscroll)
                    .child(div().width(100.0).height(200.0).on_mouse_scroll(
                        move |event: MouseEvent| {
                            seen.borrow_mut().push(event.local_position.y);
                        },
                    ))
                    .into_element()
            }))
        });
        let (mut tree, cache) = build(
            rich_editor(&editor).views(views).id("edit"),
            "textarea{width:300px;height:150px;margin:20px;padding:10px}",
        );
        let edit = id(&tree, "edit");
        let bounds = tree.input_bounds_for_range(edit, 0..0, &cache).unwrap();
        let position = Point::new(bounds.origin.x + 10.0, bounds.origin.y + 15.0);
        tree.dispatch_mouse_move(Some(position), ModifiersState::empty());
        let wheel = MouseWheel {
            x: 0.0,
            y: -20.0,
            unit: WheelUnit::Pixels,
        };
        for _ in 0..7 {
            tree.dispatch_mouse_scroll(wheel, ModifiersState::empty());
            assert_eq!(tree.scroll_metrics(edit).unwrap().offset, Point::default());
        }
        // The child handler's local position tracks its content scrolling, so
        // this verifies actual default scrolling, not just callback delivery.
        assert_eq!(
            *positions.borrow(),
            [15.0, 35.0, 55.0, 75.0, 95.0, 115.0, 135.0]
        );
        tree.dispatch_mouse_scroll(wheel, ModifiersState::empty());
        assert_eq!(
            tree.scroll_metrics(edit).unwrap().offset.y,
            if overscroll == OverscrollBehavior::Auto {
                20.0
            } else {
                0.0
            }
        );
        assert!(tree.focused().is_none());
        if overscroll == OverscrollBehavior::Auto {
            // The parent moved the view; the next event must use its new bounds
            // without waiting for a paint to reposition the embedded tree.
            tree.dispatch_mouse_scroll(wheel, ModifiersState::empty());
            assert_eq!(*positions.borrow().last().unwrap(), 175.0);
        }
    }
}

#[test]
fn real_image_can_be_a_source_backed_inline_view() {
    use editing::{EditorViews, Projection, Replacement, ViewId, WidgetView};
    let editor = Editor::new("image=[pixel]");
    let image = media::Image::from_rgba(1, 1, vec![255, 0, 0, 255]).unwrap();
    editor
        .update(|s| {
            s.set_projection(
                s.revision(),
                Projection::new().replace(Replacement::object(ViewId(2), 6..13)),
            )
        })
        .unwrap();
    let views = EditorViews::new().register(ViewId(2), move || {
        let image = image.clone();
        Box::new(WidgetView::new(move || {
            img(image.clone()).width(24.0).height(24.0).into_element()
        }))
    });
    let (mut tree, cache) = build(
        rich_editor(&editor).views(views).id("edit"),
        "textarea{width:240px;height:100px}",
    );
    let scene = paint(&mut tree, &cache);
    assert!(!scene.polychrome_sprites.is_empty());
    assert_eq!(editor.text(), "image=[pixel]");
    assert!(!editor.with(|s| s.can_undo()));
}

#[test]
fn extension_projection_commands_and_lifecycle_are_connected_to_the_control() {
    use editing::{
        EditorExtension, EditorExtensions, ExtensionContext, Projection, Replacement, ViewId,
    };
    use std::cell::Cell;
    use std::rc::Rc;
    struct Extension {
        mounted: Rc<Cell<usize>>,
        unmounted: Rc<Cell<usize>>,
    }
    impl EditorExtension for Extension {
        fn project(&mut self, cx: ExtensionContext<'_>) -> Result<Projection, editing::EditError> {
            use editing::TextRead;
            if cx.snapshot.text.len() >= 4 {
                Ok(Projection::new().replace(Replacement::text(ViewId(3), 0..4, "VIEW")))
            } else {
                Ok(Projection::default())
            }
        }
        fn command(
            &mut self,
            event: &InputEvent,
            cx: ExtensionContext<'_>,
        ) -> Result<Option<Transaction>, editing::EditError> {
            if matches!(
                event,
                InputEvent::Key(KeyInput {
                    key: Key::Named(NamedKey::F2),
                    ..
                })
            ) {
                Ok(Some(Transaction::new(
                    cx.snapshot.revision,
                    [Edit::new(0..4, "EDIT")],
                )))
            } else {
                Ok(None)
            }
        }
        fn mounted(&mut self, _: Option<core::updates::WidgetInvalidator>) {
            self.mounted.set(self.mounted.get() + 1);
        }
        fn unmounted(&mut self) {
            self.unmounted.set(self.unmounted.get() + 1);
        }
    }
    let editor = Editor::new("text tail");
    let mounted = Rc::new(Cell::new(0));
    let unmounted = Rc::new(Cell::new(0));
    let (m, u) = (mounted.clone(), unmounted.clone());
    let extensions = EditorExtensions::new().register(ViewId(100), 0, move || {
        Box::new(Extension {
            mounted: m.clone(),
            unmounted: u.clone(),
        })
    });
    let (mut tree, cache) = build(
        rich_editor(&editor).extensions(extensions).id("edit"),
        "textarea{width:240px}",
    );
    paint(&mut tree, &cache);
    assert_eq!(mounted.get(), 1);
    let edit = id(&tree, "edit");
    tree.dispatch_input(edit, &key(NamedKey::F2, ModifiersState::empty()), &cache);
    assert_eq!(editor.text(), "EDIT tail");
    editor.update(|s| s.undo()).unwrap();
    assert_eq!(editor.text(), "text tail");
    drop(tree);
    assert_eq!(unmounted.get(), 1);
}

/// Match the example's selection-dependent Markdown projection without relying
/// on its parser. The source offsets below are derived from the fixture text.
fn marker_reveal_editor() -> (Editor, WidgetTree, TextLayoutCache, WidgetId, usize) {
    use editing::{
        EditorExtension, EditorExtensions, ExtensionContext, Projection, Replacement, ViewId,
    };
    struct Reveal {
        range: std::ops::Range<usize>,
    }
    impl EditorExtension for Reveal {
        fn project(&mut self, cx: ExtensionContext<'_>) -> Result<Projection, editing::EditError> {
            let mut plan = Projection::new().style(StyleSpan::new(
                self.range.start + 2..self.range.end - 2,
                InlineStyle::new().bold(),
            ));
            if !cx.snapshot.selections.iter().any(|s| {
                s.text_range().start <= self.range.end && s.text_range().end >= self.range.start
            }) {
                plan = plan
                    .replace(Replacement::hide(
                        ViewId(1),
                        self.range.start..self.range.start + 2,
                    ))
                    .replace(Replacement::hide(
                        ViewId(2),
                        self.range.end - 2..self.range.end,
                    ));
            }
            Ok(plan)
        }
    }
    let source = "Click inside **these words** to reveal their markers.";
    let start = source.find("**").unwrap();
    let end = start + 2 + source[start + 2..].find("**").unwrap() + 2;
    let editor = Editor::new(source);
    editor
        .update(|s| s.select(SelectionSet::single(Selection::caret(s.document().len()))))
        .unwrap();
    let extensions = EditorExtensions::new().register(ViewId(100), 0, move || {
        Box::new(Reveal { range: start..end })
    });
    let (mut tree, cache) = build(
        rich_editor(&editor).extensions(extensions).id("edit"),
        "textarea{width:480px;height:120px;caret-animation:manual}",
    );
    let edit = id(&tree, "edit");
    paint(&mut tree, &cache);
    (editor, tree, cache, edit, start + 2)
}
fn pointer(
    phase: PointerPhase,
    position: Point<f32>,
    modifiers: ModifiersState,
    clicks: u8,
) -> InputEvent {
    InputEvent::Pointer {
        phase,
        position,
        modifiers,
        clicks,
    }
}
#[test]
fn clicking_revealed_markdown_does_not_turn_into_a_selection() {
    let (editor, mut tree, cache, edit, start) = marker_reveal_editor();
    let byte = start + 3;
    let old = tree
        .input_bounds_for_range(edit, byte..byte, &cache)
        .unwrap();
    let point = Point::new(old.origin.x, old.origin.y + old.size.height * 0.5);
    tree.dispatch_input(
        edit,
        &pointer(PointerPhase::Down, point, ModifiersState::empty(), 1),
        &cache,
    );
    let down = editor.with(|s| s.selections().primary());
    assert!(down.is_caret());
    assert_eq!(down.head, byte);
    paint(&mut tree, &cache);
    // Native backends can deliver a move notification with unchanged coordinates
    // while the button is down. Display changes alone must not count as a drag.
    tree.dispatch_input(
        edit,
        &pointer(PointerPhase::Move, point, ModifiersState::empty(), 0),
        &cache,
    );
    tree.dispatch_input(
        edit,
        &pointer(PointerPhase::Up, point, ModifiersState::empty(), 1),
        &cache,
    );
    assert_eq!(editor.with(|s| s.selections().primary()), down);
    paint(&mut tree, &cache);
    let revealed = tree
        .input_bounds_for_range(edit, byte..byte, &cache)
        .unwrap();
    assert!(
        revealed.origin.x > old.origin.x,
        "release should reveal the prefix markers"
    );
    assert!(!editor.with(|s| s.can_undo()));
}
#[test]
fn markdown_drag_uses_one_projection_until_release() {
    let (editor, mut tree, cache, edit, start) = marker_reveal_editor();
    let a = tree
        .input_bounds_for_range(edit, start + 1..start + 1, &cache)
        .unwrap();
    let b = tree
        .input_bounds_for_range(edit, start + 7..start + 7, &cache)
        .unwrap();
    let p = Point::new(a.origin.x, a.origin.y + a.size.height * 0.5);
    let q = Point::new(b.origin.x, b.origin.y + b.size.height * 0.5);
    tree.dispatch_input(
        edit,
        &pointer(PointerPhase::Down, p, ModifiersState::empty(), 1),
        &cache,
    );
    paint(&mut tree, &cache);
    tree.dispatch_input(
        edit,
        &pointer(PointerPhase::Move, q, ModifiersState::empty(), 0),
        &cache,
    );
    paint(&mut tree, &cache);
    tree.dispatch_input(
        edit,
        &pointer(PointerPhase::Up, q, ModifiersState::empty(), 1),
        &cache,
    );
    assert_eq!(
        editor.with(|s| s.selections().primary().text_range()),
        start + 1..start + 7
    );
    assert_eq!(editor.with(|s| s.selected_text()), "hese w");
}

/// A source-backed view whose dimensions can reach or exceed the editor viewport.
fn sized_object_views(width_fraction: f32, height: f32) -> editing::EditorViews {
    struct Object {
        width_fraction: f32,
        height: f32,
    }
    impl editing::EmbeddedView for Object {
        fn ignore_event(&self, _: &InputEvent) -> bool {
            false
        }
        fn measure(
            &mut self,
            width: f32,
            _: &TextLayoutCache,
        ) -> render::Result<editing::ViewMetrics> {
            Ok(editing::ViewMetrics::new(
                width * self.width_fraction,
                self.height,
            ))
        }
        fn paint(
            &mut self,
            _: &mut Painter<'_>,
            _: core::geometry::Rect<f32>,
        ) -> render::Result<()> {
            Ok(())
        }
    }
    editing::EditorViews::new().register(editing::ViewId(9), move || {
        Box::new(Object {
            width_fraction,
            height,
        })
    })
}

#[test]
fn selecting_viewport_sized_objects_does_not_scroll_or_turn_a_stationary_move_into_a_drag() {
    use editing::{
        EditorExtension, EditorExtensions, ExtensionContext, Projection, Replacement, ViewId,
    };
    struct Reveal;
    impl EditorExtension for Reveal {
        fn project(&mut self, cx: ExtensionContext<'_>) -> Result<Projection, editing::EditError> {
            let touched = cx.snapshot.selections.iter().any(|s| {
                let range = s.text_range();
                range.start <= 7 && range.end >= 2
            });
            Ok(if touched {
                Projection::new()
            } else {
                Projection::new().replace(Replacement::object(ViewId(9), 2..7))
            })
        }
    }
    for width_fraction in [0.5, 1.0, 1.5] {
        for height in [60.0, 400.0] {
            let editor = Editor::new("a\n$$x$$\nb");
            let (mut tree, cache) = build(
                rich_editor(&editor)
                    .id("edit")
                    .views(sized_object_views(width_fraction, height))
                    .extensions(
                        EditorExtensions::new().register(ViewId(1), 0, || Box::new(Reveal)),
                    ),
                "textarea{width:400px;height:300px}",
            );
            let edit = id(&tree, "edit");
            let point = Point::new(60.0, 50.0);
            for phase in [PointerPhase::Down, PointerPhase::Move, PointerPhase::Up] {
                tree.dispatch_input(
                    edit,
                    &pointer(phase, point, ModifiersState::empty(), 1),
                    &cache,
                );
                tree.layout(
                    Size {
                        width: AvailableSpace::Definite(400.0),
                        height: AvailableSpace::Definite(300.0),
                    },
                    &cache,
                );
                assert_eq!(
                    editor.with(|s| s.selections().primary()),
                    Selection::range(2, 7),
                    "width={width_fraction}, height={height}, phase={phase:?}"
                );
                assert_eq!(
                    tree.scroll_metrics(edit).unwrap().offset,
                    Point::default(),
                    "a pointer selection must not reveal the object's far edge"
                );
            }
        }
    }
}

#[test]
fn full_width_object_end_caret_is_visible_without_extra_scroll_extent() {
    use editing::{Projection, Replacement, ViewId};
    let editor = Editor::new("a\n$$x$$\nb");
    editor
        .update(|s| {
            s.set_projection(
                s.revision(),
                Projection::new().replace(Replacement::object(ViewId(9), 2..7)),
            )
        })
        .unwrap();
    let (mut tree, cache) = build(
        rich_editor(&editor)
            .id("edit")
            .views(sized_object_views(1.0, 60.0)),
        "textarea{width:400px;height:300px;caret-color:red;caret-animation:manual}",
    );
    let edit = id(&tree, "edit");
    tree.set_focused(Some(edit));
    editor
        .update(|s| s.select(SelectionSet::single(Selection::caret(7))))
        .unwrap();
    let scene = paint(&mut tree, &cache);
    tree.refresh_scroll_content(&cache);
    let metrics = tree.scroll_metrics(edit).unwrap();
    assert_eq!(
        metrics.max.x, 0.0,
        "caret ink must not enlarge the content extent"
    );
    assert_eq!(metrics.offset.x, 0.0);
    let caret = tree.input_bounds_for_range(edit, 7..7, &cache).unwrap();
    assert_eq!(caret.origin.x, 399.0);
    assert_eq!(caret.size.width, 1.0);
    assert!(
        scene.quads.iter().any(|quad| {
            quad.bounds.origin.x.0 == caret.origin.x
                && quad.bounds.size.width.0 == caret.size.width
                && quad.bounds.size.height.0 == caret.size.height
        }),
        "the painted caret and IME geometry must agree"
    );
}

#[test]
fn pointer_selection_tracks_real_scrolling_and_dragging_outside_the_viewport() {
    let editor = Editor::new("abcdefghij\n".repeat(40));
    let (mut tree, cache) = build(
        textarea(&editor).id("edit"),
        "textarea{width:200px;height:120px}",
    );
    let edit = id(&tree, "edit");
    let point = Point::new(25.0, 45.0);
    tree.dispatch_input(
        edit,
        &pointer(PointerPhase::Down, point, ModifiersState::empty(), 1),
        &cache,
    );
    let initial = editor.with(|s| s.selections().primary());
    tree.dispatch_mouse_move(Some(point), ModifiersState::empty());
    tree.dispatch_mouse_scroll(
        MouseWheel {
            x: 0.0,
            y: -30.0,
            unit: WheelUnit::Pixels,
        },
        ModifiersState::empty(),
    );
    tree.dispatch_input(
        edit,
        &pointer(PointerPhase::Move, point, ModifiersState::empty(), 1),
        &cache,
    );
    let scrolled = editor.with(|s| s.selections().primary());
    assert_eq!(scrolled.anchor, initial.anchor);
    assert!(
        scrolled.head > initial.head,
        "scrolling under a stationary pointer must extend the selection"
    );
    assert_eq!(tree.scroll_metrics(edit).unwrap().offset.y, 30.0);
    tree.dispatch_input(
        edit,
        &pointer(PointerPhase::Move, point, ModifiersState::empty(), 1),
        &cache,
    );
    assert_eq!(editor.with(|s| s.selections().primary()), scrolled);

    // Out-of-bounds motion scrolls toward the pointer, not toward a possibly
    // distant source endpoint. Repeated events continue dragging through content.
    let outside = Point::new(25.0, 140.0);
    for expected in [50.0, 70.0] {
        tree.dispatch_input(
            edit,
            &pointer(PointerPhase::Move, outside, ModifiersState::empty(), 1),
            &cache,
        );
        assert_eq!(tree.scroll_metrics(edit).unwrap().offset.y, expected);
        assert_eq!(
            editor.with(|s| s.selections().primary().anchor),
            initial.anchor
        );
    }
    tree.dispatch_input(
        edit,
        &pointer(PointerPhase::Up, outside, ModifiersState::empty(), 1),
        &cache,
    );
    assert_eq!(tree.scroll_metrics(edit).unwrap().offset.y, 70.0);
}

#[test]
fn unwrapped_keyboard_carets_share_visible_bounds_with_ime_and_completion() {
    use std::{cell::Cell, rc::Rc};
    for multiline in [false, true] {
        for align in ["left", "right"] {
            let editor = Editor::new("x".repeat(80));
            let completion = Rc::new(Cell::new(None));
            let captured = completion.clone();
            let control = if multiline {
                textarea(&editor)
            } else {
                input(&editor)
            };
            let (mut tree, cache) = build(
                control.id("edit").on_key(move |key, cx| {
                    if key.key == Key::Named(NamedKey::F2) {
                        captured.set(cx.caret_bounds());
                        true
                    } else {
                        false
                    }
                }),
                &format!(
                    "input,textarea{{width:120px;height:60px;text-wrap-mode:nowrap;text-align:{align};caret-animation:manual}}"
                ),
            );
            let edit = id(&tree, "edit");
            tree.set_focused(Some(edit));
            tree.dispatch_input(edit, &key(NamedKey::End, ModifiersState::empty()), &cache);
            tree.dispatch_input(edit, &InputEvent::Text("x".into()), &cache);
            let end = editor.text().len();
            assert_eq!(editor.with(|s| s.selections().primary().head), end);
            let scene = paint(&mut tree, &cache);
            let metrics = tree.scroll_metrics(edit).unwrap();
            assert!(metrics.offset.x > 0.0);
            assert!((metrics.offset.x - metrics.max.x).abs() < 0.01);
            let caret = tree.input_bounds_for_range(edit, end..end, &cache).unwrap();
            let bounds = tree.content_bounds(edit);
            assert!(
                (caret.origin.x + caret.size.width - bounds.origin.x - bounds.size.width).abs()
                    < 0.01
            );
            assert!(
                scene
                    .quads
                    .iter()
                    .any(|quad| quad.bounds.origin.x.0 == caret.origin.x
                        && quad.bounds.size.width.0 == caret.size.width)
            );
            tree.dispatch_input(edit, &key(NamedKey::F2, ModifiersState::empty()), &cache);
            assert_eq!(completion.get(), Some(caret));
            let offscreen = tree.input_bounds_for_range(edit, 0..0, &cache).unwrap();
            assert!(
                offscreen.origin.x + offscreen.size.width < bounds.origin.x,
                "offscreen carets must not be clamped onto the viewport edge"
            );
            tree.dispatch_input(edit, &key(NamedKey::Home, ModifiersState::empty()), &cache);
            assert_eq!(tree.scroll_metrics(edit).unwrap().offset.x, 0.0);
        }
    }
}

#[test]
fn right_aligned_short_text_keeps_its_end_caret_inside_the_viewport() {
    let editor = Editor::new("short");
    let (mut tree, cache) = build(
        input(&editor).id("edit"),
        "input{width:200px;text-align:right;caret-animation:manual}",
    );
    let edit = id(&tree, "edit");
    tree.set_focused(Some(edit));
    tree.dispatch_input(edit, &key(NamedKey::End, ModifiersState::empty()), &cache);
    let scene = paint(&mut tree, &cache);
    let caret = tree.input_bounds_for_range(edit, 5..5, &cache).unwrap();
    assert_eq!(tree.scroll_metrics(edit).unwrap().offset.x, 0.0);
    assert_eq!(tree.scroll_metrics(edit).unwrap().max.x, 0.0);
    assert_eq!(caret.origin.x + caret.size.width, 200.0);
    assert!(
        scene
            .quads
            .iter()
            .any(|quad| quad.bounds.origin.x.0 == caret.origin.x
                && quad.bounds.size.width.0 == caret.size.width)
    );
}

#[test]
fn marker_reveal_gestures_preserve_shift_and_double_click_selection() {
    for (modifiers, clicks) in [(ModifiersState::SHIFT, 1), (ModifiersState::empty(), 2)] {
        let (editor, mut tree, cache, edit, start) = marker_reveal_editor();
        let bounds = tree
            .input_bounds_for_range(edit, start + 3..start + 3, &cache)
            .unwrap();
        let point = Point::new(bounds.origin.x, bounds.origin.y + bounds.size.height * 0.5);
        tree.dispatch_input(
            edit,
            &pointer(PointerPhase::Down, point, modifiers, clicks),
            &cache,
        );
        let selected = editor.with(|s| s.selections().clone());
        assert!(!selected.primary().is_caret());
        paint(&mut tree, &cache);
        tree.dispatch_input(
            edit,
            &pointer(PointerPhase::Move, point, modifiers, 0),
            &cache,
        );
        tree.dispatch_input(
            edit,
            &pointer(PointerPhase::Up, point, modifiers, clicks),
            &cache,
        );
        assert_eq!(editor.with(|s| s.selections().clone()), selected);
    }
}
#[test]
fn projection_gestures_release_on_cancel_focus_loss_and_document_edits() {
    for finish in 0..3 {
        let (editor, mut tree, cache, edit, start) = marker_reveal_editor();
        tree.set_focused(Some(edit));
        let byte = start + 3;
        let old = tree
            .input_bounds_for_range(edit, byte..byte, &cache)
            .unwrap();
        let point = Point::new(old.origin.x, old.origin.y + old.size.height * 0.5);
        tree.dispatch_input(
            edit,
            &pointer(PointerPhase::Down, point, ModifiersState::empty(), 1),
            &cache,
        );
        paint(&mut tree, &cache);
        assert_eq!(
            tree.input_bounds_for_range(edit, byte..byte, &cache)
                .unwrap(),
            old
        );
        match finish {
            0 => {
                tree.dispatch_input(
                    edit,
                    &pointer(PointerPhase::Cancel, point, ModifiersState::empty(), 0),
                    &cache,
                );
            }
            1 => {
                tree.set_focused(None);
            }
            _ => {
                editor
                    .update(|s| s.transact(Transaction::new(s.revision(), [Edit::new(0..0, "x")])))
                    .unwrap();
            }
        }
        let selection = editor.with(|s| s.selections().clone());
        tree.dispatch_input(
            edit,
            &pointer(PointerPhase::Move, point, ModifiersState::empty(), 0),
            &cache,
        );
        assert_eq!(editor.with(|s| s.selections().clone()), selection);
        let head = selection.primary().head;
        assert!(
            tree.input_bounds_for_range(edit, head..head, &cache)
                .unwrap()
                .origin
                .x
                > old.origin.x
        );
    }
}
#[test]
fn keyboard_selection_reveals_markers_without_waiting_for_a_pointer_release() {
    let (editor, mut tree, cache, edit, start) = marker_reveal_editor();
    let byte = start + 3;
    let old = tree
        .input_bounds_for_range(edit, byte..byte, &cache)
        .unwrap();
    editor
        .update(|s| s.select(SelectionSet::single(Selection::caret(byte))))
        .unwrap();
    paint(&mut tree, &cache);
    assert!(
        tree.input_bounds_for_range(edit, byte..byte, &cache)
            .unwrap()
            .origin
            .x
            > old.origin.x
    );
}

#[test]
fn unfocused_editor_scroll_wakes_after_svg_and_hover_repaint() {
    use std::{cell::Cell, rc::Rc};

    let editor = Editor::new("short line\n".repeat(200));
    let (mut tree, cache) = build(
        div().child(textarea(&editor).id("edit")).child(
            svg_from_str(r#"<svg width="20" height="20"><rect width="20" height="20"/></svg>"#)
                .unwrap(),
        ),
        "textarea{width:300px;height:120px} textarea:hover{background:#eee}",
    );
    let edit = id(&tree, "edit");
    paint(&mut tree, &cache);
    let wakes = Rc::new(Cell::new(0));
    let count = wakes.clone();
    tree.set_update_waker(move || count.set(count.get() + 1));
    let wheel = MouseWheel {
        x: 0.0,
        y: -30.0,
        unit: WheelUnit::Pixels,
    };

    // A file load or wheel update can share a frame with a hover change. SVG
    // icons force that CSS frame to repaint even when their own style is stable.
    assert!(tree.pointer_moved(Some(Point::new(40.0, 40.0))));
    tree.dispatch_mouse_scroll(wheel, ModifiersState::empty());
    assert_eq!(wakes.get(), 1);
    let changes = tree.update_styles(Instant::now());
    assert!(changes.paint);
    assert!(!changes.layout);
    tree.refresh_scroll_content(&cache);

    // No focus, caret timer, pointer exit, or extra style pass may rescue this
    // wakeup. The previous frame must have consumed its paint invalidation.
    assert!(tree.focused().is_none());
    assert!(
        tree.text_input(edit)
            .unwrap()
            .next_frame(Instant::now())
            .is_none()
    );
    tree.dispatch_mouse_scroll(wheel, ModifiersState::empty());
    assert_eq!(tree.scroll_metrics(edit).unwrap().offset.y, 60.0);
    assert_eq!(
        wakes.get(),
        2,
        "scroll changed the offset without waking the host"
    );
    let changes = tree.update_styles(Instant::now());
    assert!(changes.paint);
    assert!(!changes.layout);
    assert!(!tree.update_styles(Instant::now()).paint);
}

#[test]
fn resizing_editor_preserves_manual_scroll_instead_of_revealing_the_caret() {
    let editor = Editor::new("short line\n".repeat(200));
    let (mut tree, cache) = build(
        textarea(&editor).id("edit"),
        "textarea{width:300px;height:120px;caret-animation:manual}",
    );
    let edit = id(&tree, "edit");
    tree.set_focused(Some(edit));
    paint(&mut tree, &cache);
    assert!(tree.scroll_to(edit, Point::new(0.0, 900.0)));
    paint(&mut tree, &cache);
    for (width, height) in [(220.0, 120.0), (360.0, 180.0), (240.0, 90.0)] {
        tree.style_mut(edit).layout.size.width = core::layout::Dimension::length(width).into();
        tree.style_mut(edit).layout.size.height = core::layout::Dimension::length(height).into();
        paint(&mut tree, &cache);
        assert_eq!(editor.with(|s| s.selections().primary().head), 0);
        assert_eq!(
            tree.scroll_metrics(edit).unwrap().offset.y,
            900.0,
            "layout changes must not request caret reveal"
        );
        let caret = tree.input_bounds_for_range(edit, 0..0, &cache).unwrap();
        assert!(caret.origin.y < tree.content_bounds(edit).origin.y);
    }
    // Real navigation must still reveal the caret, even if Home changes neither
    // its byte position nor the editor generation at the start of the document.
    tree.dispatch_input(edit, &key(NamedKey::Home, ModifiersState::empty()), &cache);
    paint(&mut tree, &cache);
    let caret = tree.input_bounds_for_range(edit, 0..0, &cache).unwrap();
    assert!(caret.origin.y >= tree.content_bounds(edit).origin.y);
}

#[test]
fn window_width_changes_do_not_seek_an_offscreen_end_caret() {
    let editor = Editor::new("line\n".repeat(200));
    editor
        .update(|s| s.select(SelectionSet::single(Selection::caret(s.document().len()))))
        .unwrap();
    let (mut tree, cache) = build(
        textarea(&editor)
            .id("edit")
            .width(style::pct(100.0))
            .height(120.0),
        "",
    );
    let edit = id(&tree, "edit");
    paint(&mut tree, &cache);
    assert!(tree.scroll_to(edit, Point::new(0.0, 600.0)));
    paint(&mut tree, &cache);
    for width in [320.0, 640.0, 280.0] {
        tree.layout(
            Size {
                width: AvailableSpace::Definite(width),
                height: AvailableSpace::Definite(500.0),
            },
            &cache,
        );
        paint(&mut tree, &cache);
        assert_eq!(tree.scroll_metrics(edit).unwrap().offset.y, 600.0);
        let head = editor.with(|s| s.selections().primary().head);
        let caret = tree
            .input_bounds_for_range(edit, head..head, &cache)
            .unwrap();
        let bounds = tree.content_bounds(edit);
        assert!(caret.origin.y > bounds.origin.y + bounds.size.height);
    }
    // Resize must not suppress the next genuine edit's reveal request.
    tree.dispatch_input(edit, &InputEvent::Text("x".into()), &cache);
    paint(&mut tree, &cache);
    let head = editor.with(|s| s.selections().primary().head);
    let caret = tree
        .input_bounds_for_range(edit, head..head, &cache)
        .unwrap();
    let bounds = tree.content_bounds(edit);
    assert!(caret.origin.y >= bounds.origin.y);
    assert!(caret.origin.y + caret.size.height <= bounds.origin.y + bounds.size.height + 0.01);
}

#[test]
fn typography_reflow_preserves_manual_scroll_and_ime_still_reveals() {
    let editor = Editor::new("line\n".repeat(200));
    let (mut tree, cache) = build(
        textarea(&editor).id("edit"),
        "textarea{width:300px;height:120px}",
    );
    let edit = id(&tree, "edit");
    paint(&mut tree, &cache);
    assert!(tree.scroll_to(edit, Point::new(0.0, 600.0)));
    paint(&mut tree, &cache);
    tree.set_stylesheets(vec![Stylesheet::parse("*{font-family:'IBM Plex Sans';font-size:22px;line-height:30px}textarea{width:240px;height:150px}").unwrap()]);
    paint(&mut tree, &cache);
    assert_eq!(tree.scroll_metrics(edit).unwrap().offset.y, 600.0);
    tree.dispatch_input(
        edit,
        &InputEvent::Preedit("ni".into(), Some((2, 2))),
        &cache,
    );
    paint(&mut tree, &cache);
    let caret = tree.input_bounds_for_range(edit, 2..2, &cache).unwrap();
    assert!(caret.origin.y >= tree.content_bounds(edit).origin.y);
    assert_eq!(editor.with(|s| s.document().revision()), 0);
}

#[test]
fn transformed_editor_pointer_drag_and_ime_rectangles_share_coordinates() {
    let editor = Editor::new("abcdef");
    let (mut tree, cache) = build(
        input(&editor).id("edit"),
        "input {width:200px;transform:translate(100px,60px) scale(1.5);transform-origin:0 0}",
    );
    let edit = id(&tree, "edit");
    let a = tree.input_bounds_for_range(edit, 1..1, &cache).unwrap();
    let b = tree.input_bounds_for_range(edit, 4..4, &cache).unwrap();
    assert!(a.origin.x >= 100. && a.origin.y >= 60.);
    for (phase, rect) in [
        (PointerPhase::Down, a),
        (PointerPhase::Move, b),
        (PointerPhase::Up, b),
    ] {
        tree.dispatch_input(
            edit,
            &InputEvent::Pointer {
                phase,
                position: Point::new(rect.origin.x, rect.origin.y + rect.size.height * 0.5),
                modifiers: ModifiersState::empty(),
                clicks: 1,
            },
            &cache,
        );
    }
    assert_eq!(tree.text_input(edit).unwrap().selected_text(), "bcd");
}

#[test]
fn described_widget_props_reconcile_without_losing_embedded_input_focus() {
    use editing::{Projection, Replacement, WidgetView};
    #[derive(Clone)]
    struct Props {
        editor: Editor,
        width: f32,
    }
    impl PartialEq for Props {
        fn eq(&self, other: &Self) -> bool {
            self.editor.same_session(&other.editor) && self.width == other.width
        }
    }
    fn render_input(props: &Props) -> Element {
        input(&props.editor)
            .font("IBM Plex Sans")
            .width(props.width)
            .height(35.0)
            .into_element()
    }
    let outer = Editor::new("a [control] z");
    let inner = Editor::new("inner");
    let publish = |width| {
        outer
            .update(|s| {
                s.set_projection(
                    s.revision(),
                    Projection::new().replace(Replacement::widget(
                        2..11,
                        WidgetView::describe(
                            Props {
                                editor: inner.clone(),
                                width,
                            },
                            render_input,
                        ),
                    )),
                )
            })
            .unwrap();
    };
    publish(100.0);
    let (mut tree, cache) = build(
        rich_editor(&outer).id("edit"),
        "textarea{width:350px;height:150px;caret-animation:manual}",
    );
    paint(&mut tree, &cache);
    let edit = id(&tree, "edit");
    let bounds = tree.input_bounds_for_range(edit, 2..2, &cache).unwrap();
    let position = Point::new(bounds.origin.x + 10.0, bounds.origin.y + 15.0);
    for phase in [PointerPhase::Down, PointerPhase::Up] {
        tree.dispatch_input(
            edit,
            &pointer(phase, position, ModifiersState::empty(), 1),
            &cache,
        );
    }
    tree.dispatch_input(edit, &InputEvent::Text("X".into()), &cache);
    assert!(inner.text().contains('X'));
    publish(180.0);
    paint(&mut tree, &cache);
    // No new pointer activation: both host focus and subtree focus must survive.
    tree.dispatch_input(edit, &InputEvent::Text("Y".into()), &cache);
    assert!(inner.text().contains('Y'));
    assert_eq!(outer.text(), "a [control] z");
    tree.dispatch_input(
        edit,
        &InputEvent::Preedit("中文".into(), Some((6, 6))),
        &cache,
    );
    assert!(tree.text_input(edit).unwrap().marked_range().is_some());
    assert!(tree.input_bounds_for_range(edit, 1..1, &cache).is_some());
}

#[test]
fn extensions_can_use_the_same_widget_key_without_sharing_instances() {
    use editing::{
        EditorExtension, EditorExtensions, ExtensionContext, Projection, Replacement, ViewId,
        WidgetView,
    };
    struct Extension {
        range: std::ops::Range<usize>,
        label: &'static str,
    }
    impl EditorExtension for Extension {
        fn project(&mut self, _: ExtensionContext<'_>) -> Result<Projection, editing::EditError> {
            Ok(Projection::new().replace(
                Replacement::widget(
                    self.range.clone(),
                    WidgetView::describe(self.label, |label| {
                        text(*label).width(80.0).height(40.0).into_element()
                    }),
                )
                .key("preview"),
            ))
        }
    }
    let editor = Editor::new("a b");
    let extensions = EditorExtensions::new()
        .register(ViewId(1), 0, || {
            Box::new(Extension {
                range: 0..1,
                label: "first",
            })
        })
        .register(ViewId(2), 1, || {
            Box::new(Extension {
                range: 2..3,
                label: "second",
            })
        });
    let (mut tree, cache) = build(
        rich_editor(&editor).extensions(extensions).id("edit"),
        "textarea{width:300px;height:150px}",
    );
    paint(&mut tree, &cache);
    let edit = id(&tree, "edit");
    let first = tree.input_bounds_for_range(edit, 0..0, &cache).unwrap();
    let last = tree.input_bounds_for_range(edit, 3..3, &cache).unwrap();
    // A rejected projection would leave three plain characters, not two widgets.
    assert!(last.origin.x - first.origin.x >= 160.0);
}

#[test]
fn placeholder_uses_inherited_variables_and_relative_font_size() {
    let editor = Editor::default();
    let (mut tree, cache) = build(
        div().child(input(&editor).placeholder("Search")),
        ":root{--ink:green} .changed{--ink:red} input::placeholder{color:var(--ink);font-size:0.7em}",
    );
    let hsla = |css: &str| -> render::Hsla { css.parse::<style::color::Color>().unwrap().into() };
    assert!(
        paint(&mut tree, &cache)
            .monochrome_sprites
            .iter()
            .any(|g| g.color == hsla("green"))
    );
    tree.set_classes(tree.root().unwrap(), "changed");
    assert!(tree.update_styles(Instant::now()).paint);
    assert!(
        paint(&mut tree, &cache)
            .monochrome_sprites
            .iter()
            .any(|g| g.color == hsla("red"))
    );
}
