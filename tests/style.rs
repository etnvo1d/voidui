//! Fluent declarations and inheritance run through the real widget tree/Taffy adapter.
use std::{borrow::Cow, sync::Arc};
use voidui::{
    core::{
        element::IntoElement,
        layout::{
            self, Direction, Display, FlexDirection, FlexWrap, LengthPercentage,
            LengthPercentageAuto, Size, TaffyMaxContent, fr, length,
        },
        widget_tree::WidgetTree,
    },
    div,
    render::{FontStyle, FontWeight, ParleyTextSystem, TextLayoutCache, TextSystem, font},
    style::{
        AUTO, CssValue, FontSize, LayoutProperty, LineHeight, TextAlignment,
        color::{Color, Rgba8},
        pct,
        text::TextStyle,
    },
    text,
};

const RED: Rgba8 = Rgba8 {
    r: 255,
    g: 0,
    b: 0,
    a: 255,
};
const BLUE: Rgba8 = Rgba8 {
    r: 0,
    g: 0,
    b: 255,
    a: 255,
};

fn cache() -> TextLayoutCache {
    let fonts = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    fonts
        .add_fonts(vec![Cow::Borrowed(include_bytes!(
            "../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
        ))])
        .unwrap();
    TextLayoutCache::new(Arc::new(TextSystem::new(Arc::new(fonts))))
}
fn build(element: impl IntoElement) -> WidgetTree {
    let mut tree = WidgetTree::new();
    tree.build_root(element);
    tree.layout(Size::MAX_CONTENT, &cache());
    tree
}

#[test]
fn flex_conveniences_enable_display_and_place_children() {
    for (column, reverse) in [(false, false), (true, false), (false, true), (true, true)] {
        let root = match (column, reverse) {
            (false, false) => div().flex_row(),
            (true, false) => div().flex_col(),
            (false, true) => div().flex_row_reverse(),
            (true, true) => div().flex_col_reverse(),
        };
        let tree = build(
            root.size(100, 100)
                .gap(10)
                .child(div().size(20, 20))
                .child(div().size(20, 20)),
        );
        let root = tree.root().unwrap();
        assert_eq!(tree.layout_style(root).display, Display::Flex);
        let children = tree.children(root);
        let first = tree.bounds(children[0]).origin;
        let second = tree.bounds(children[1]).origin;
        let displacement = if column {
            second.y - first.y
        } else {
            second.x - first.x
        };
        assert_eq!(displacement, if reverse { -30.0 } else { 30.0 });
    }
}

#[test]
fn shorthands_and_longhands_obey_declaration_order() {
    let tree = build(
        div()
            .margin(20)
            .margin_top(3)
            .margin_x(-2)
            .padding(10)
            .padding_y(4)
            .border_width(2)
            .border_left_width(5)
            .gap(8)
            .row_gap(3)
            .inset(4)
            .left(AUTO),
    );
    let s = tree.layout_style(tree.root().unwrap());
    assert_eq!(
        s.margin,
        layout::Rect {
            top: length(3),
            right: length(-2),
            bottom: length(20),
            left: length(-2)
        }
    );
    assert_eq!(
        s.padding,
        layout::Rect {
            top: length(4),
            right: length(10),
            bottom: length(4),
            left: length(10)
        }
    );
    assert_eq!(s.border.left, length(5));
    assert_eq!(
        s.gap,
        Size {
            width: length(8),
            height: length(3)
        }
    );
    assert_eq!(s.inset.left, LengthPercentageAuto::auto());
    let tree = build(div().margin_top(3).margin(12).padding_left(2).padding(6));
    let s = tree.layout_style(tree.root().unwrap());
    assert_eq!(s.margin.top, length(12));
    assert_eq!(s.padding.left, length(6));
}

#[test]
fn numbers_percent_auto_and_native_lengths_are_accepted() {
    let tree = build(
        div().width(200).height(100).child(
            div()
                .width(pct(50.0))
                .height(25u32)
                .margin_x(AUTO)
                .padding_top(LengthPercentage::percent(0.1)),
        ),
    );
    let child = tree.children(tree.root().unwrap())[0];
    assert_eq!(tree.bounds(child).size.width, 100.0);
    assert_eq!(
        tree.layout_style(child).padding.top,
        LengthPercentage::percent(0.1)
    );
    assert_eq!(tree.layout_result(child).margin.left, 50.0);
}

