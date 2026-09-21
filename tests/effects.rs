//! CSS effects are sampled with an injected clock; these tests never wait for VSync.
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use voidui::{
    core::{
        layout::{Dimension, Size, TaffyMaxContent},
        widget_tree::WidgetTree,
    },
    div,
    render::{ParleyTextSystem, TextLayoutCache, TextSystem},
    style::{
        color::{Color, ColorSpace},
        css::Stylesheet,
        transition::{Easing, StepPosition},
    },
};
fn cache() -> TextLayoutCache {
    TextLayoutCache::new(Arc::new(TextSystem::new(Arc::new(
        ParleyTextSystem::new_without_system_fonts("unused"),
    ))))
}
fn tree(css: &str) -> (WidgetTree, Instant) {
    let mut tree = WidgetTree::new();
    tree.build_root(div().child(div()));
    tree.set_stylesheets(vec![Stylesheet::parse(css).unwrap()]);
    let now = Instant::now();
    tree.update_styles(now);
    tree.layout(Size::MAX_CONTENT, &cache());
    (tree, now)
}
fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 0.001, "{a} != {b}");
}
fn width(tree: &WidgetTree) -> f32 {
    tree.layout_style(tree.root().unwrap()).size.width.value()
}
fn activate(tree: &mut WidgetTree) {
    tree.set_classes(tree.root().unwrap(), "active");
}

