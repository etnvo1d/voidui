//! CPU/state allocation benchmark; excludes CSS, layout, text shaping and GPU work.
//! Run: cargo run --release --example state_bench -- [rows] [updates]
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::RefCell,
    hint::black_box,
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering::Relaxed},
    time::Instant,
};
use voidui::{State, component, core::widget_tree::WidgetTree, div, state};

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
    let rows: usize = args
        .next()
        .map(|s| s.parse().expect("rows must be an integer"))
        .unwrap_or(10_000);
    let updates: usize = args
        .next()
        .map(|s| s.parse().expect("updates must be an integer"))
        .unwrap_or(10_000);
    assert!(rows > 0 && updates > 0, "rows and updates must be positive");
    let before = LIVE.load(Relaxed);
    let mut plain = WidgetTree::new();
    plain.build_root(div().children((0..rows).map(|_| div().width(0))));
    let plain_bytes = LIVE.load(Relaxed) - before;
    drop(plain);

    let handles: Rc<RefCell<Vec<Option<State<usize>>>>> = Rc::new(RefCell::new(vec![None; rows]));
    let before = LIVE.load(Relaxed);
    let start = Instant::now();
    let mut tree = WidgetTree::new();
    tree.build_root(div().children((0..rows).map(|index| {
        let handles = handles.clone();
        component(move || {
            let value = state(|| 0usize);
            *handles.borrow_mut().get_mut(index).unwrap() = Some(value.clone());
            div().width((value.get() % 100) as f32)
        })
    })));
    let mount = start.elapsed();
    let state_bytes = LIVE.load(Relaxed) - before;
    let selected = handles.borrow()[rows / 2].as_ref().unwrap().clone();
    let selected_id = tree.children(tree.root().unwrap())[rows / 2];
    selected.set(1);
    tree.flush_updates();
    let allocations = ALLOCATIONS.load(Relaxed);
    let start = Instant::now();
    for i in 0..updates {
        selected.set(i);
        assert_eq!(tree.flush_updates(), 1);
    }
    let update_time = start.elapsed();
    let update_allocations = ALLOCATIONS.load(Relaxed) - allocations;
    let start = Instant::now();
    for _ in 0..updates {
        black_box(tree.flush_updates());
    }
    let idle = start.elapsed();
    let start = Instant::now();
    for i in 0..updates {
        selected.set(i);
    }
    assert_eq!(tree.flush_updates(), 1);
    let batch = start.elapsed();
    println!(
        "rows={rows} updates={updates} mount_ms={:.3}",
        mount.as_secs_f64() * 1e3
    );
    println!(
        "additional_retained_bytes_per_component={:.1} (same DOM; one usize state; excludes caller handle array)",
        (state_bytes - plain_bytes) as f64 / rows as f64
    );
    println!(
        "local_update_us={:.3} idle_check_ns={:.2} batched_write_ns={:.2} update_allocations={update_allocations}",
        update_time.as_secs_f64() * 1e6 / updates as f64,
        idle.as_secs_f64() * 1e9 / updates as f64,
        batch.as_secs_f64() * 1e9 / updates as f64
    );
    assert_eq!(tree.children(tree.root().unwrap())[rows / 2], selected_id);
    assert_eq!(tree.component_count(), rows);
    assert_eq!(tree.state_count(), rows);
    assert_eq!(
        update_allocations, 0,
        "a warmed, value-only div update should not allocate"
    );
    println!(
        "PASS each local update rendered one component, equal DOM IDs retained, zero warm update allocations"
    );
    let start = Instant::now();
    tree.reconcile_root(div());
    let unmount = start.elapsed();
    assert_eq!(tree.component_count(), 0);
    assert!(!selected.is_mounted());
    println!("bulk_unmount_ms={:.3}", unmount.as_secs_f64() * 1e3);

    let output: Rc<RefCell<Option<State<usize>>>> = Rc::default();
    let slot = output.clone();
    let start = Instant::now();
    tree.build_root(component(move || {
        let value = state(|| 0usize);
        *slot.borrow_mut() = Some(value.clone());
        div().children((0..rows).map(|_| {
            let value = value.clone();
            component(move || div().width((value.get() % 100) as f32))
        }))
    }));
    let mount = start.elapsed();
    let shared = output.borrow().as_ref().unwrap().clone();
    shared.set(1);
    assert_eq!(tree.flush_updates(), rows);
    let start = Instant::now();
    shared.set(2);
    assert_eq!(tree.flush_updates(), rows);
    println!(
        "shared_state_mount_ms={:.3} fanout_update_ms={:.3} readers={rows}",
        mount.as_secs_f64() * 1e3,
        start.elapsed().as_secs_f64() * 1e3
    );

    // Each row reads a distinct key. Updating one record must not scan or render
    // unrelated rows, unlike a shared State<HashMap<...>> subscription.
    let output = Rc::new(RefCell::new(None));
    let slot = output.clone();
    tree.build_root(component(move || {
        let values = voidui::store(|| (0..rows).map(|key| (key, 0usize)));
        *slot.borrow_mut() = Some(values);
        div().children(
            (0..rows).map(|key| {
                component(move || div().width(values.with(&key, |v| *v.unwrap()) as f32))
            }),
        )
    }));
    let values = output.borrow().unwrap();
    values.insert(rows / 2, 1);
    assert_eq!(tree.flush_updates(), 1);
    let allocations = ALLOCATIONS.load(Relaxed);
    let start = Instant::now();
    for i in 0..updates {
        values.update(&(rows / 2), |value| *value = i % 100);
        assert_eq!(tree.flush_updates(), 1);
    }
    let elapsed = start.elapsed();
    let allocated = ALLOCATIONS.load(Relaxed) - allocations;
    assert_eq!(allocated, 0, "unshared keyed edits should not allocate");
    println!(
        "keyed_store_update_us={:.3} readers={rows} rendered_per_update=1 update_allocations={allocated}",
        elapsed.as_secs_f64() * 1e6 / updates as f64
    );
}
