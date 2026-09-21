//! Local edits must preserve unrelated style and layout work, without timing thresholds.
use std::{cell::Cell, rc::Rc, sync::Arc};
use voidui::{
    core::{
        context::LayoutContext,
        element::ElementProps,
        layout::{self, LayoutInput, LayoutOutput, Size, TaffyMaxContent},
        widget::{Widget, WidgetBuilder},
        widget_tree::WidgetTree,
    },
    div,
    render::{ParleyTextSystem, TextLayoutCache, TextSystem},
    style::css::Stylesheet,
};

fn cache() -> TextLayoutCache {
    TextLayoutCache::new(Arc::new(TextSystem::new(Arc::new(
        ParleyTextSystem::new_without_system_fonts("unused"),
    ))))
}

struct CountLayout(Rc<Cell<usize>>);
impl Widget for CountLayout {
    fn layout(&mut self, input: LayoutInput, ctx: LayoutContext<'_, '_>) -> LayoutOutput {
        self.0.set(self.0.get() + 1);
        layout::layout_leaf(ctx.layout_style(), input, |_, _| Size {
            width: 40.,
            height: 20.,
        })
    }
}
fn counted(calls: &Rc<Cell<usize>>) -> WidgetBuilder<CountLayout> {
    WidgetBuilder {
        widget: CountLayout(calls.clone()),
        props: ElementProps::new(Default::default()),
        children: Vec::new(),
        events: Default::default(),
    }
}

#[test]
fn changing_one_inline_width_does_not_recascade_the_file_list() {
    let cache = cache();
    let mut tree = WidgetTree::new();
    tree.build_root(
        div()
            .id("sidebar")
            .width(200)
            .children((0..256).map(|_| div().class("row").height(24))),
    );
    tree.set_stylesheets(vec![Stylesheet::parse(".row { padding: 4px; }").unwrap()]);
    tree.layout(Size::MAX_CONTENT, &cache);
    let before = tree.cascade_stats();
    tree.style_mut(tree.root().unwrap()).layout.size.width = layout::length(210);
    tree.layout(Size::MAX_CONTENT, &cache);
    assert_eq!(tree.bounds(tree.root().unwrap()).size.width, 210.);
    assert_eq!(tree.cascade_stats().candidate_tests, before.candidate_tests);
}

#[test]
fn unchanged_layout_reuses_retained_measurements() {
    let calls = Rc::new(Cell::new(0));
    let cache = cache();
    let mut tree = WidgetTree::new();
    tree.build_root(div().child(counted(&calls)));
    tree.layout(Size::MAX_CONTENT, &cache);
    calls.set(0);
    tree.layout(Size::MAX_CONTENT, &cache);
    assert_eq!(calls.get(), 0, "unchanged widgets were laid out again");
}

#[test]
fn local_width_change_preserves_an_unaffected_siblings_layout() {
    let calls = Rc::new(Cell::new(0));
    let cache = cache();
    let mut tree = WidgetTree::new();
    tree.build_root(
        div()
            .width(800)
            .flex()
            .child(div().id("sidebar").width(200))
            .child(div().width(300).child(counted(&calls))),
    );
    tree.layout(Size::MAX_CONTENT, &cache);
    calls.set(0);
    tree.style_mut(tree.find_by_id("sidebar").unwrap())
        .layout
        .size
        .width = layout::length(210);
    tree.layout(Size::MAX_CONTENT, &cache);
    assert_eq!(calls.get(), 0, "an unrelated sibling lost its layout cache");
}

#[test]
fn reconciliation_stops_style_propagation_before_unchanged_rows() {
    let view = |width| {
        div().width(width).child(
            div()
                .w_max()
                .min_w_full()
                .children((0..256).map(|_| div().child(div().class("row").height(24)))),
        )
    };
    let cache = cache();
    let mut tree = WidgetTree::new();
    tree.build_root(view(200));
    tree.set_stylesheets(vec![Stylesheet::parse(".row { padding: 1rem; }").unwrap()]);
    tree.layout(Size::MAX_CONTENT, &cache);
    let styles = tree.style_resolutions();
    let candidates = tree.cascade_stats().candidate_tests;
    tree.reconcile_root(view(210));
    tree.layout(Size::MAX_CONTENT, &cache);
    // The sidebar changed; its directory's computed style did not. The rows
    // therefore need neither selector matching nor relative-length resolution.
    assert_eq!(tree.style_resolutions() - styles, 2);
    assert_eq!(tree.cascade_stats().candidate_tests, candidates);
}

