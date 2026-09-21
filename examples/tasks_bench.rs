//! CPU-only task bookkeeping benchmark. No threads, native windows, or GPU work.
//! Run: cargo run --release --example tasks_bench -- [tasks] [idle-checks]
use std::{
    alloc::{GlobalAlloc, Layout, System},
    future::pending,
    hint::black_box,
    sync::atomic::{AtomicUsize, Ordering::Relaxed},
    time::{Duration, Instant},
};
use voidui::{TaskOptions, TaskRuntime};

struct CountingAllocator;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
// Count requested bytes, not allocator metadata or RSS. This standalone process
// has no window, GPU, font workers, or other concurrent benchmark workloads.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the unchanged layout is forwarded to the system allocator.
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            LIVE.fetch_add(layout.size(), Relaxed);
            ALLOCATIONS.fetch_add(1, Relaxed);
        }
        pointer
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Relaxed);
        // SAFETY: pointer and layout come from this allocator's matching allocation.
        unsafe { System.dealloc(pointer, layout) };
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        // SAFETY: the original allocation and requested size are forwarded unchanged.
        let next = unsafe { System.realloc(pointer, layout, size) };
        if !next.is_null() {
            LIVE.fetch_add(size, Relaxed);
            LIVE.fetch_sub(layout.size(), Relaxed);
            ALLOCATIONS.fetch_add(1, Relaxed);
        }
        next
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn main() {
    let mut args = std::env::args().skip(1);
    let count: usize = args
        .next()
        .map(|value| value.parse().expect("tasks must be an integer"))
        .unwrap_or(10_000);
    let checks: usize = args
        .next()
        .map(|value| value.parse().expect("idle-checks must be an integer"))
        .unwrap_or(50_000);
    assert!(count > 0 && checks > 0);
    let runtime = TaskRuntime::new(TaskOptions {
        max_tasks: count,
        polls_per_tick: count,
        poll_budget: Duration::from_secs(1),
        ..Default::default()
    })
    .unwrap();
    let scope = runtime.scope();
    let before = LIVE.load(Relaxed);
    let start = Instant::now();
    for _ in 0..count {
        scope.spawn(pending::<()>());
    }
    while runtime.has_ready_tasks() {
        runtime.tick();
    }
    let spawn_time = start.elapsed();
    let retained = LIVE.load(Relaxed) - before;
    let allocations = ALLOCATIONS.load(Relaxed);
    let start = Instant::now();
    for _ in 0..checks {
        black_box(runtime.tick());
    }
    let idle = start.elapsed();
    let idle_allocations = ALLOCATIONS.load(Relaxed) - allocations;
    assert_eq!(idle_allocations, 0);
    assert_eq!(runtime.stats().polls, count as u64);
    assert!(!runtime.stats().background_started);
    let start = Instant::now();
    scope.cancel_all();
    let cancel_time = start.elapsed();
    while runtime.has_ready_tasks() {
        runtime.tick();
    }
    assert_eq!(runtime.stats().active, 0);
    println!(
        "tasks={count} additional_retained_bytes_per_suspended_task={:.1} spawn_and_first_poll_us={:.3}",
        retained as f64 / count as f64,
        spawn_time.as_secs_f64() * 1e6 / count as f64
    );
    println!(
        "idle_check_ns={:.2} idle_allocations={idle_allocations} cancellation_us_per_task={:.3}",
        idle.as_secs_f64() * 1e9 / checks as f64,
        cancel_time.as_secs_f64() * 1e6 / count as f64
    );
    println!(
        "Retained bytes include registry/queue reservations and exclude allocator metadata; results depend on allocator, compiler, and capacity growth."
    );
}