#[test]
fn color_inherits_through_containers_and_child_overrides_do_not_leak() {
    let tree = build(
        div()
            .color(RED)
            .child(div().child("inherited").child(text("override").color(BLUE)))
            .child("sibling"),
    );
    let root = tree.root().unwrap();
    let container = tree.children(root)[0];
    let children = tree.children(container);
    assert_eq!(tree.text_style(container).color, RED.into());
    assert_eq!(tree.text_style(children[0]).color, RED.into());
    assert_eq!(tree.text_style(children[1]).color, BLUE.into());
    assert_eq!(tree.text_style(tree.children(root)[1]).color, RED.into());
}

#[test]
fn css_defaulting_keywords_have_distinct_inherited_behavior() {
    let tree = build(
        div()
            .color(RED)
            .font_size(30)
            .child(text("inherit").color(CssValue::Inherit))
            .child(
                text("initial")
                    .color(CssValue::Initial)
                    .font_size(CssValue::Initial),
            )
            .child(text("unset").color(BLUE).color(CssValue::Unset)),
    );
    let children = tree.children(tree.root().unwrap());
    assert_eq!(tree.text_style(children[0]).color, RED.into());
    assert_eq!(
        tree.text_style(children[1]).color,
        TextStyle::default().color
    );
    assert_eq!(
        tree.text_style(children[1]).font_size,
        TextStyle::default().font_size
    );
    assert_eq!(tree.text_style(children[2]).color, RED.into());
    let tree = build(text("root").color(CssValue::Inherit));
    assert_eq!(
        tree.text_style(tree.root().unwrap()).color,
        TextStyle::default().color
    );
}

#[test]
fn family_changes_preserve_independent_font_longhands() {
    let tree = build(
        div()
            .font_options(font("IBM Plex Sans").bold().italic())
            .child(text("child").font("IBM Plex Sans"))
            .child(
                text("reset")
                    .font("IBM Plex Sans")
                    .font_weight(CssValue::Initial),
            ),
    );
    let children = tree.children(tree.root().unwrap());
    assert_eq!(tree.text_style(children[0]).font.weight, FontWeight::BOLD);
    assert_eq!(tree.text_style(children[0]).font.style, FontStyle::Italic);
    assert_eq!(
        tree.text_style(children[1]).font.weight,
        FontWeight::default()
    );
    assert_eq!(tree.text_style(children[1]).font.style, FontStyle::Italic);
    let tree = build(text("order").bold().italic().font("IBM Plex Sans"));
    assert_eq!(
        tree.text_style(tree.root().unwrap()).font.weight,
        FontWeight::BOLD
    );
}

#[test]
fn relative_font_size_is_computed_once_before_inheritance() {
    let tree = build(
        div().font_size(20).child(
            div()
                .font_size(FontSize::Percent(1.5))
                .child("inherits pixels")
                .child(text("relative").font_size(FontSize::Em(2.0))),
        ),
    );
    let parent = tree.children(tree.root().unwrap())[0];
    assert_eq!(tree.text_style(parent).font_size, 30.0);
    assert_eq!(tree.text_style(tree.children(parent)[0]).font_size, 30.0);
    assert_eq!(tree.text_style(tree.children(parent)[1]).font_size, 60.0);
}

#[test]
fn percentage_line_height_inherits_pixels_unitless_inherits_multiplier() {
    for (height, expected) in [
        (LineHeight::Percent(1.5), 30.0),
        (LineHeight::Em(1.5), 30.0),
        (LineHeight::Pixels(30.0), 30.0),
        (LineHeight::Relative(1.5), 15.0),
    ] {
        let tree = build(
            div()
                .font_size(20)
                .line_height(height)
                .child(text("child").font_size(10)),
        );
        let child = tree.children(tree.root().unwrap())[0];
        assert_eq!(tree.bounds(child).size.height, expected);
    }
}

