//! Element constructors exercise CSS, focus, activation, and retained identity
//! through the headless runtime, without opening a native window.
use std::{cell::Cell, rc::Rc, sync::Arc};
use voidui::{
    IntoElement, button,
    core::{
        geometry::Point,
        layout::{AvailableSpace, Display, Size},
        widget::{Widget, WidgetBuilder, WidgetId, WidgetUpdate},
        widget_tree::WidgetTree,
    },
    dialog, div,
    render::{ParleyTextSystem, TextLayoutCache, TextSystem},
    style::{css::Stylesheet, selection::UserSelect},
    widgets::{Button, Container, Dialog, Div},
};

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
            height: AvailableSpace::Definite(300.0),
        },
        &cache(),
    );
}

fn build(view: impl IntoElement, css: Option<&str>) -> WidgetTree {
    let mut tree = WidgetTree::new();
    tree.build_root(view);
    if let Some(css) = css {
        tree.set_stylesheets(vec![Stylesheet::parse(css).unwrap()]);
    }
    layout(&mut tree);
    tree
}

fn id(tree: &WidgetTree, name: &str) -> WidgetId {
    tree.find_by_id(name).unwrap()
}

#[test]
fn constructors_keep_concrete_types_through_children_styles_and_events() {
    let _: WidgetBuilder<Button> = button()
        .child(div())
        .children([div(), div()])
        .on_click(|| {})
        .flex()
        .when(true, |view| view.padding(8));
    let _: WidgetBuilder<Dialog> = dialog()
        .on_click(|| {})
        .child(button())
        .children([button()])
        .width(100);
    let _: WidgetBuilder<Div> = div().child(button()).children([dialog()]);
}

#[test]
fn button_defaults_and_css_type_selectors_need_no_explicit_name() {
    // Test both cascade paths: intrinsic defaults also apply without a stylesheet.
    for css in [None, Some("button { width: 80px; } div { width: 40px; }")] {
        let tree = build(
            div().child(button().id("action")).child(div().id("plain")),
            css,
        );
        assert_eq!(
            tree.selection_style(id(&tree, "action")).user_select,
            UserSelect::None
        );
        assert_eq!(
            tree.selection_style(id(&tree, "plain")).user_select,
            UserSelect::Auto
        );
        if css.is_some() {
            assert_eq!(tree.bounds(id(&tree, "action")).size.width, 80.0);
            assert_eq!(tree.bounds(id(&tree, "plain")).size.width, 40.0);
        }
    }
    let tree = build(button(), Some("button { user-select: text; }"));
    assert_eq!(
        tree.selection_style(tree.root().unwrap()).user_select,
        UserSelect::Text
    );
}

#[test]
fn buttons_focus_from_descendant_clicks_and_follow_tab_order() {
    let calls = Rc::new(Cell::new(0));
    let output = calls.clone();
    let mut tree = build(
        div()
            .child(
                button()
                    .id("first")
                    .size(80, 30)
                    .child(div().size(20, 10))
                    .on_click(move || output.set(output.get() + 1)),
            )
            .child(div().id("plain").size(80, 30).on_click(|| {}))
            .child(button().id("disabled").attr("disabled", "").size(80, 30))
            .child(
                button()
                    .id("pointer-only")
                    .attr("tabindex", "-1")
                    .size(80, 30),
            )
            .child(button().id("second").size(80, 30)),
        None,
    );
    let first = id(&tree, "first");
    let second = id(&tree, "second");
    tree.pointer_moved(Some(Point::new(5.0, 5.0)));
    tree.pointer_pressed(true);
    assert_eq!(tree.focused(), Some(first));
    tree.pointer_pressed(false);
    assert_eq!(calls.get(), 1);
    assert!(tree.focus_next(false));
    assert_eq!(tree.focused(), Some(second));
    assert!(tree.focus_next(true));
    assert_eq!(tree.focused(), Some(first));

    let plain = tree.bounds(id(&tree, "plain"));
    tree.pointer_moved(Some(Point::new(plain.origin.x + 5.0, plain.origin.y + 5.0)));
    tree.pointer_pressed(true);
    assert_eq!(tree.focused(), None);
    tree.pointer_pressed(false);

    let pointer_only = tree.bounds(id(&tree, "pointer-only"));
    tree.pointer_moved(Some(Point::new(
        pointer_only.origin.x + 5.0,
        pointer_only.origin.y + 5.0,
    )));
    tree.pointer_pressed(true);
    assert_eq!(tree.focused(), Some(id(&tree, "pointer-only")));
}

