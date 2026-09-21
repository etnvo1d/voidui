//! Allocation checks run on the calling thread, excluding font loading and setup.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    sync::Arc,
};
use voidui_gpui_wgpu::*;

struct CountingAllocator;
thread_local! {
    static TRACKING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

fn record() {
    if TRACKING.try_with(Cell::get).unwrap_or(false) {
        let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
    }
}

// Delegate every pointer operation unchanged to System. Only thread-local
// counters are touched, so other tests and worker threads cannot skew results.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record();
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record();
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record();
        unsafe { System.realloc(ptr, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[test]
fn unchanged_width_reflow_allocates_nothing_for_uniform_mixed_and_empty_paragraphs() {
    let backend = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    backend
        .add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!(
            "fonts/IBMPlexSans-Regular.ttf"
        ))])
        .unwrap();
    let system = TextSystem::new(Arc::new(backend));
    for width in [None, Some(80.0)] {
        for mixed in [false, true] {
            let runs: Vec<_> = [24.0, 48.0, 20.0]
                .into_iter()
                .map(|height| TextRun {
                    len: 2,
                    font: font("IBM Plex Sans"),
                    line_height: mixed.then_some(height),
                    ..Default::default()
                })
                .collect();
            for clamp in [None, Some(0)] {
                let mut p = system
                    .shape_paragraph("a\nb\nc\n".into(), &runs, 16.0, 24.0, width, clamp)
                    .unwrap();
                let expected_height = p.height();
                ALLOCATIONS.with(|count| count.set(0));
                TRACKING.with(|tracking| tracking.set(true));
                for _ in 0..1000 {
                    p.reflow(width);
                }
                TRACKING.with(|tracking| tracking.set(false));
                assert_eq!(ALLOCATIONS.with(Cell::get), 0);
                assert_eq!(p.height(), expected_height);
            }
        }
        let mut empty = system
            .shape_paragraph("".into(), &[], 16.0, 24.0, width, None)
            .unwrap();
        ALLOCATIONS.with(|count| count.set(0));
        TRACKING.with(|tracking| tracking.set(true));
        for _ in 0..1000 {
            empty.reflow(width);
        }
        TRACKING.with(|tracking| tracking.set(false));
        assert_eq!(ALLOCATIONS.with(Cell::get), 0);
    }
}