#[test]
fn initial_styles_do_not_animate_and_idle_does_not_recascade() {
    let (mut tree, t) = tree("div {width:100px;transition:all 1s;}");
    assert_eq!(width(&tree), 100.0);
    assert!(tree.next_animation_frame(t).is_none());
    let stats = tree.cascade_stats();
    assert_eq!(
        tree.update_styles(t + Duration::from_secs(1)),
        Default::default()
    );
    assert_eq!(stats, tree.cascade_stats());
}
#[test]
fn length_animation_requests_layout_and_finishes() {
    let (mut tree, t) =
        tree(":root{width:100px;transition:width 1s linear}:root.active{width:200px}");
    activate(&mut tree);
    tree.update_styles(t);
    close(width(&tree), 100.0);
    assert!(tree.update_styles(t + Duration::from_millis(500)).layout);
    close(width(&tree), 150.0);
    tree.update_styles(t + Duration::from_secs(1));
    close(width(&tree), 200.0);
    assert!(
        tree.next_animation_frame(t + Duration::from_secs(1))
            .is_none()
    );
}
#[test]
fn foreground_animation_inherits_without_layout_or_selector_matching() {
    let (mut tree, t) =
        tree(":root{width:100px;color:red;transition:color 1s linear}:root.active{color:blue}");
    activate(&mut tree);
    tree.update_styles(t);
    let stats = tree.cascade_stats();
    let changes = tree.update_styles(t + Duration::from_millis(500));
    assert!(changes.paint);
    assert!(!changes.layout);
    assert_eq!(stats, tree.cascade_stats());
    let root = tree.root().unwrap();
    let color = tree.text_style(root).color;
    assert_eq!(color, tree.text_style(tree.children(root)[0]).color);
    let expected = "red".parse::<Color>().unwrap().interpolate(
        "blue".parse().unwrap(),
        0.5,
        ColorSpace::Oklab,
        Default::default(),
    );
    assert_eq!(color, expected);
}
#[test]
fn positive_and_negative_delay_and_zero_duration() {
    let (mut a, t) =
        tree(":root{width:100px;transition:width 1s linear 500ms}:root.active{width:200px}");
    activate(&mut a);
    a.update_styles(t);
    assert_eq!(
        a.next_animation_frame(t),
        Some(t + Duration::from_millis(500))
    );
    a.update_styles(t + Duration::from_millis(750));
    close(width(&a), 125.0);
    let (mut a, t) =
        tree(":root{width:100px;transition:width 1s linear -500ms}:root.active{width:200px}");
    activate(&mut a);
    a.update_styles(t);
    close(width(&a), 150.0);
    let (mut a, t) =
        tree(":root{width:100px;transition:width 0s linear 500ms}:root.active{width:200px}");
    activate(&mut a);
    a.update_styles(t);
    close(width(&a), 100.0);
    a.update_styles(t + Duration::from_millis(500));
    close(width(&a), 200.0);
    assert!(a.next_animation_frame(t).is_none());
}
#[test]
fn reversal_shortens_duration_and_retarget_starts_at_current_value() {
    let (mut a, t) = tree(
        ":root{width:100px;transition:width 1s linear}:root.active{width:200px}:root.third{width:300px}",
    );
    let root = a.root().unwrap();
    activate(&mut a);
    a.update_styles(t);
    // No intermediate sample: retarget must sample the old animation at this time.
    a.set_classes(root, "");
    a.update_styles(t + Duration::from_millis(400));
    close(width(&a), 140.0);
    a.update_styles(t + Duration::from_millis(600));
    close(width(&a), 120.0);
    a.update_styles(t + Duration::from_millis(800));
    close(width(&a), 100.0);
    assert!(a.next_animation_frame(t).is_none());
    activate(&mut a);
    a.update_styles(t + Duration::from_secs(1));
    a.set_classes(root, "third");
    a.update_styles(t + Duration::from_millis(1500));
    close(width(&a), 150.0);
    a.update_styles(t + Duration::from_secs(2));
    close(width(&a), 225.0);
}
#[test]
fn last_matching_property_and_repeated_lists_win() {
    let (mut a, t) = tree(
        ":root{width:100px;height:100px;transition-property:all,width;transition-duration:1s,2s;transition-timing-function:linear}:root.active{width:200px;height:200px}",
    );
    activate(&mut a);
    a.update_styles(t);
    a.update_styles(t + Duration::from_millis(500));
    close(width(&a), 125.0);
    assert_eq!(
        a.layout_style(a.root().unwrap()).size.height,
        Dimension::length(150.0).into_taffy()
    );
}
#[test]
fn timing_only_change_does_not_restart_or_cancel_a_running_transition() {
    let (mut a, t) = tree(
        ":root{width:100px;transition:width 1s linear}:root.active{width:200px}:root.active.short{transition-duration:0s}",
    );
    activate(&mut a);
    a.update_styles(t);
    let root = a.root().unwrap();
    a.set_classes(root, "active short");
    a.update_styles(t + Duration::from_millis(250));
    close(width(&a), 125.0);
    a.update_styles(t + Duration::from_millis(500));
    close(width(&a), 150.0);
}
#[test]
fn removal_none_and_display_none_cancel_transitions() {
    let (mut a, t) = tree(
        ":root{width:100px;transition:width 1s linear}:root.active{width:200px}:root.off{width:200px;transition:none}",
    );
    activate(&mut a);
    a.update_styles(t);
    let root = a.root().unwrap();
    a.set_classes(root, "off");
    a.update_styles(t + Duration::from_millis(250));
    close(width(&a), 200.0);
    assert!(a.next_animation_frame(t).is_none());
    a.set_stylesheets(vec![Stylesheet::parse("div{display:none}").unwrap()]);
    a.update_styles(t);
    assert!(a.next_animation_frame(t).is_none());
}
#[test]
fn shadows_pad_lists_and_inset_mismatch_is_discrete() {
    let (mut a, t) = tree(
        ":root{box-shadow:none;transition:box-shadow 1s linear}:root.active{box-shadow:10px 20px 30px 4px red,0 0 2px blue}",
    );
    activate(&mut a);
    a.update_styles(t);
    let change = a.update_styles(t + Duration::from_millis(500));
    assert!(!change.layout);
    let shadows = &a.paint_style(a.root().unwrap()).box_shadow;
    assert_eq!(shadows.len(), 2);
    close(shadows[0].offset_x, 5.0);
    close(shadows[0].blur, 15.0);
    let (mut a, t) = tree(
        ":root{box-shadow:0 0 red;transition:box-shadow 1s}:root.active{box-shadow:inset 0 0 blue}",
    );
    activate(&mut a);
    a.update_styles(t);
    assert!(a.next_animation_frame(t).is_none());
}
#[test]
fn easing_endpoints_steps_and_overshoot() {
    close(Easing::EASE.evaluate(0.5, false) as f32, 0.8024);
    close(
        Easing::Steps(4, StepPosition::Start).evaluate(0.0, true) as f32,
        0.0,
    );
    close(
        Easing::Steps(4, StepPosition::Start).evaluate(0.0, false) as f32,
        0.25,
    );
    close(
        Easing::Steps(4, StepPosition::JumpNone).evaluate(0.5, false) as f32,
        2.0 / 3.0,
    );
    assert!(Easing::CubicBezier(0.2, 2.0, 0.8, 2.0).evaluate(0.5, false) > 1.0);
}
#[test]
fn parses_all_color4_spaces_without_8bit_quantization() {
    for css in [
        "rgb(10.5 20.5 30.5 / .1234)",
        "hwb(20 10% 20%)",
        "lab(50% 20 -30)",
        "lch(50% 40 270)",
        "oklab(60% .1 -.1)",
        "oklch(70% .2 120)",
        "color(srgb-linear .1 .2 .3)",
        "color(display-p3 1 .2 0)",
        "color(a98-rgb .1 .2 .3)",
        "color(prophoto-rgb .1 .2 .3)",
        "color(rec2020 .1 .2 .3)",
        "color(xyz-d50 .1 .2 .3)",
        "color(xyz-d65 .1 .2 .3)",
        "oklch(50% none 120 / none)",
    ] {
        let c: Color = css.parse().unwrap();
        assert!(c.components().components.iter().all(|v| v.is_finite()));
    }
    let c: Color = "rgb(10.5 20.5 30.5 / .1234)".parse().unwrap();
    close(c.components().components[3], 0.1234);
}
#[test]
fn rejects_invalid_effects_without_silently_dropping_tokens() {
    for value in [
        "transition:width -1s",
        "transition:width 1s 2s 3s",
        "transition-duration:1",
        "transition:width 1s steps(1,jump-none)",
        "transition:width 1s cubic-bezier(-.1,0,1,1)",
        "transition-property:none,color",
        "box-shadow:1px",
        "box-shadow:0 0 -1px red",
        "box-shadow:0 0 red blue",
        "background-image:linear-gradient(red)",
        "background-image:linear-gradient(in srgb longer hue,red,blue)",
        "background-image:radial-gradient(circle 50%,red,blue)",
    ] {
        assert!(
            Stylesheet::parse(&format!("div{{{value}}}")).is_err(),
            "accepted {value}"
        );
    }
}
#[test]
fn parses_nested_commas_layers_stops_hints_and_wide_gamut_gradients() {
    for image in [
        "linear-gradient(to top right in oklch longer hue, rgb(255,0,0) 10% 20%, 60%, color(display-p3 0 1 0))",
        "radial-gradient(ellipse closest-corner at 20% 30%,red,transparent)",
        "repeating-radial-gradient(circle 20px,red 0 5px,blue 5px 10px)",
        "conic-gradient(from 45deg at right bottom,red 0deg 90deg,blue 25% 100%)",
    ] {
        let css = format!(
            "div{{background:{image},linear-gradient(red,blue) #123;box-shadow:0 2px 3px rgb(0,0,0),inset 0 0 2px currentColor}}"
        );
        let (tree, _) = tree(&css);
        let style = tree.paint_style(tree.root().unwrap());
        assert_eq!(style.background_image.len(), 2);
        assert_eq!(style.box_shadow.len(), 2);
    }
}

