//! `cargo bench --bench css -- [nodes] [rules]`
#[path = "support/args.rs"]
mod arguments;

use std::{sync::Arc, time::Instant};
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
    let nodes = args
        .first()
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(1000usize);
    let rules = args
        .get(1)
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(1000usize);
    anyhow::ensure!(rules > 0, "rules must be positive");
    let source = (0..rules)
        .map(|n| format!(".c{n}{{height:2px;padding:1px;}}\n"))
        .collect::<String>();
    let start = Instant::now();
    let sheet = Stylesheet::parse(&source)?;
    let parse = start.elapsed();
    let mut root = div();
    for n in 0..nodes {
        root = root.child(div().class(format!("c{}", n % rules)));
    }
    let mut tree = WidgetTree::new();
    tree.build_root(root);
    tree.set_stylesheets(vec![sheet]);
    let cache = TextLayoutCache::new(Arc::new(TextSystem::new(Arc::new(
        ParleyTextSystem::new_without_system_fonts("unused"),
    ))));
    let start = Instant::now();
    tree.layout(Size::MAX_CONTENT, &cache);
    let first = start.elapsed();
    let stats = tree.cascade_stats();
    let start = Instant::now();
    tree.layout(Size::MAX_CONTENT, &cache);
    let repeat = start.elapsed();
    anyhow::ensure!(
        tree.cascade_stats() == stats,
        "unchanged layout rematched selectors"
    );
    anyhow::ensure!(
        stats.candidate_tests == nodes as u64,
        "unexpected candidate scan"
    );
    println!(
        "nodes={nodes} rules={rules} parse={parse:?} first_layout={first:?} repeated_layout={repeat:?} cascade={stats:?}; repeated candidate tests=0"
    );
    Ok(())
}
