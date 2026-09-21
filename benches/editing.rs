//! Reproduce core allocation and retained-paragraph edit costs without a GPU.
#[path = "support/allocations.rs"]
mod allocations;
use allocations::{ALLOCATIONS, LIVE};
#[path = "support/args.rs"]
mod arguments;

use std::{
    borrow::Cow,
    hint::black_box,
    sync::{Arc, atomic::Ordering::Relaxed},
    time::Instant,
};
use voidui::{
    Editor,
    editing::{Edit, EditorLayout, EditorState, HistoryOptions, LayoutOptions, Transaction},
    render::{ParleyTextSystem, TextSystem, font},
};
fn main() {
    let mut args = arguments::args();
    let controls: usize = args.next().map(|s| s.parse().unwrap()).unwrap_or(10000);
    let lines: usize = args.next().map(|s| s.parse().unwrap()).unwrap_or(1000);
    let edits: usize = args.next().map(|s| s.parse().unwrap()).unwrap_or(1000);
    assert!(controls > 0 && lines > 0 && edits > 0);
    let before = LIVE.load(Relaxed);
    let models: Vec<_> = (0..controls).map(|_| Editor::default()).collect();
    let retained = LIVE.load(Relaxed) - before;
    println!(
        "empty_sessions={controls} retained_requested_bytes={retained} bytes_per_session={:.1}",
        retained as f64 / controls as f64
    );
    drop(models);

    let backend = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    backend
        .add_fonts(vec![Cow::Borrowed(include_bytes!(
            "../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
        ))])
        .unwrap();
    let system = Arc::new(TextSystem::new(Arc::new(backend)));
    let mut state = EditorState::new(
        (0..lines)
            .map(|i| format!("Line {i}: retained paragraph editing.\n"))
            .collect::<String>(),
    );
    state.set_history_options(HistoryOptions {
        max_bytes: 0,
        ..Default::default()
    });
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
            state.snapshot(),
            voidui::editing::Projection::new(),
            options.clone(),
            system.clone(),
            Some(voidui::core::geometry::Rect::from_xywh(
                0.0, 0.0, 400.0, 600.0,
            )),
            Default::default(),
        )
        .unwrap();
    let initial_time = initial.elapsed();
    let retained = LIVE.load(Relaxed) - before;
    let shapes = system.stats().paragraphs_shaped;
    let allocations = ALLOCATIONS.load(Relaxed);
    let start = Instant::now();
    for i in 0..edits {
        let replacement = if i % 2 == 0 { "l" } else { "L" };
        state
            .transact(Transaction::new(
                state.revision(),
                [Edit::new(0..1, replacement)],
            ))
            .unwrap();
        layout
            .prepare_snapshot(
                state.snapshot(),
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
    let time = start.elapsed();
    let new_shapes = system.stats().paragraphs_shaped - shapes;
    let allocations = ALLOCATIONS.load(Relaxed) - allocations;
    assert_eq!(new_shapes, edits as u64);
    println!(
        "lines={lines} initial_layout={initial_time:?} layout_and_font_requested_bytes={retained}"
    );
    println!(
        "edits={edits} total={time:?} us_per_edit={:.2} reshaped_paragraphs={new_shapes} allocation_calls={allocations}",
        time.as_secs_f64() * 1e6 / edits as f64
    );
}