#[test]
fn a_newly_displayed_element_has_no_before_change_style() {
    let (mut a, t) = tree(
        ":root{display:none;width:100px;transition:width 1s linear}:root.active{display:block;width:200px}",
    );
    activate(&mut a);
    a.update_styles(t);
    close(width(&a), 200.0);
    assert!(a.next_animation_frame(t).is_none());
}
#[test]
fn shorthand_matching_and_inline_effect_setters() {
    use voidui::style::{
        CssValue,
        gradient::Gradient,
        shadow::BoxShadow,
        transition::{Transition, TransitionProperty},
    };
    let root = div()
        .background_image("linear-gradient(red,blue)".parse::<Gradient>().unwrap())
        .box_shadow(BoxShadow::new(
            0.0,
            2.0,
            4.0,
            0.0,
            "black".parse::<Color>().unwrap(),
        ))
        .transition(Transition::new("padding", Duration::from_secs(1)).easing(Easing::Linear))
        .transition_property([TransitionProperty::from("padding")])
        .transition_duration([1.0])
        .transition_delay([0.0])
        .transition_timing_function([Easing::Linear]);
    let mut a = WidgetTree::new();
    a.build_root(root);
    a.set_stylesheets(vec![
        Stylesheet::parse("div{padding:10px}div.active{padding:20px}").unwrap(),
    ]);
    let t = Instant::now();
    a.update_styles(t);
    activate(&mut a);
    a.update_styles(t);
    a.update_styles(t + Duration::from_millis(500));
    assert_eq!(
        a.layout_style(a.root().unwrap()).padding.top,
        voidui::core::layout::LengthPercentage::length(15.0)
    );
    let _ = div()
        .background_image(CssValue::Inherit)
        .box_shadow(CssValue::Initial);
}
#[test]
fn opaque_alpha_and_transparent_channels_do_not_leak_during_color_interpolation() {
    let a = "rgb(255 0 0 / 0)".parse::<Color>().unwrap();
    let b = "blue".parse::<Color>().unwrap();
    let [r, g, b, alpha] = a
        .interpolate(b, 0.5, ColorSpace::Srgb, Default::default())
        .components()
        .to_alpha_color::<color::Srgb>()
        .components;
    close(r, 0.0);
    close(g, 0.0);
    close(b, 1.0);
    close(alpha, 0.5);
}

