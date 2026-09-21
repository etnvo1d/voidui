//! Headless sidebar resize benchmark: cargo bench --bench resize -- [rows] [frames]
//! Reports reconciliation, style and layout time; excludes native events and painting.
#[path = "support/args.rs"]
mod arguments;

use std::{borrow::Cow, cell::Cell, rc::Rc, sync::Arc, time::Instant};
use voidui::{
    Read, component,
    core::{
        layout::{AvailableSpace, Size},
        widget_tree::WidgetTree,
    },
    div,
    render::{ParleyTextSystem, TextLayoutCache, TextSystem},
    state,
    style::tailwind,
    svg,
    svg::SvgDocument,
    text,
};

#[component(memo)]
fn file_list(names: Read<Vec<String>>, document: Read<SvgDocument>) {
    div()
        .w_max()
        .min_w_full()
        .children(names.iter().map(|name| {
            div().child(
                div()
                    .tag("button")
                    .w_full()
                    .h_6()
                    .pl(8)
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_sm()
                    .text_nowrap()
                    .class("hover:bg-gray-50")
                    .child(svg().document(document.as_ref().clone()).size_3p5())
                    .child(text(name.clone())),
            )
        }))
}

fn main() -> anyhow::Result<()> {
    let mut args = arguments::args();
    let rows = args
        .next()
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(1000usize);
    let frames = args
        .next()
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(200usize);
    anyhow::ensure!(rows > 0 && frames > 0, "rows and frames must be positive");
    let names = Read::new(
        (0..rows)
            .map(|i| format!("dependency-{i:05}-0123456789abcdef"))
            .collect(),
    );
    let document = Read::new(SvgDocument::parse(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 5 H9 L11 7 H21 V20 H3 Z"/></svg>"#,
    )?);
    let control = Rc::new(Cell::new(None));
    let output = control.clone();
    let mut tree = WidgetTree::new();
    tree.build_root(component(move || {
        let width = state(|| 232.0f32);
        output.set(Some(width));
        div().size(1000, 700).flex().font("IBM Plex Sans").child(
            div()
                .width(width.get())
                .h_full()
                .overflow_x_hidden()
                .child(file_list(&names, &document)),
        )
    }));
    tree.set_stylesheets(vec![tailwind::all()?]);
    let backend = Arc::new(ParleyTextSystem::new_without_system_fonts("IBM Plex Sans"));
    backend.add_fonts(vec![Cow::Borrowed(include_bytes!(
        "../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
    ))])?;
    let cache = TextLayoutCache::new(Arc::new(TextSystem::new(backend.clone())));
    let available = Size {
        width: AvailableSpace::Definite(1000.),
        height: AvailableSpace::Definite(700.),
    };
    tree.layout(available, &cache);
    let width = control.get().unwrap();
    let shaped = backend.stats().paragraphs_shaped;
    let resolved = tree.style_resolutions();
    let candidates = tree.cascade_stats().candidate_tests;
    let mut totals = [0.0f64; 3];
    let mut renders = 0;
    for frame in 0..frames {
        width.set(240. + (frame % 80) as f32);
        let start = Instant::now();
        renders += tree.flush_updates();
        totals[0] += start.elapsed().as_secs_f64();
        let start = Instant::now();
        let changes = tree.update_styles(Instant::now());
        totals[1] += start.elapsed().as_secs_f64();
        let start = Instant::now();
        if changes.layout {
            tree.layout_computed(available, &cache);
        }
        totals[2] += start.elapsed().as_secs_f64();
    }
    let resolved = tree.style_resolutions() - resolved;
    let candidates = tree.cascade_stats().candidate_tests - candidates;
    anyhow::ensure!(
        renders == frames,
        "unchanged file-list inputs were executed again"
    );
    // The path contains the root, sidebar and directory; rows must remain untouched.
    anyhow::ensure!(
        resolved == frames as u64 * 3,
        "styles propagated into unchanged rows"
    );
    anyhow::ensure!(
        candidates == 0,
        "unrelated row selectors were matched again"
    );
    anyhow::ensure!(
        backend.stats().paragraphs_shaped == shaped,
        "resizing reshaped text"
    );
    let milliseconds = |seconds: f64| seconds * 1000. / frames as f64;
    println!(
        "rows={rows} frames={frames} mean_ms reconcile={:.3} styles={:.3} layout={:.3} total={:.3}",
        milliseconds(totals[0]),
        milliseconds(totals[1]),
        milliseconds(totals[2]),
        milliseconds(totals.iter().sum())
    );
    println!(
        "component_renders={renders} resolved_nodes={resolved} selector_tests={candidates} new_shapes=0"
    );
    Ok(())
}
