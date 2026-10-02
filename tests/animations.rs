//! CSS animation semantics use an injected clock: no GPU or sleeps are required.
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use voidui::{
    core::{
        layout::{Size, TaffyMaxContent},
        widget_tree::WidgetTree,
    },
    div,
    render::{ParleyTextSystem, TextLayoutCache, TextSystem},
    style::{
        color::{Color, ColorSpace},
        css::Stylesheet,
    },
};
fn setup(css: &str) -> (WidgetTree, Instant) {
    let mut tree = WidgetTree::new();
    tree.build_root(div().child(div()));
    tree.set_stylesheets(vec![Stylesheet::parse(css).unwrap()]);
    let t = Instant::now();
    tree.update_styles(t);
    tree.layout_computed(
        Size::MAX_CONTENT,
        &TextLayoutCache::new(Arc::new(TextSystem::new(Arc::new(
            ParleyTextSystem::new_without_system_fonts("unused"),
        )))),
    );
    (tree, t)
}
fn sample(tree: &mut WidgetTree, t: Instant, seconds: f64) {
    tree.update_styles(t + Duration::from_secs_f64(seconds));
}
fn width(tree: &WidgetTree) -> f32 {
    tree.layout_style(tree.root().unwrap()).size.width.value()
}
fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 0.005, "{a} != {b}");
}
const MOVE: &str = "@keyframes move { from {width:0px} to {width:100px} }";
#[test]
fn starts_on_first_style_and_returns_to_underlying_then_idles() {
    let (mut tree, t) = setup(&format!(
        "{MOVE} :root{{width:40px;animation:move 1s linear}}"
    ));
    close(width(&tree), 0.0);
    assert_eq!(tree.next_animation_frame(t), Some(t));
    sample(&mut tree, t, 0.5);
    close(width(&tree), 50.0);
    sample(&mut tree, t, 1.0);
    close(width(&tree), 40.0);
    assert_eq!(tree.next_animation_frame(t + Duration::from_secs(1)), None);
    let resolutions = tree.style_resolutions();
    sample(&mut tree, t, 2.0);
    assert_eq!(resolutions, tree.style_resolutions());
}
#[test]
fn directions_and_fractional_iterations_finish_at_correct_endpoint() {
    for (direction, half, end) in [
        ("normal", 50., 50.),
        ("reverse", 50., 50.),
        ("alternate", 50., 50.),
        ("alternate-reverse", 50., 50.),
    ] {
        let (mut tree, t) = setup(&format!(
            "{MOVE} :root{{animation:move 1s linear 2.5 {direction} forwards}}"
        ));
        sample(&mut tree, t, 0.5);
        close(width(&tree), half);
        sample(&mut tree, t, 2.5);
        close(width(&tree), end);
        assert_eq!(tree.next_animation_frame(t + Duration::from_secs(3)), None);
    }
    for (direction, start, end) in [
        ("normal", 0., 100.),
        ("reverse", 100., 0.),
        ("alternate", 0., 0.),
        ("alternate-reverse", 100., 100.),
    ] {
        let (mut tree, t) = setup(&format!(
            "{MOVE} :root{{animation:move 1s linear 2 {direction} both}}"
        ));
        close(width(&tree), start);
        sample(&mut tree, t, 2.0);
        close(width(&tree), end);
    }
}
#[test]
fn delays_fill_and_zero_duration() {
    for (fill, start, end) in [
        ("none", 40., 40.),
        ("backwards", 0., 40.),
        ("forwards", 40., 100.),
        ("both", 0., 100.),
    ] {
        let (mut tree, t) = setup(&format!(
            "{MOVE} :root{{width:40px;animation:move 1s linear 2s {fill}}}"
        ));
        close(width(&tree), start);
        assert_eq!(
            tree.next_animation_frame(t),
            Some(t + Duration::from_secs(2))
        );
        sample(&mut tree, t, 2.5);
        close(width(&tree), 50.);
        sample(&mut tree, t, 3.);
        close(width(&tree), end);
    }
    let (tree, _) = setup(&format!("{MOVE} :root{{animation:move 1s linear -250ms}}"));
    close(width(&tree), 25.);
    for count in ["1", "infinite"] {
        let (tree, t) = setup(&format!(
            "{MOVE} :root{{animation:move 0s {count} forwards}}"
        ));
        close(width(&tree), 100.);
        assert_eq!(tree.next_animation_frame(t), None);
    }
    let (tree, t) = setup(&format!(
        "{MOVE} :root{{animation:move 1s 0 reverse forwards}}"
    ));
    close(width(&tree), 100.);
    assert_eq!(tree.next_animation_frame(t), None);
}
#[test]
fn pause_resume_including_delay_does_not_count_paused_time() {
    let (mut tree, t) = setup(&format!(
        "{MOVE} :root{{animation:move 1s linear 1s both}} .paused{{animation-play-state:paused}}"
    ));
    let root = tree.root().unwrap();
    tree.set_classes(root, "paused");
    sample(&mut tree, t, 0.5);
    assert_eq!(tree.next_animation_frame(t + Duration::from_secs(1)), None);
    sample(&mut tree, t, 10.);
    close(width(&tree), 0.);
    tree.set_classes(root, "");
    sample(&mut tree, t, 10.);
    sample(&mut tree, t, 10.75);
    close(width(&tree), 25.);
    tree.set_classes(root, "paused");
    sample(&mut tree, t, 10.75);
    sample(&mut tree, t, 20.);
    close(width(&tree), 25.);
    tree.set_classes(root, "");
    sample(&mut tree, t, 20.);
    sample(&mut tree, t, 20.5);
    close(width(&tree), 75.);
}
#[test]
fn important_and_inheritance_and_paint_only_frames() {
    let (mut tree, t) = setup(
        "@keyframes glow { from {color:red;border-radius:0px} to {color:blue;border-radius:20px} } :root{animation:glow 1s linear forwards;border-radius:7px!important}",
    );
    let stats = tree.cascade_stats();
    let change = tree.update_styles(t + Duration::from_millis(500));
    assert!(change.paint);
    assert!(!change.layout);
    assert_eq!(stats, tree.cascade_stats());
    let root = tree.root().unwrap();
    let color = tree.text_style(root).color;
    assert_eq!(color, tree.text_style(tree.children(root)[0]).color);
    assert_eq!(tree.paint_style(root).border_radius, 7.);
    assert_eq!(
        color,
        "red".parse::<Color>().unwrap().interpolate(
            "blue".parse().unwrap(),
            0.5,
            ColorSpace::Oklab,
            Default::default()
        )
    );
}
#[test]
fn duplicate_offsets_missing_endpoints_and_per_property_tracks() {
    let (mut tree, t) = setup(
        "@keyframes move { 50%{width:60px} 50%{width:100px;color:red} to{color:blue} } :root{width:20px;animation:move 1s linear both}",
    );
    close(width(&tree), 20.);
    sample(&mut tree, t, 0.25);
    close(width(&tree), 60.);
    sample(&mut tree, t, 0.5);
    close(width(&tree), 100.);
    sample(&mut tree, t, 0.75);
    close(width(&tree), 60.);
    sample(&mut tree, t, 1.);
    close(width(&tree), 20.);
}
#[test]
fn list_order_repeated_names_and_timing_updates_preserve_playback() {
    let (mut tree, t) = setup(&format!(
        "{MOVE} :root{{animation:move 2s linear forwards}} .twice{{animation-name:move,move;animation-duration:4s,2s}} .slow{{animation-duration:4s}}"
    ));
    sample(&mut tree, t, 0.5);
    close(width(&tree), 25.);
    let root = tree.root().unwrap();
    tree.set_classes(root, "twice");
    sample(&mut tree, t, 0.5);
    close(width(&tree), 25.);
    // The last old name maps to the last new name; it is not restarted.
    sample(&mut tree, t, 1.);
    close(width(&tree), 50.);
    tree.set_classes(root, "slow");
    sample(&mut tree, t, 1.);
    close(width(&tree), 25.);
}
#[test]
fn sheets_reload_and_late_definitions_preserve_or_start_time() {
    let (mut tree, t) = setup(":root{animation:move 1s linear forwards}");
    assert_eq!(tree.next_animation_frame(t), None);
    tree.set_stylesheets(vec![
        Stylesheet::parse(&format!(
            "{MOVE} :root{{animation:move 1s linear forwards}} "
        ))
        .unwrap(),
    ]);
    sample(&mut tree, t, 1.);
    sample(&mut tree, t, 1.25);
    close(width(&tree), 25.);
    tree.set_stylesheets(vec![Stylesheet::parse("@keyframes move{from{width:0px}to{width:200px}} :root{animation:move 1s linear forwards}").unwrap()]);
    sample(&mut tree, t, 1.25);
    close(width(&tree), 50.);
}
#[test]
fn keyframes_last_definition_wins_and_names_are_case_sensitive() {
    let (mut tree, t) = setup(
        "@keyframes Move{to{width:100px}} @keyframes Move{to{width:200px}} @keyframes move{to{width:300px}} :root{width:0px;animation:Move 1s linear forwards}",
    );
    sample(&mut tree, t, 1.);
    close(width(&tree), 200.);
}
#[test]
fn variables_relative_units_and_keyframe_easing() {
    let (mut tree, t) = setup(
        "@keyframes move{from{width:0px;animation-timing-function:steps(2,end)}to{width:var(--end)}} :root{--end:10em;font-size:10px;animation:move 1s linear forwards} .wide{--end:20em}",
    );
    sample(&mut tree, t, 0.25);
    close(width(&tree), 0.);
    sample(&mut tree, t, 0.75);
    close(width(&tree), 50.);
    let root = tree.root().unwrap();
    tree.set_classes(root, "wide");
    sample(&mut tree, t, 0.75);
    close(width(&tree), 100.);
}
#[test]
fn display_none_cancels_descendants_and_show_restarts() {
    let (mut tree, t) = setup(&format!(
        "{MOVE} div{{animation:move 1s linear both}} .hidden{{display:none}}"
    ));
    sample(&mut tree, t, 0.5);
    let root = tree.root().unwrap();
    tree.set_classes(root, "hidden");
    sample(&mut tree, t, 0.5);
    assert_eq!(tree.next_animation_frame(t), None);
    tree.set_classes(root, "");
    sample(&mut tree, t, 2.);
    close(width(&tree), 0.);
    close(
        tree.layout_style(tree.children(root)[0]).size.width.value(),
        0.,
    );
}
#[test]
fn keyframe_motion_never_starts_declared_transitions() {
    let (mut tree, t) = setup(&format!(
        "{MOVE} :root{{animation:move 1s linear;transition:width 5s linear;width:40px}} .other{{color:red}}"
    ));
    sample(&mut tree, t, 0.5);
    let root = tree.root().unwrap();
    tree.set_classes(root, "other");
    sample(&mut tree, t, 0.5);
    close(width(&tree), 50.);
    sample(&mut tree, t, 1.);
    close(width(&tree), 40.);
    assert_eq!(tree.next_animation_frame(t + Duration::from_secs(1)), None);
}
#[test]
fn invalid_grammar_rejected_without_partial_sheet() {
    for css in [
        "@keyframes none{to{width:1px}}",
        "@keyframes a{101%{width:1px}}",
        "@keyframes a{0{width:1px}}",
        "div{animation:a -1s}",
        "div{animation:a 1s 2s 3s}",
        "div{animation-iteration-count:-1}",
        "div{animation-direction:sideways}",
        "div{animation-name:initial, a}",
        "div{animation:a b}",
    ] {
        assert!(Stylesheet::parse(css).is_err(), "accepted {css}");
    }
    assert!(Stylesheet::parse("@keyframes \"none\"{from,to{width:20px!important}} div{animation:1s \"none\", 2s ease ease}").is_ok());
}

