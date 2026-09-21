//! Warm pointer dispatch benchmark; excludes native delivery and rendering.
#[path = "support/allocations.rs"]
mod allocations;
use allocations::ALLOCATIONS;
#[path = "support/args.rs"]
mod arguments;

use std::{
    cell::Cell,
    rc::Rc,
    sync::{Arc, atomic::Ordering::Relaxed},
    time::Instant,
};
use voidui::{
    core::{
        geometry::Point,
        layout::{AvailableSpace, Size},
        widget_tree::WidgetTree,
    },
    div,
    render::{ParleyTextSystem, TextLayoutCache, TextSystem},
};
fn main() {
    let samples: usize = arguments::args()
        .next()
        .map(|value| value.parse().expect("samples must be an integer"))
        .unwrap_or(100_000);
    assert!(samples > 0);
    let count = Rc::new(Cell::new(0));
    let output = count.clone();
    let mut tree = WidgetTree::new();
    tree.build_root(
        div()
            .width(400.0)
            .height(200.0)
            .on_mouse_move(move || output.set(output.get() + 1))
            .on_drag(|| {}),
    );
    let cache = TextLayoutCache::new(Arc::new(TextSystem::new(Arc::new(
        ParleyTextSystem::new_without_system_fonts("unused"),
    ))));
    tree.layout(
        Size {
            width: AvailableSpace::Definite(400.0),
            height: AvailableSpace::Definite(200.0),
        },
        &cache,
    );
    tree.pointer_moved(Some(Point::new(20.0, 20.0)));
    for captured in [false, true] {
        if captured {
            tree.pointer_pressed(true);
        }
        let before = ALLOCATIONS.load(Relaxed);
        let start = Instant::now();
        for index in 0..samples {
            tree.pointer_moved(Some(Point::new(20.0 + (index % 2) as f32, 20.0)));
        }
        let elapsed = start.elapsed();
        let allocations = ALLOCATIONS.load(Relaxed) - before;
        assert_eq!(allocations, 0, "warm same-target movement allocated");
        println!(
            "samples={samples} captured={captured} ns_per_move={:.2} allocations={allocations}",
            elapsed.as_secs_f64() * 1e9 / samples as f64
        );
    }
    tree.pointer_pressed(false);
    assert_eq!(count.get(), samples * 2 + 1);
    assert_eq!(tree.task_runtime().stats().spawned, 0);
}
