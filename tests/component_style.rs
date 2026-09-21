//! Component styles reach the real root without changing logical state ownership.
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
    time::Instant,
};
use voidui::{
    Children, IntoElement, State, component,
    core::{
        layout::{AvailableSpace, Size},
        widget_tree::WidgetTree,
    },
    div,
    render::{ParleyTextSystem, TextLayoutCache, TextSystem},
    state,
    style::{color::Rgba8, css::Stylesheet, selection::Cursor, tailwind},
    text,
};
use winit::window::CursorIcon;

type Slot = Rc<RefCell<Option<State<usize>>>>;
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
            height: AvailableSpace::Definite(300.0),
        },
        &cache,
    );
}
fn build(view: impl IntoElement) -> WidgetTree {
    let mut tree = WidgetTree::new();
    tree.build_root(view);
    layout(&mut tree);
    tree
}
fn padding(tree: &WidgetTree, expected: [f32; 4]) {
    let value = &tree.layout_style(tree.root().unwrap()).padding;
    for (actual, expected) in [value.top, value.right, value.bottom, value.left]
        .into_iter()
        .zip(expected)
    {
        assert_eq!(actual, taffy::LengthPercentage::length(expected));
    }
}

#[component]
fn surface(children: Children, #[prop(default = 5.0)] padding: f32) {
    div()
        .id("surface-root")
        .padding(padding)
        .h(100)
        .child(text(format!("{padding}")).id("input-value"))
        .children(children)
}
#[component]
fn label() {
    text("Label").id("label-root")
}
#[component(memo)]
fn counter(renders: Rc<Cell<usize>>, slot: Slot) {
    renders.set(renders.get() + 1);
    let value = state(|| 0);
    *slot.borrow_mut() = Some(value);
    div()
        .id("counter-root")
        .p(2)
        .bg_white()
        .child(text(value.get().to_string()).id("counter-value"))
}
#[component(memo)]
fn transparent(children: Children) {
    children.single()
}

#[test]
fn named_builders_interleave_styles_inputs_children_and_events() {
    let clicks = Rc::new(Cell::new(0));
    let call = clicks.clone();
    let mut tree = build(
        surface()
            .w_full()
            .p_4()
            .padding(9.0)
            .border_r(3)
            .border_l_2()
            .cursor_col_resize()
            .class("surface")
            .child(label().text_sm())
            .bg_gray_50()
            .on_click(move || call.set(call.get() + 1))
            .child("Tail"),
    );
    let root = tree.root().unwrap();
    assert_eq!(tree.find_by_id("surface-root"), Some(root));
    assert_eq!(tree.children(root).len(), 3);
    assert_eq!(
        tree.layout_style(root).size.width,
        taffy::Dimension::percent(1.0)
    );
    padding(&tree, [16.0; 4]);
    assert_eq!(
        tree.text_content(tree.find_by_id("input-value").unwrap()),
        Some("9")
    );
    assert_eq!(
        tree.layout_style(root).border.right,
        taffy::LengthPercentage::length(3.0)
    );
    assert_eq!(
        tree.layout_style(root).border.left,
        taffy::LengthPercentage::length(2.0)
    );
    assert_eq!(
        tree.selection_style(root).cursor,
        Cursor::Icon(CursorIcon::ColResize)
    );
    tree.click(root);
    assert_eq!(clicks.get(), 1);
}

#[test]
fn shadowed_inputs_keep_their_api_and_build_exposes_root_styles() {
    let tree = build(
        surface()
            .padding(11.0)
            .p_4()
            .child("child")
            .build()
            .padding(7),
    );
    padding(&tree, [7.0; 4]);
    assert_eq!(
        tree.text_content(tree.find_by_id("input-value").unwrap()),
        Some("11")
    );
    let tree = build(surface().p(3).padding(12.0));
    padding(&tree, [3.0; 4]);
}

