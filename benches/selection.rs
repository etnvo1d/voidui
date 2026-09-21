//! CPU selection cost only: excludes initial layout, font shaping, GPU painting,
//! and native events. Run with `cargo bench --bench selection`.
use std::{borrow::Cow, hint::black_box, sync::Arc, time::Instant};
use voidui::{
    core::{
        geometry::Point,
        layout::{AvailableSpace, Size},
        widget_tree::WidgetTree,
    },
    div,
    render::{ParleyTextSystem, TextLayoutCache, TextSystem},
    style::css::Stylesheet,
    text,
};
fn main() -> anyhow::Result<()> {
    let nodes = 1000;
    let fonts = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    fonts.add_fonts(vec![Cow::Borrowed(include_bytes!(
        "../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
    ))])?;
    let cache = TextLayoutCache::new(Arc::new(TextSystem::new(Arc::new(fonts))));
    let mut root = div();
    for i in 0..nodes {
        root = root
            .child(text("A paragraph with selectable Unicode: Cafe\u{301}.").id(format!("p{i}")));
    }
    let mut tree = WidgetTree::new();
    tree.build_root(root);
    tree.set_stylesheets(vec![Stylesheet::parse("*{font-family:'IBM Plex Sans';font-size:16px;line-height:24px}::selection{color:black;background:yellow}")?]);
    tree.layout(
        Size {
            width: AvailableSpace::Definite(700.0),
            height: AvailableSpace::MaxContent,
        },
        &cache,
    );
    let first = tree.find_by_id("p0").unwrap();
    let last = tree.find_by_id(&format!("p{}", nodes - 1)).unwrap();
    let point = |id, byte| {
        let p = tree.text_caret_position(id, byte).unwrap();
        Point::new(p.x, p.y + 8.0)
    };
    let start = point(first, 0);
    let short = [point(first, 8), point(first, 25)];
    let long = [point(last, 8), point(last, 25)];
    let stats = tree.cascade_stats();
    let bounds = tree.bounds(last);
    for (label, points, target) in [("short", short, first), ("document", long, last)] {
        tree.selection_pointer_down(start, false, 1);
        tree.selection_pointer_move(points[0]);
        black_box(tree.selected_range(target));
        let iterations = 2000;
        let now = Instant::now();
        for i in 0..iterations {
            tree.selection_pointer_move(points[i % 2]);
            black_box(tree.selected_range(target));
        }
        println!(
            "{nodes} text nodes, {label} range: {:.2} us/update (hit test + range fragments)",
            now.elapsed().as_secs_f64() * 1e6 / iterations as f64
        );
        tree.end_selection_drag();
    }
    assert_eq!(tree.update_styles(Instant::now()), Default::default());
    assert_eq!(tree.cascade_stats(), stats);
    assert_eq!(tree.bounds(last), bounds);
    println!("PASS no selector rematch or layout invalidation during range updates");
    Ok(())
}