#[test]
fn missed_end_deadline_still_requests_final_sample() {
    let (mut tree, t) = setup(&format!(
        "{MOVE} :root{{width:40px;animation:move 1s linear}}"
    ));
    sample(&mut tree, t, 0.99);
    let end = t + Duration::from_secs(2);
    assert_eq!(tree.next_animation_frame(end), Some(end));
    sample(&mut tree, t, 2.);
    close(width(&tree), 40.);
    assert_eq!(tree.next_animation_frame(end), None);
}
#[test]
fn finite_fill_is_recomputed_on_underlying_style_changes() {
    let (mut tree, t) = setup(
        "@keyframes pulse{50%{width:100px}} :root{width:20px;animation:pulse 1s linear forwards} .wide{width:40px}",
    );
    sample(&mut tree, t, 1.);
    close(width(&tree), 20.);
    let root = tree.root().unwrap();
    tree.set_classes(root, "wide");
    sample(&mut tree, t, 2.);
    close(width(&tree), 40.);
    assert_eq!(tree.next_animation_frame(t + Duration::from_secs(2)), None);
}
#[test]
fn fluent_longhands_share_css_cascade_and_inherit_on_request() {
    use voidui::style::{
        CssValue,
        animation::{AnimationName, AnimationStyle},
        transition::Easing,
    };
    let mut tree = WidgetTree::new();
    tree.build_root(
        div()
            .animation(AnimationStyle {
                names: [AnimationName::from("move")].into(),
                durations: [1.].into(),
                easing: [Easing::Linear].into(),
                ..Default::default()
            })
            .child(
                div()
                    .animation_name(CssValue::Inherit)
                    .animation_duration(CssValue::Inherit)
                    .animation_timing_function(CssValue::Inherit),
            ),
    );
    tree.set_stylesheets(vec![Stylesheet::parse(MOVE).unwrap()]);
    let t = Instant::now();
    tree.update_styles(t);
    sample(&mut tree, t, 0.5);
    close(width(&tree), 50.);
    let root = tree.root().unwrap();
    close(
        tree.layout_style(tree.children(root)[0]).size.width.value(),
        50.,
    );
}
#[test]
fn existing_transition_has_priority_over_new_keyframes() {
    let (mut tree, t) = setup(&format!(
        "{MOVE} :root{{width:0px;transition:width 1s linear}} .changed{{width:200px}} .moving{{animation:move 1s linear forwards}}"
    ));
    let root = tree.root().unwrap();
    tree.set_classes(root, "changed");
    sample(&mut tree, t, 0.);
    tree.set_classes(root, "changed moving");
    sample(&mut tree, t, 0.25);
    close(width(&tree), 50.);
    sample(&mut tree, t, 0.5);
    close(width(&tree), 100.);
    sample(&mut tree, t, 1.1);
    close(width(&tree), 85.);
}