#[test]
fn positional_and_closure_components_share_all_style_groups() {
    let renders = Rc::new(Cell::new(0));
    let count = renders.clone();
    let view = component(move || {
        count.set(count.get() + 1);
        div().p(6)
    })
    .width(120)
    .p_4()
    .pl(7)
    .border_r_1()
    .bg_gray_50()
    .text_sm()
    .opacity(0.75)
    .overflow_y_auto()
    .object_fit(voidui::style::media::ObjectFit::Contain);
    assert_eq!(renders.get(), 0, "style calls must not render the body");
    let tree = build(view);
    assert_eq!(renders.get(), 1);
    // text_sm sets this root's font size to 14px, so its rem-based padding is 14px.
    padding(&tree, [14.0, 14.0, 14.0, 7.0]);
    assert_eq!(tree.text_style(tree.root().unwrap()).font_size, 14.0);
    let tree = build(label().w_16().p_0().font_bold().cursor_col_resize());
    assert_eq!(tree.root(), tree.find_by_id("label-root"));
    assert!(tree.children(tree.root().unwrap()).is_empty());
    assert_eq!(
        tree.layout_style(tree.root().unwrap()).size.width,
        taffy::Dimension::length(64.0)
    );
}

#[test]
fn memo_observes_changed_and_removed_styles_without_resetting_state() {
    let renders = Rc::new(Cell::new(0));
    let slot: Slot = Default::default();
    let make = || counter(renders.clone(), slot.clone());
    let mut tree = build(make().p_4().bg_gray_50());
    let root = tree.root().unwrap();
    tree.reconcile_root(make().p_4().bg_gray_50());
    assert_eq!(renders.get(), 1, "equal overlays must preserve memo skips");
    slot.borrow().unwrap().set(8);
    assert_eq!(tree.flush_updates(), 1);
    layout(&mut tree);
    padding(&tree, [16.0; 4]);
    assert_eq!(
        tree.text_content(tree.find_by_id("counter-value").unwrap()),
        Some("8")
    );
    tree.reconcile_root(make().p_0().bg_transparent());
    layout(&mut tree);
    padding(&tree, [0.0; 4]);
    assert_eq!(
        tree.paint_style(root).background,
        Rgba8::new(0, 0, 0, 0).into()
    );
    tree.reconcile_root(make());
    layout(&mut tree);
    padding(&tree, [2.0; 4]);
    assert_eq!(
        tree.paint_style(root).background,
        Rgba8::from_rgb8(255, 255, 255).into()
    );
    assert_eq!(tree.root(), Some(root));
    assert_eq!(tree.state_count(), 1);
    assert_eq!(slot.borrow().unwrap().get(), 8);
    assert_eq!(renders.get(), 4);
}

#[test]
fn nested_overlays_survive_independent_inner_updates_and_can_be_removed() {
    let renders = Rc::new(Cell::new(0));
    let slot: Slot = Default::default();
    let children = Children::new().child(counter(renders.clone(), slot.clone()).p_4().pr(5));
    let make = || transparent().children(children.clone());
    let mut tree = build(make().pl(3).border_r_1());
    let root = tree.root().unwrap();
    assert_eq!(tree.root(), tree.find_by_id("counter-root"));
    assert_eq!(tree.component_count(), 2);
    padding(&tree, [16.0, 5.0, 16.0, 3.0]);
    slot.borrow().unwrap().set(4);
    assert_eq!(tree.flush_updates(), 1);
    layout(&mut tree);
    padding(&tree, [16.0, 5.0, 16.0, 3.0]);
    tree.reconcile_root(make());
    layout(&mut tree);
    padding(&tree, [16.0, 5.0, 16.0, 16.0]);
    assert_eq!(
        tree.layout_style(root).border.right,
        taffy::LengthPercentage::length(0.0)
    );
    assert_eq!(tree.root(), Some(root));
    assert_eq!(slot.borrow().unwrap().get(), 4);
}