#[test]
fn layout_spacing_and_background_do_not_inherit_by_default() {
    let tree = build(
        div()
            .flex_col()
            .padding(10)
            .margin(20)
            .gap(15)
            .background(RED)
            .border_color(BLUE)
            .border_radius(8)
            .child(div().height(20)),
    );
    let child = tree.children(tree.root().unwrap())[0];
    let s = tree.layout_style(child);
    assert_eq!(s.display, Display::Block);
    assert_eq!(s.flex_direction, FlexDirection::Row);
    assert_eq!(s.padding.top, length(0));
    assert_eq!(s.margin.top, length(0));
    assert_eq!(s.gap.width, length(0));
    assert_eq!(tree.paint_style(child).border_color, Color::CurrentColor);
    assert_eq!(tree.paint_style(child).border_radius, 0.0);
    assert_eq!(
        tree.paint_style(child).background,
        Rgba8::new(0, 0, 0, 0).into()
    );
}

#[test]
fn layout_inherit_uses_computed_values_and_later_setters_override() {
    let tree = build(
        div()
            .width(240)
            .padding_top(12)
            .margin_left(pct(10.0))
            .child(
                div()
                    .width(100)
                    .inherit(LayoutProperty::PaddingTop)
                    .inherit(LayoutProperty::MarginLeft),
            )
            .child(div().inherit(LayoutProperty::PaddingTop).padding(3))
            .child(div().padding(30).initial(LayoutProperty::PaddingTop))
            .child(div().padding(30).unset(LayoutProperty::PaddingTop)),
    );
    let children = tree.children(tree.root().unwrap());
    assert_eq!(tree.layout_style(children[0]).padding.top, length(12));
    assert_eq!(
        tree.layout_style(children[0]).margin.left,
        LengthPercentageAuto::percent(0.1)
    );
    assert_eq!(tree.layout_result(children[0]).margin.left, 24.0);
    assert_eq!(tree.layout_style(children[1]).padding.top, length(3));
    assert_eq!(tree.layout_style(children[2]).padding.top, length(0));
    assert_eq!(tree.layout_style(children[3]).padding.top, length(0));
    // Authored values are not overwritten with inherited computed results.
    assert_eq!(tree.style(children[0]).layout.padding.top, length(0));
}

#[test]
fn overflow_shorthand_can_override_one_inherited_axis() {
    let tree = build(
        div().overflow(layout::Overflow::Hidden).child(
            div()
                .inherit(LayoutProperty::OverflowX)
                .inherit(LayoutProperty::OverflowY)
                .overflow_axes(layout::Point {
                    x: layout::Overflow::Visible,
                    y: layout::Overflow::Clip,
                }),
        ),
    );
    let s = tree.layout_style(tree.children(tree.root().unwrap())[0]);
    assert_eq!(s.overflow.x, layout::Overflow::Visible);
    assert_eq!(s.overflow.y, layout::Overflow::Clip);
}

#[test]
fn inherited_direction_reaches_taffy_and_logical_alignment() {
    let tree = build(
        div()
            .direction(Direction::Rtl)
            .width(200)
            .child(
                div()
                    .flex_row()
                    .width(100)
                    .child(div().size(20, 20))
                    .child(div().size(30, 20)),
            )
            .child(text("end").text_align(TextAlignment::End)),
    );
    let root = tree.root().unwrap();
    let parent = tree.children(root)[0];
    let children = tree.children(parent);
    assert_eq!(tree.layout_style(parent).direction, Direction::Rtl);
    assert!(tree.bounds(children[0]).origin.x > tree.bounds(children[1]).origin.x);
    let child = tree.children(root)[1];
    assert_eq!(
        tree.text_style(child)
            .align
            .resolve(tree.text_style(child).direction),
        voidui::render::TextAlign::Left
    );
}

#[test]
fn reflow_refreshes_inherited_values_without_changing_declarations() {
    let mut tree = build(
        div()
            .color(RED)
            .font_size(20)
            .padding_top(10)
            .child(div().inherit(LayoutProperty::PaddingTop).child("inherited")),
    );
    let root = tree.root().unwrap();
    let child = tree.children(root)[0];
    let text = tree.children(child)[0];
    tree.style_mut(root).color = BLUE.into();
    tree.style_mut(root).font_size = 30.into();
    tree.style_mut(root).layout.padding.top = length(25);
    tree.layout(Size::MAX_CONTENT, &cache());
    assert_eq!(tree.text_style(text).color, BLUE.into());
    assert_eq!(tree.text_style(text).font_size, 30.0);
    assert_eq!(tree.layout_style(child).padding.top, length(25));
    assert_eq!(tree.style(text).color, CssValue::Unset);
}

