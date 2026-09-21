//! `cargo run --release --example stacking_bench [nodes] [frames]`
//! Measures CPU scene construction with a cached stacking order, not GPU time.
use std::{borrow::Cow, sync::Arc, time::Instant};
use voidui::{
    core::{
        layout::{AvailableSpace, Size},
        widget_tree::WidgetTree,
    },
    div,
    render::{
        self, AtlasKey, AtlasTile, DevicePixels, Painter, ParleyTextSystem, PlatformAtlas, Scene,
        TextLayoutCache, TextSystem, px, size,
    },
    style::color::Rgba8,
};
struct NoGlyphs;
impl PlatformAtlas for NoGlyphs {
    fn get_or_insert_with<'a>(
        &self,
        _: &AtlasKey,
        _: &mut dyn FnMut() -> render::Result<Option<(render::Size<DevicePixels>, Cow<'a, [u8]>)>>,
    ) -> render::Result<Option<AtlasTile>> {
        panic!("no glyphs")
    }
    fn remove(&self, _: &AtlasKey) {}
}
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let count = args
        .first()
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(1000usize);
    let frames = args
        .get(1)
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(200usize);
    anyhow::ensure!(
        count > 0 && frames > 0 && count <= i32::MAX as usize,
        "invalid benchmark dimensions"
    );
    let mut root = div().relative().width(400).height(300);
    for i in 0..count {
        root = root.child(
            div()
                .absolute()
                .left(0)
                .top(0)
                .width(40)
                .height(30)
                .z_index((count - i) as i32)
                .background(Rgba8::from_rgb8(40, 80, 120)),
        );
    }
    let mut tree = WidgetTree::new();
    tree.build_root(root);
    let system = Arc::new(TextSystem::new(Arc::new(
        ParleyTextSystem::new_without_system_fonts("unused"),
    )));
    tree.layout(
        Size {
            width: AvailableSpace::Definite(400.0),
            height: AvailableSpace::Definite(300.0),
        },
        &TextLayoutCache::new(system.clone()),
    );
    let mut scene = Scene::default();
    let paint = |scene: &mut Scene| -> anyhow::Result<()> {
        scene.clear();
        let mut painter = Painter::new(
            scene,
            &NoGlyphs,
            system.clone(),
            size(px(400.0), px(300.0)),
            1.0,
        )?;
        tree.draw(&mut painter)?;
        drop(painter);
        scene.finish();
        Ok(())
    };
    let start = Instant::now();
    paint(&mut scene)?;
    println!(
        "first scene including stacking sort: {:.3} ms",
        start.elapsed().as_secs_f64() * 1000.0
    );
    let builds = tree.paint_order_rebuilds();
    let start = Instant::now();
    for _ in 0..frames {
        paint(&mut scene)?;
    }
    anyhow::ensure!(
        tree.paint_order_rebuilds() == builds,
        "unchanged order was rebuilt"
    );
    println!(
        "{count} overlapping contexts: {:.3} ms/cached scene, {frames} frames, stacking rebuilds={builds}",
        start.elapsed().as_secs_f64() * 1000.0 / frames as f64
    );
    Ok(())
}