#[test]
fn button_updates_retain_focus_handlers_and_keyed_identity() {
    let calls = Rc::new(Cell::new(0));
    let view = |reverse: bool, step: usize| {
        let output = calls.clone();
        let action = button()
            .id("action")
            .key("action")
            .on_click(move || output.set(output.get() + step))
            .child(div().id("label"));
        let other = button().id("other").key("other");
        let children = if reverse {
            [other, action]
        } else {
            [action, other]
        };
        div().children(children)
    };
    let mut tree = build(view(false, 1), None);
    let action = id(&tree, "action");
    let label = id(&tree, "label");
    tree.set_focused(Some(action));
    assert!(tree.click(action));
    tree.reconcile_root(view(true, 5));
    layout(&mut tree);
    assert_eq!(id(&tree, "action"), action);
    assert_eq!(id(&tree, "label"), label);
    assert_eq!(tree.focused(), Some(action));
    assert!(tree.click(label));
    assert_eq!(calls.get(), 6);
}

#[test]
fn changing_constructor_replaces_widget_identity_and_clears_focus() {
    let mut tree = build(div().child(button().id("action").key("action")), None);
    let old = id(&tree, "action");
    tree.set_focused(Some(old));
    tree.reconcile_root(div().child(div().id("action").key("action")));
    layout(&mut tree);
    assert_ne!(id(&tree, "action"), old);
    assert_eq!(tree.focused(), None);
    assert!(!tree.click(old));
}

#[test]
fn dialog_defaults_modal_focus_and_reconciliation_share_one_lifecycle() {
    for css in [None, Some("dialog { width: 100px; height: 60px; }")] {
        let view = || {
            div().child(button().id("launch")).child(
                dialog()
                    .id("modal")
                    .key("modal")
                    .child(button().id("inside").attr("autofocus", "")),
            )
        };
        let mut tree = build(view(), css);
        let launch = id(&tree, "launch");
        let modal = id(&tree, "modal");
        assert_eq!(tree.layout_style(modal).display, Display::None);
        tree.set_focused(Some(launch));
        assert!(tree.show_modal(modal).unwrap());
        layout(&mut tree);
        assert_eq!(tree.focused(), Some(id(&tree, "inside")));
        assert!(tree.is_inert(launch));
        assert_ne!(tree.layout_style(modal).display, Display::None);

        tree.reconcile_root(view());
        layout(&mut tree);
        assert_eq!(id(&tree, "modal"), modal);
        assert_eq!(tree.active_modal(), Some(modal));
        assert_ne!(tree.layout_style(modal).display, Display::None);
        assert!(tree.close_top_layer(modal));
        layout(&mut tree);
        assert_eq!(tree.focused(), Some(launch));
        assert_eq!(tree.layout_style(modal).display, Display::None);
    }
}

#[test]
fn non_modal_dialog_open_attribute_and_type_replacement_work() {
    let mut tree = build(
        div().child(dialog().id("panel").key("panel").attr("open", "")),
        None,
    );
    let panel = id(&tree, "panel");
    assert_ne!(tree.layout_style(panel).display, Display::None);
    assert_eq!(tree.active_modal(), None);
    tree.show_modal(panel).unwrap();
    tree.reconcile_root(div().child(div().id("panel").key("panel")));
    layout(&mut tree);
    assert_ne!(id(&tree, "panel"), panel);
    assert_eq!(tree.active_modal(), None);
    assert_eq!(tree.top_layer().count(), 0);
}

#[test]
fn custom_containers_declare_their_own_name_and_share_child_methods() {
    struct Card;
    impl Container for Card {}
    impl Widget for Card {
        fn tag_name(&self) -> &'static str {
            "card"
        }
        fn reconcile(&mut self, _: &dyn Widget) -> WidgetUpdate {
            WidgetUpdate::Unchanged
        }
        fn layout(
            &mut self,
            inputs: voidui::core::layout::LayoutInput,
            ctx: voidui::core::context::LayoutContext<'_, '_>,
        ) -> voidui::core::layout::LayoutOutput {
            ctx.layout_children(inputs)
        }
    }
    let tree = build(
        WidgetBuilder::from_widget(Card)
            .child(button())
            .children([div()]),
        Some("card { width: 120px; }"),
    );
    let root = tree.root().unwrap();
    assert_eq!(tree.bounds(root).size.width, 120.0);
    assert_eq!(tree.children(root).len(), 2);
}

#[cfg(feature = "editing")]
#[test]
fn input_constructors_keep_editing_clients_and_default_focus() {
    use voidui::{Editor, input, textarea};

    let mut tree = build(
        div()
            .child(button().id("action"))
            .child(input(Editor::default()).id("single"))
            .child(textarea(Editor::default()).id("multi")),
        Some(
            "* { font-family: 'IBM Plex Sans'; } input { width: 90px; } textarea { width: 150px; }",
        ),
    );
    let single = id(&tree, "single");
    let multi = id(&tree, "multi");
    assert!(tree.text_input(single).is_some());
    assert!(tree.text_input(multi).is_some());
    assert_eq!(tree.bounds(single).size.width, 90.0);
    assert_eq!(tree.bounds(multi).size.width, 150.0);
    assert!(tree.focus_next(false));
    assert_eq!(tree.focused(), Some(id(&tree, "action")));
    assert!(tree.focus_next(false));
    assert_eq!(tree.focused(), Some(single));
    assert!(tree.focus_next(false));
    assert_eq!(tree.focused(), Some(multi));
}