#[test]
fn css_important_theme_values_and_overlay_removal_use_normal_cascade() {
    let mut tree = build(surface().p_4().pl(3).bg_gray_50());
    tree.set_stylesheets(vec![Stylesheet::parse(
        ":root {font-size:20px; --spacing:0.5rem; --color-gray-50:#123456; padding-left:12px !important;}"
    ).unwrap()]);
    layout(&mut tree);
    padding(&tree, [40.0, 40.0, 40.0, 12.0]);
    assert_eq!(
        tree.paint_style(tree.root().unwrap()).background,
        Rgba8::from_hex_rgb(0x123456).into()
    );
    tree.set_stylesheets(vec![tailwind::stylesheet("p-4").unwrap()]);
    tree.reconcile_root(surface().class("p-4").build().padding(0));
    layout(&mut tree);
    padding(&tree, [0.0; 4]);
}

#[test]
fn cloned_component_overlays_are_independent() {
    let shared = label().w_16().bg_white();
    let tree = build(
        div()
            .child(shared.clone().w_8().id("small"))
            .child(shared.clone().id("large")),
    );
    assert_eq!(
        tree.layout_style(tree.find_by_id("small").unwrap())
            .size
            .width,
        taffy::Dimension::length(32.0)
    );
    assert_eq!(
        tree.layout_style(tree.find_by_id("large").unwrap())
            .size
            .width,
        taffy::Dimension::length(64.0)
    );
}

#[test]
fn unchanged_styled_output_does_not_invalidate_css_or_layout() {
    let slot: Slot = Default::default();
    let captured = slot.clone();
    let view = component(move || {
        let value = state(|| 0);
        value.with(|_| ());
        *captured.borrow_mut() = Some(value);
        component(|| div().p(2)).pl(4)
    })
    .p_4()
    .border_r_1();
    let mut tree = build(view);
    let before = tree.cascade_stats().passes;
    slot.borrow().unwrap().set(1);
    tree.flush_updates();
    let changes = tree.update_styles(Instant::now());
    assert!(!changes.layout);
    assert!(!changes.paint);
    assert_eq!(tree.cascade_stats().passes, before);
}

mod public_components {
    use voidui::{Children, component, div};

    #[component]
    pub fn generic_panel<const N: usize>(#[prop(default = true)] w_full: bool, children: Children) {
        div()
            .width(if w_full { N as f32 } else { 0.0 })
            .children(children)
    }
}

#[test]
fn public_generic_builders_keep_style_named_inputs_and_children() {
    let tree = build(
        public_components::generic_panel::<80>()
            .w_full(false)
            .p_4()
            .child("content")
            .build()
            .w_full(),
    );
    padding(&tree, [16.0; 4]);
    assert_eq!(
        tree.layout_style(tree.root().unwrap()).size.width,
        taffy::Dimension::percent(1.0)
    );
    let tree = build(
        public_components::generic_panel::<80>()
            .w_16()
            .w_full(false)
            .child("content"),
    );
    assert_eq!(
        tree.layout_style(tree.root().unwrap()).size.width,
        taffy::Dimension::length(64.0)
    );
}

#[test]
fn root_kind_changes_keep_the_invocation_overlay_and_hooks() {
    let slot: Slot = Default::default();
    let captured = slot.clone();
    let mut tree = build(
        component(move || {
            let value = state(|| 0);
            *captured.borrow_mut() = Some(value);
            if value.get() == 0 {
                div().p(2).into_element()
            } else {
                text("New root").p(6).into_element()
            }
        })
        .p_4()
        .cursor_col_resize(),
    );
    let old = tree.root().unwrap();
    slot.borrow().unwrap().set(1);
    tree.flush_updates();
    layout(&mut tree);
    let root = tree.root().unwrap();
    assert_ne!(root, old);
    assert_eq!(tree.text_content(root), Some("New root"));
    padding(&tree, [16.0; 4]);
    assert_eq!(
        tree.selection_style(root).cursor,
        Cursor::Icon(CursorIcon::ColResize)
    );
    assert_eq!(tree.component_count(), 1);
    assert_eq!(tree.state_count(), 1);
}
