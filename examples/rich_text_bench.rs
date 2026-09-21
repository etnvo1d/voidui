//! Measure sparse rich formatting and retained layout without a window or GPU.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    borrow::Cow,
    hint::black_box,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering::Relaxed},
    },
    time::Instant,
};
use voidui::{
    Editor, InlineStyle, RichText, StyleSpan,
    editing::{
        Edit, EditorLayout, EditorState, HistoryOptions, LayoutOptions, StylePatch, Transaction,
    },
    render::{ParleyTextSystem, TextSystem, font},
};
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
    let args: Vec<_> = std::env::args().skip(1).collect();
    let lines: usize = args.first().map(|s| s.parse().unwrap()).unwrap_or(1000);
    let edits: usize = args.get(1).map(|s| s.parse().unwrap()).unwrap_or(500);
    assert!(lines > 0 && edits > 0);
    let baseline = LIVE.load(Relaxed);
    let models: Vec<_> = (0..10000).map(|_| Editor::default()).collect();
    println!(
        "empty_sessions=10000 requested_bytes_per_session={:.1}",
        (LIVE.load(Relaxed) - baseline) as f64 / 10000.0
    );
    drop(models);
    let backend = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    backend
        .add_fonts(vec![Cow::Borrowed(include_bytes!(
            "../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
        ))])
        .unwrap();
    let system = Arc::new(TextSystem::new(Arc::new(backend)));
    let baseline = LIVE.load(Relaxed);
    let mut text = String::new();
    let mut spans = Vec::with_capacity(lines);
    for i in 0..lines {
        let start = text.len();
        text.push_str(&format!("Line {i}: retained rich paragraph editing.\n"));
        spans.push(StyleSpan::new(
            start + 5..start + 5 + i.to_string().len(),
            InlineStyle::new().bold(),
        ));
    }
    let mut editor = EditorState::from_rich(RichText::from_spans(text, spans).unwrap());
    editor.set_history_options(HistoryOptions {
        max_bytes: 0,
        ..Default::default()
    });
    println!(
        "document_bytes={} style_spans={} retained_document_requested_bytes={}",
        editor.document().len(),
        editor.document().spans().len(),
        LIVE.load(Relaxed) - baseline
    );
    let options = LayoutOptions {
        font: font("IBM Plex Sans"),
        font_size: 16.0,
        line_height: 24.0,
        width: Some(400.0),
    };
    let mut layout = EditorLayout::default();
    let before = LIVE.load(Relaxed);
    let initial = Instant::now();
    layout
        .prepare_snapshot(
            editor.snapshot(),
            voidui::editing::Projection::new(),
            options.clone(),
            system.clone(),
            Some(voidui::core::geometry::Rect::from_xywh(
                0.0, 0.0, 400.0, 600.0,
            )),
            Default::default(),
        )
        .unwrap();
    println!(
        "paragraphs={lines} initial_layout={:?} layout_and_font_requested_bytes={}",
        initial.elapsed(),
        LIVE.load(Relaxed) - before
    );
    for kind in ["text", "format"] {
        let start = Instant::now();
        let shaped = system.stats().paragraphs_shaped;
        let allocations = ALLOCATIONS.load(Relaxed);
        for i in 0..edits {
            let tx = if kind == "text" {
                Transaction::new(
                    editor.revision(),
                    [Edit::new(0..1, if i % 2 == 0 { "l" } else { "L" })],
                )
            } else {
                Transaction::new(editor.revision(), [])
                    .format(0..4, StylePatch::new().italic(i % 2 == 0))
            };
            editor.transact(tx).unwrap();
            layout
                .prepare_snapshot(
                    editor.snapshot(),
                    voidui::editing::Projection::new(),
                    options.clone(),
                    system.clone(),
                    Some(voidui::core::geometry::Rect::from_xywh(
                        0.0, 0.0, 400.0, 600.0,
                    )),
                    Default::default(),
                )
                .unwrap();
            black_box(layout.size());
        }
        let elapsed = start.elapsed();
        let shaped = system.stats().paragraphs_shaped - shaped;
        assert_eq!(
            shaped, edits as u64,
            "local edits must only reshape one paragraph"
        );
        println!(
            "kind={kind} edits={edits} total={elapsed:?} us_per_edit={:.2} reshaped_paragraphs={shaped} allocation_calls={}",
            elapsed.as_secs_f64() * 1e6 / edits as f64,
            ALLOCATIONS.load(Relaxed) - allocations
        );
    }
    let shapes = system.stats().paragraphs_shaped;
    let start = Instant::now();
    for i in 0..20 {
        layout.reflow(Some(if i % 2 == 0 { 240.0 } else { 400.0 }));
    }
    assert_eq!(system.stats().paragraphs_shaped, shapes);
    println!(
        "width_changes=20 elapsed={:?} new_shapes=0",
        start.elapsed()
    );
}
