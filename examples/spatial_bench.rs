//! CPU-only spatial benchmark. Run with --release; arguments are rows and samples.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering::Relaxed},
    },
    time::Instant,
};
use voidui::{
    core::{
        geometry::Point,
        layout::{AvailableSpace, Size},
        widget_tree::WidgetTree,
    },
    render::{ParleyTextSystem, TextLayoutCache, TextSystem},
    *,
};
struct Allocator;
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
// Requested allocation bytes exclude allocator metadata, GPU memory, and RSS.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            ALLOCATIONS.fetch_add(1, Relaxed);
            LIVE.fetch_add(l.size(), Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size(), Relaxed);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        let p = unsafe { System.realloc(p, l, n) };
        if !p.is_null() {
            ALLOCATIONS.fetch_add(1, Relaxed);
            LIVE.fetch_sub(l.size(), Relaxed);
            LIVE.fetch_add(n, Relaxed);
        }
        p
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let rows: usize = args.first().map(|s| s.parse().unwrap()).unwrap_or(1000);
    let samples: usize = args.get(1).map(|s| s.parse().unwrap()).unwrap_or(1000);
    assert!(rows > 1 && samples > 0);
    let cache = TextLayoutCache::new(Arc::new(TextSystem::new(Arc::new(
        ParleyTextSystem::new_without_system_fonts("unused"),
    ))));
    for transformed in [false, true] {
        let before = LIVE.load(Relaxed);
        let mut t = WidgetTree::new();
        let mut pane = div()
            .id("pane")
            .size(400, 200)
            .overflow(Overflow::Auto)
            .child(div().id("header").size(400, 20).sticky().top(0))
            .children((0..rows).map(|_| div().size(400, 20)));
        if transformed {
            pane = pane.transform(
                "translate(10px,10px) rotate(2deg)"
                    .parse::<Transform>()
                    .unwrap(),
            );
        }
        t.build_root(div().size(600, 400).child(pane));
        t.layout(
            Size {
                width: AvailableSpace::Definite(600.),
                height: AvailableSpace::Definite(400.),
            },
            &cache,
        );
        let p = t.find_by_id("pane").unwrap();
        t.hit_test(Point::new(50., 50.));
        let retained = LIVE.load(Relaxed).saturating_sub(before);
        let order = t.paint_order_rebuilds();
        let allocations = ALLOCATIONS.load(Relaxed);
        let time = Instant::now();
        for i in 0..samples {
            t.scroll_to(p, Point::new(0., (i % 100) as f32));
            t.hit_test(Point::new(50., 50.));
        }
        let us = time.elapsed().as_secs_f64() * 1e6 / samples as f64;
        let scroll_alloc = ALLOCATIONS.load(Relaxed) - allocations;
        assert_eq!(t.paint_order_rebuilds(), order);
        assert!(!t.update_styles(Instant::now()).layout);
        let allocations = ALLOCATIONS.load(Relaxed);
        let time = Instant::now();
        for _ in 0..samples {
            std::hint::black_box(t.hit_test(Point::new(50., 50.)));
        }
        let hit_us = time.elapsed().as_secs_f64() * 1e6 / samples as f64;
        let hit_alloc = ALLOCATIONS.load(Relaxed) - allocations;
        assert_eq!(hit_alloc, 0);
        println!(
            "rows={rows} transformed={transformed} retained_bytes={retained} scroll_hit_us={us:.2} scroll_allocations={scroll_alloc} hit_us={hit_us:.2} hit_allocations={hit_alloc}"
        );
    }
}