#[test]
fn local_edits_preserve_inheritance_important_rules_and_removed_overrides() {
    let cache = cache();
    let mut tree = WidgetTree::new();
    tree.build_root(
        div()
            .width(200)
            .font_size(10)
            .child(div().id("child").child(div().id("grandchild"))),
    );
    tree.set_stylesheets(vec![Stylesheet::parse(
        ":root {width: 90px; color: red !important} #child {width:inherit; color:inherit} #grandchild {width:2em; height:1rem}"
    ).unwrap()]);
    tree.layout(Size::MAX_CONTENT, &cache);
    let root = tree.root().unwrap();
    let child = tree.find_by_id("child").unwrap();
    let grandchild = tree.find_by_id("grandchild").unwrap();
    tree.style_mut(root).layout.size.width = layout::length(300);
    tree.style_mut(root).font_size = 20.into();
    tree.style_mut(root).color = "blue"
        .parse::<voidui::style::color::Color>()
        .unwrap()
        .into();
    tree.layout(Size::MAX_CONTENT, &cache);
    assert_eq!(tree.bounds(child).size.width, 300.);
    assert_eq!(tree.bounds(grandchild).size.width, 40.);
    assert_eq!(tree.bounds(grandchild).size.height, 20.);
    assert_eq!(tree.text_style(child).color, "red".parse().unwrap());
    *tree.style_mut(root) = Default::default();
    tree.layout(Size::MAX_CONTENT, &cache);
    assert_eq!(tree.bounds(child).size.width, 90.);
    assert_eq!(tree.bounds(grandchild).size.width, 32.);
}

#[test]
fn local_style_edits_and_viewport_changes_in_the_same_frame_both_apply() {
    let cache = cache();
    let mut tree = WidgetTree::new();
    tree.build_root(div().id("root").child(div().id("child")));
    tree.set_stylesheets(vec![
        Stylesheet::parse("#child {width:50vw;height:20px}").unwrap(),
    ]);
    let space = |width| Size {
        width: layout::AvailableSpace::Definite(width),
        height: layout::AvailableSpace::Definite(200.),
    };
    tree.layout(space(400.), &cache);
    let child = tree.find_by_id("child").unwrap();
    tree.style_mut(child).layout.size.height = layout::length(30);
    tree.layout(space(600.), &cache);
    assert_eq!(tree.bounds(child).size.width, 300.);
    assert_eq!(tree.bounds(child).size.height, 30.);
}

#[test]
fn widget_model_changes_invalidate_intrinsic_ancestors() {
    struct Model(Rc<Cell<f32>>);
    impl Widget for Model {
        fn layout(&mut self, input: LayoutInput, ctx: LayoutContext<'_, '_>) -> LayoutOutput {
            layout::layout_leaf(ctx.layout_style(), input, |_, _| Size {
                width: self.0.get(),
                height: 20.,
            })
        }
    }
    let cache = cache();
    let width = Rc::new(Cell::new(40.));
    let mut tree = WidgetTree::new();
    tree.build_root(div().w_max().child(WidgetBuilder {
        widget: Model(width.clone()),
        props: ElementProps::new(Default::default()),
        children: Vec::new(),
        events: Default::default(),
    }));
    tree.layout(Size::MAX_CONTENT, &cache);
    let root = tree.root().unwrap();
    width.set(90.);
    tree.invalidator(tree.children(root)[0]).relayout();
    tree.layout(Size::MAX_CONTENT, &cache);
    assert_eq!(tree.bounds(root).size.width, 90.);
}

#[test]
fn font_system_and_font_revision_changes_expire_layout_cache() {
    let calls = Rc::new(Cell::new(0));
    let backend = Arc::new(ParleyTextSystem::new_without_system_fonts("unused"));
    let fonts = TextLayoutCache::new(Arc::new(TextSystem::new(backend.clone())));
    let mut tree = WidgetTree::new();
    tree.build_root(div().child(counted(&calls)));
    tree.layout(Size::MAX_CONTENT, &fonts);
    calls.set(0);
    backend
        .add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!(
            "../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
        ))])
        .unwrap();
    tree.layout(Size::MAX_CONTENT, &fonts);
    assert!(calls.get() > 0);
    calls.set(0);
    tree.layout(Size::MAX_CONTENT, &cache());
    assert!(calls.get() > 0);
}

#[test]
fn retained_layout_restores_transform_overflow_before_recomputing_it() {
    use voidui::Overflow;
    let cache = cache();
    let mut tree = WidgetTree::new();
    tree.build_root(
        div()
            .size(100, 100)
            .overflow(Overflow::Auto)
            .child(div().id("child").size(50, 50)),
    );
    let child = tree.find_by_id("child").unwrap();
    tree.style_mut(child).transform = "translateX(200px)"
        .parse::<voidui::style::transform::Transform>()
        .unwrap()
        .into();
    tree.layout(Size::MAX_CONTENT, &cache);
    let root = tree.root().unwrap();
    assert_eq!(tree.scroll_metrics(root).unwrap().max.x, 150.);
    tree.layout(Size::MAX_CONTENT, &cache);
    tree.style_mut(child).transform = "translateX(0px)"
        .parse::<voidui::style::transform::Transform>()
        .unwrap()
        .into();
    tree.update_styles(std::time::Instant::now());
    assert_eq!(tree.scroll_metrics(root).unwrap().max.x, 0.);
}

