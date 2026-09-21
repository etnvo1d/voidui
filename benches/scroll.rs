//! Warm retained scrolling benchmark. Usage: scroll_bench [rows] [samples].
//! Measures subtree positioning and hit/clip refresh, excluding layout and GPU work.
#[path = "support/allocations.rs"]
mod allocations;
use allocations::{ALLOCATIONS, LIVE};
#[path = "support/args.rs"]
mod arguments;

use std::{
    sync::{Arc, atomic::Ordering::Relaxed},
    time::Instant,
};
use voidui::{
    Overflow,
    core::{
        geometry::Point,
        layout::{AvailableSpace, Size},
        widget_tree::WidgetTree,
    },
    div,
    render::{ParleyTextSystem, TextLayoutCache, TextSystem},
};
fn main() {
    let mut args = arguments::args();
    let rows = args
        .next()
        .map(|v| v.parse().expect("rows must be an integer"))
        .unwrap_or(10_000usize);
    let samples = args
        .next()
        .map(|v| v.parse().expect("samples must be an integer"))
        .unwrap_or(2_000usize);
    assert!(rows > 0 && samples > 0);
    let mut view = div().width(400).height(400).overflow(Overflow::Auto);
    for _ in 0..rows {
        view = view.child(div().width(300).height(20));
    }
    let cache = TextLayoutCache::new(Arc::new(TextSystem::new(Arc::new(
        ParleyTextSystem::new_without_system_fonts("unused"),
    ))));
    let mut tree = WidgetTree::new();
    tree.build_root(view);
    tree.layout(
        Size {
            width: AvailableSpace::Definite(400.),
            height: AvailableSpace::Definite(400.),
        },
        &cache,
    );
    let root = tree.root().unwrap();
    tree.scroll_to(root, Point::new(0., 1.));
    tree.hit_test(Point::new(10., 10.));
    tree.update_styles(Instant::now());
    let rebuilds = tree.paint_order_rebuilds();
    let live = LIVE.load(Relaxed);
    let allocations = ALLOCATIONS.load(Relaxed);
    let start = Instant::now();
    for index in 0..samples {
        tree.scroll_to(root, Point::new(0., (index % 200) as f32));
        tree.hit_test(Point::new(10., 10.));
        assert!(!tree.update_styles(Instant::now()).layout);
    }
    let elapsed = start.elapsed();
    let allocations = ALLOCATIONS.load(Relaxed) - allocations;
    assert_eq!(allocations, 0, "warm scrolling allocated");
    assert_eq!(
        tree.paint_order_rebuilds(),
        rebuilds,
        "scrolling reordered stacking contexts"
    );
    assert!(tree.next_animation_frame(Instant::now()).is_none());
    println!(
        "rows={rows} samples={samples} us_per_scroll={:.2} warm_allocations={allocations} retained_bytes={live}",
        elapsed.as_secs_f64() * 1e6 / samples as f64
    );
    tree.pointer_moved(Some(Point::new(10., 10.)));
    let wheel = |index: usize| voidui::MouseWheel {
        x: 0.,
        y: if index % 2 == 0 { -1. } else { 1. },
        unit: voidui::WheelUnit::Pixels,
    };
    tree.dispatch_mouse_scroll(wheel(0), voidui::core::event::ModifiersState::empty());
    tree.update_styles(Instant::now());
    let allocations = ALLOCATIONS.load(Relaxed);
    let start = Instant::now();
    for index in 0..samples {
        tree.dispatch_mouse_scroll(wheel(index), voidui::core::event::ModifiersState::empty());
        tree.refresh_pointer();
        assert!(!tree.update_styles(Instant::now()).layout);
    }
    let elapsed = start.elapsed();
    let allocations = ALLOCATIONS.load(Relaxed) - allocations;
    assert_eq!(allocations, 0, "warm wheel dispatch allocated");
    assert_eq!(tree.paint_order_rebuilds(), rebuilds);
    println!(
        "us_per_wheel={:.2} warm_wheel_allocations={allocations}",
        elapsed.as_secs_f64() * 1e6 / samples as f64
    );
    println!(
        "PASS zero warm allocations, no layout/reconciliation, retained paint order, no idle timer"
    );
}