#[test]
fn currentcolor_is_inherited_on_color_and_local_on_borders() {
    let tree = build(
        div().color(RED).border_color(BLUE).child(
            text("child")
                .color(Color::CurrentColor)
                .background(Color::CurrentColor),
        ),
    );
    let child = tree.children(tree.root().unwrap())[0];
    assert_eq!(tree.text_style(child).color, RED.into());
    assert_eq!(
        tree.paint_style(child)
            .background
            .resolve(tree.text_style(child).color),
        RED.into()
    );
    assert_eq!(tree.paint_style(child).border_color, Color::CurrentColor);
}

#[test]
fn grid_and_position_setters_are_usable_on_all_widgets() {
    let tree = build(
        div()
            .grid()
            .size(220, 100)
            .column_gap(20)
            .row_gap(5)
            .grid_template_columns(vec![fr(1.0), fr(1.0)])
            .grid_auto_rows(vec![length(30)])
            .align_items(layout::AlignItems::CENTER)
            .child(text("grid").min_width(0).max_width(100))
            .child(div().absolute().size(20, 10).right(5).bottom(7)),
    );
    let root = tree.root().unwrap();
    let child = tree.children(root)[1];
    assert_eq!(tree.bounds(child).origin.x, 195.0);
    assert_eq!(tree.bounds(child).origin.y, 83.0);
    assert_eq!(tree.layout_style(root).grid_template_columns.len(), 2);
}

#[test]
fn wrapping_can_be_inherited_and_overridden() {
    let tree = build(
        div()
            .width(50)
            .wrap(false)
            .child("alpha beta gamma")
            .child(text("alpha beta gamma").wrap(true)),
    );
    let children = tree.children(tree.root().unwrap());
    assert!(!tree.text_style(children[0]).wrap);
    assert!(tree.text_style(children[1]).wrap);
    assert!(tree.bounds(children[1]).size.height > tree.bounds(children[0]).size.height);
}

#[test]
#[should_panic(expected = "padding_top must be finite and nonnegative")]
fn negative_padding_is_rejected() {
    div().padding(-5);
}
#[test]
#[should_panic(expected = "row_gap must be finite and nonnegative")]
fn nonfinite_gap_is_rejected() {
    div().gap(f32::NAN);
}
#[test]
fn negative_margins_and_offsets_are_valid() {
    let tree = build(
        div()
            .relative()
            .margin(-10)
            .top(-5)
            .flex_wrap(FlexWrap::Wrap),
    );
    let s = tree.layout_style(tree.root().unwrap());
    assert_eq!(s.margin.left, length(-10));
    assert_eq!(s.inset.top, length(-5));
}

#[test]
fn paint_properties_support_explicit_defaulting_without_implicit_inheritance() {
    let tree = build(
        div()
            .background(RED)
            .border_color(BLUE)
            .border_radius(10)
            .child(
                div()
                    .background(CssValue::Inherit)
                    .border_color(CssValue::Inherit)
                    .border_radius(CssValue::Inherit),
            )
            .child(
                div()
                    .background(RED)
                    .background(CssValue::Unset)
                    .border_radius(CssValue::Initial),
            ),
    );
    let children = tree.children(tree.root().unwrap());
    assert_eq!(tree.paint_style(children[0]).background, RED.into());
    assert_eq!(tree.paint_style(children[0]).border_color, BLUE.into());
    assert_eq!(tree.paint_style(children[0]).border_radius, 10.0);
    assert_eq!(
        *tree.paint_style(children[1]),
        voidui::style::paint::PaintStyle::default()
    );
}

#[test]
fn font_family_alias_and_numeric_weight_preserve_longhand_order() {
    let tree = build(
        div()
            .font_weight(650)
            .italic()
            .child(text("family").font_family("IBM Plex Sans")),
    );
    let style = tree.text_style(tree.children(tree.root().unwrap())[0]);
    assert_eq!(style.font.weight, FontWeight(650.0));
    assert_eq!(style.font.style, FontStyle::Italic);
}
