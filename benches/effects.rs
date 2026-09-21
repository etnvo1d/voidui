//! `cargo bench --bench effects -- [nodes] [frames]`
//! Separately measures idle style updates and animated paint-only style sampling.
#[path = "support/args.rs"]
mod arguments;

use std::{
    hint::black_box,
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
    style::css::Stylesheet,
};
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = arguments::args().collect();
    let count = args
        .first()
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(1000usize);
    let frames = args
        .get(1)
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(600usize);
    anyhow::ensure!(count > 0 && frames > 0, "nodes and frames must be positive");
    let mut root = div();
    for _ in 0..count {
        root = root.child(div().class("box").width(10).height(10));
    }
    let mut tree = WidgetTree::new();
    let id = tree.build_root(root);
    tree.set_stylesheets(vec![Stylesheet::parse(".box {background:#135;transition:background-color 10s linear}:root.active>.box {background:#ace}")?]);
    let cache = TextLayoutCache::new(Arc::new(TextSystem::new(Arc::new(
        ParleyTextSystem::new_without_system_fonts("unused"),
    ))));
    tree.layout(Size::MAX_CONTENT, &cache);
    let stats = tree.cascade_stats();
    let t = Instant::now();
    let start = Instant::now();
    for _ in 0..100_000 {
        black_box(tree.update_styles(black_box(t)));
    }
    println!(
        "idle update: {:.1} ns/call",
        start.elapsed().as_nanos() as f64 / 100_000.0
    );
    anyhow::ensure!(tree.cascade_stats() == stats, "idle rematched selectors");
    tree.set_classes(id, "active");
    tree.update_styles(t);
    let stats = tree.cascade_stats();
    let start = Instant::now();
    for frame in 1..=frames {
        let change =
            tree.update_styles(t + Duration::from_secs_f64(9.0 * frame as f64 / frames as f64));
        anyhow::ensure!(!change.layout, "paint transition invalidated layout");
        black_box(change);
    }
    println!(
        "{count} animated boxes: {:.3} ms/style frame, {frames} samples",
        start.elapsed().as_secs_f64() * 1000.0 / frames as f64
    );
    anyhow::ensure!(
        tree.cascade_stats() == stats,
        "animation rematched selectors"
    );
    tree.update_styles(t + Duration::from_secs(10));
    anyhow::ensure!(
        tree.next_animation_frame(t).is_none(),
        "animation did not finish"
    );
    println!("PASS: no layout invalidation, selector rematching, or idle deadline");
    Ok(())
}