#[test]
fn linear_function_easing_supports_holds_discontinuities_and_overshoot() {
    let (mut a, t) = tree(
        ":root{width:100px;transition:width 1s linear(0,0 25%,1 75%,1)}:root.active{width:200px}",
    );
    activate(&mut a);
    a.update_styles(t);
    a.update_styles(t + Duration::from_millis(250));
    close(width(&a), 100.0);
    a.update_styles(t + Duration::from_millis(500));
    close(width(&a), 150.0);
    a.update_styles(t + Duration::from_millis(750));
    close(width(&a), 200.0);
}
#[test]
fn srgb_gamut_mapping_preserves_in_gamut_values_and_maps_extreme_lightness() {
    use voidui::style::color::map_to_srgb;
    let original = "color(srgb .123 .456 .789 / .42)"
        .parse::<Color>()
        .unwrap()
        .components();
    assert_eq!(map_to_srgb(original), original.components);
    assert_eq!(
        map_to_srgb(
            "oklch(120% .2 20 / .5)"
                .parse::<Color>()
                .unwrap()
                .components()
        ),
        [1.0, 1.0, 1.0, 0.5]
    );
    assert_eq!(
        map_to_srgb(
            "oklch(-20% .2 20 / .5)"
                .parse::<Color>()
                .unwrap()
                .components()
        ),
        [0.0, 0.0, 0.0, 0.5]
    );
    let original = "color(display-p3 1 0 0)"
        .parse::<Color>()
        .unwrap()
        .components();
    let mapped = map_to_srgb(original);
    assert!(mapped.iter().all(|v| (0.0..=1.0).contains(v)));
    assert!(
        mapped[1] > 0.0 || mapped[2] > 0.0,
        "mapping must not simply clip P3 red"
    );
}

#[test]
fn animated_unitless_line_height_remains_relative_when_inherited() {
    let (mut a, t) = tree(
        ":root{font-size:10px;line-height:1;transition:line-height 1s linear}:root>div{font-size:20px}:root.active{line-height:2}",
    );
    activate(&mut a);
    a.update_styles(t);
    a.update_styles(t + Duration::from_millis(500));
    let root = a.root().unwrap();
    let child = a.children(root)[0];
    close(
        a.text_style(child)
            .line_height
            .resolve(a.text_style(child).font_size),
        30.0,
    );
}

#[test]
fn step_transitions_schedule_the_next_change_instead_of_continuous_frames() {
    let (mut a, t) =
        tree(":root{width:100px;transition:width 1s steps(4,end)}:root.active{width:200px}");
    activate(&mut a);
    a.update_styles(t);
    assert_eq!(
        a.next_animation_frame(t),
        Some(t + Duration::from_millis(250))
    );
    a.update_styles(t + Duration::from_millis(250));
    close(width(&a), 125.0);
    assert_eq!(
        a.next_animation_frame(t + Duration::from_millis(250)),
        Some(t + Duration::from_millis(500))
    );
}

#[test]
fn sampled_geometry_can_be_laid_out_without_reading_the_wall_clock_again() {
    let (mut a, t) =
        tree(":root{width:100px;height:20px;transition:width 1s linear}:root.active{width:200px}");
    activate(&mut a);
    a.update_styles(t);
    a.update_styles(t + Duration::from_millis(500));
    a.layout_computed(Size::MAX_CONTENT, &cache());
    close(a.bounds(a.root().unwrap()).size.width, 150.0);
}