#[test]
fn persistent_scroll_layout_discovers_and_removes_classic_gutters() {
    use voidui::{Overflow, ScrollbarMode};
    let cache = cache();
    let mut tree = WidgetTree::new();
    tree.build_root(
        div()
            .size(100, 100)
            .overflow(Overflow::Auto)
            .scrollbar_mode(ScrollbarMode::Classic)
            .child(div().id("content").size(100, 200)),
    );
    let root = tree.root().unwrap();
    let content = tree.find_by_id("content").unwrap();
    for tall in [true, true, false, true, false] {
        tree.style_mut(content).layout.size = Size {
            width: layout::length(if tall { 100 } else { 30 }),
            height: layout::length(if tall { 200 } else { 30 }),
        };
        tree.layout(Size::MAX_CONTENT, &cache);
        let gutter = if tall {
            tree.scroll_options().width
        } else {
            0.
        };
        assert_eq!(
            tree.layout_result(root).scrollbar_size,
            Size {
                width: gutter,
                height: gutter
            }
        );
    }
}

#[test]
fn transition_longhand_inheritance_survives_local_ancestor_updates() {
    use std::time::{Duration, Instant};
    let cache = cache();
    let mut tree = WidgetTree::new();
    tree.build_root(div().child(div().id("child").width(100)));
    tree.set_stylesheets(vec![Stylesheet::parse(
        ":root {transition-property:width;transition-duration:0s;transition-timing-function:linear} #child {transition:inherit}"
    ).unwrap()]);
    tree.layout(Size::MAX_CONTENT, &cache);
    let now = Instant::now();
    let root = tree.root().unwrap();
    let child = tree.find_by_id("child").unwrap();
    tree.style_mut(root).transition_duration =
        voidui::style::list::StyleList::from(vec![1.]).into();
    tree.update_styles(now);
    tree.style_mut(child).layout.size.width = layout::length(200);
    tree.update_styles(now);
    assert_eq!(tree.layout_style(child).size.width, layout::length(100));
    tree.update_styles(now + Duration::from_millis(500));
    assert_eq!(tree.layout_style(child).size.width, layout::length(150));
}

#[test]
fn subtree_measurement_and_structural_edits_do_not_leave_stale_ancestors() {
    let cache = cache();
    let mut tree = WidgetTree::new();
    tree.build_root(
        div()
            .width(300)
            .child(div().id("branch").child(div().height(20))),
    );
    tree.layout(Size::MAX_CONTENT, &cache);
    let root = tree.root().unwrap();
    let branch = tree.find_by_id("branch").unwrap();
    tree.layout_subtree(
        branch,
        Size {
            width: layout::AvailableSpace::Definite(100.),
            height: layout::AvailableSpace::MaxContent,
        },
        Default::default(),
        &cache,
    );
    tree.layout(Size::MAX_CONTENT, &cache);
    assert_eq!(tree.bounds(branch).size.width, 300.);
    let added = tree.append_child(branch, div().height(30)).unwrap();
    tree.layout(Size::MAX_CONTENT, &cache);
    assert_eq!(tree.bounds(root).size.height, 50.);
    tree.style_mut(added).layout.size.height = layout::length(40);
    tree.remove_subtree(added);
    tree.layout(Size::MAX_CONTENT, &cache);
    assert_eq!(tree.bounds(root).size.height, 20.);
}

#[test]
fn replacing_a_custom_widgets_defaults_restyles_only_that_widget() {
    struct SizedWidget(f32);
    impl Widget for SizedWidget {
        fn default_style(&self) -> Option<voidui::style::style::Style> {
            let mut style = voidui::style::style::Style::default();
            style.layout.size.width = layout::length(self.0);
            Some(style)
        }
        fn layout(&mut self, input: LayoutInput, ctx: LayoutContext<'_, '_>) -> LayoutOutput {
            layout::layout_leaf(ctx.layout_style(), input, |_, _| Size::ZERO)
        }
    }
    let view = |width| {
        div()
            .width(800)
            .flex()
            .child(WidgetBuilder {
                widget: SizedWidget(width),
                props: ElementProps::new(Default::default()),
                children: Vec::new(),
                events: Default::default(),
            })
            .child(div().class("other").width(100))
    };
    let cache = cache();
    let mut tree = WidgetTree::new();
    tree.build_root(view(50.));
    tree.set_stylesheets(vec![Stylesheet::parse(".other {padding:1px}").unwrap()]);
    tree.layout(Size::MAX_CONTENT, &cache);
    let candidates = tree.cascade_stats().candidate_tests;
    let widget = tree.children(tree.root().unwrap())[0];
    tree.reconcile_root(view(80.));
    tree.layout(Size::MAX_CONTENT, &cache);
    assert_eq!(tree.bounds(widget).size.width, 80.);
    assert_eq!(tree.cascade_stats().candidate_tests, candidates);
}