#[test]
fn transform_keyframes_refresh_visual_coordinates_without_layout() {
    use voidui::core::geometry::Point;
    let (mut tree, t) = setup(
        "@keyframes turn{to{transform:rotate(360deg)}} :root{width:100px;height:100px;animation:turn 1s linear infinite}",
    );
    let change = tree.update_styles(t + Duration::from_millis(500));
    assert!(change.paint);
    assert!(!change.layout);
    let local = tree
        .window_to_local(tree.root().unwrap(), Point::new(0., 0.))
        .unwrap();
    close(local.x, 100.);
    close(local.y, 100.);
}
#[test]
fn empty_or_unrepresentable_tracks_do_not_schedule_frames() {
    for frames in [
        "@keyframes move{}",
        "@keyframes move{from{width:auto}to{width:100px}}",
    ] {
        let (tree, t) = setup(&format!(
            "{frames} :root{{width:20px;animation:move 1s linear infinite}}"
        ));
        close(width(&tree), 20.);
        assert_eq!(tree.next_animation_frame(t), None);
    }
}
#[test]
fn keyframe_important_is_ignored_and_wide_keywords_reset_shorthand() {
    let (mut tree, t) = setup(
        "@keyframes move{from{width:0px!important}to{width:100px}} :root{width:20px;animation:move 1s linear forwards}.reset{animation:initial}",
    );
    sample(&mut tree, t, 0.5);
    close(width(&tree), 60.);
    let root = tree.root().unwrap();
    tree.set_classes(root, "reset");
    sample(&mut tree, t, 0.5);
    close(width(&tree), 20.);
    assert_eq!(tree.next_animation_frame(t), None);
}

#[test]
fn easing_on_an_implicit_start_and_transition_removal() {
    let (mut tree, t) = setup(
        "@keyframes move{from{animation-timing-function:steps(2,end)}to{width:100px}} :root{width:20px;animation:move 1s linear forwards}",
    );
    sample(&mut tree, t, 0.25);
    close(width(&tree), 20.);
    sample(&mut tree, t, 0.75);
    close(width(&tree), 60.);
    let (mut tree, t) = setup(&format!(
        "{MOVE} :root{{width:0px;transition:width 1s linear}} .changed{{width:200px}} .moving{{animation:move 1s linear forwards;transition:none}}"
    ));
    let root = tree.root().unwrap();
    tree.set_classes(root, "changed");
    sample(&mut tree, t, 0.);
    tree.set_classes(root, "changed moving");
    sample(&mut tree, t, 0.25);
    close(width(&tree), 0.);
    sample(&mut tree, t, 0.5);
    close(width(&tree), 25.);
}
