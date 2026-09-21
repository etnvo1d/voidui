//! Headless scene benchmark: cargo bench --bench media -- 1000 200
//! The first argument is the icon count; the second is the cached frame count.
#[path = "support/args.rs"]
mod arguments;

use anyhow::{Result, ensure};
use std::{
    borrow::Cow,
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Instant,
};
use voidui::{
    core::{
        layout::{Size, TaffyMaxContent},
        widget_tree::WidgetTree,
    },
    div,
    render::{
        self, AtlasKey, AtlasTile, DevicePixels, Painter, ParleyTextSystem, PlatformAtlas, Scene,
        TextLayoutCache, TextSystem,
    },
    svg,
};
#[derive(Default)]
struct Atlas(Mutex<HashMap<AtlasKey, AtlasTile>>);
impl PlatformAtlas for Atlas {
    fn get_or_insert_with<'a>(
        &self,
        key: &AtlasKey,
        build: &mut dyn FnMut() -> Result<Option<(render::Size<DevicePixels>, Cow<'a, [u8]>)>>,
    ) -> Result<Option<AtlasTile>> {
        let mut cache = self.0.lock().unwrap();
        if let Some(tile) = cache.get(key) {
            return Ok(Some(*tile));
        }
        let Some((size, _)) = build()? else {
            return Ok(None);
        };
        let tile = AtlasTile {
            texture_id: render::AtlasTextureId {
                kind: render::AtlasTextureKind::Polychrome,
                index: 0,
            },
            tile_id: render::TileId(cache.len() as u32),
            padding: 0,
            bounds: render::Bounds::new(render::point(DevicePixels(0), DevicePixels(0)), size),
        };
        cache.insert(key.clone(), tile);
        Ok(Some(tile))
    }
    fn remove(&self, key: &AtlasKey) {
        self.0.lock().unwrap().remove(key);
    }
}
fn main() -> Result<()> {
    let mut args = arguments::args();
    let count = args
        .next()
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(1000usize);
    let frames = args
        .next()
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(200usize);
    ensure!(count > 0 && frames > 0, "counts must be positive");
    let text = Arc::new(TextSystem::new(Arc::new(
        ParleyTextSystem::new_without_system_fonts("unused"),
    )));
    let mut tree = WidgetTree::new();
    tree.build_root(div().flex().children((0..count).map(|_| {
        svg()
            .width(24)
            .height(24)
            .view_box(0, 0, 24, 24)
            .fill("none")
            .stroke("blue")
            .stroke_width(2)
            .child(svg::path().d("M5 12 L10 17 L19 7"))
    })));
    tree.layout(Size::MAX_CONTENT, &TextLayoutCache::new(text.clone()));
    let atlas = Atlas::default();
    let mut scene = Scene::default();
    let mut paint = || -> Result<()> {
        scene.clear();
        let mut p = Painter::new(
            &mut scene,
            &atlas,
            text.clone(),
            render::size(render::px(count as f32 * 24.), render::px(24.)),
            2.,
        )?;
        tree.draw(&mut p)?;
        drop(p);
        scene.finish();
        Ok(())
    };
    let cold = Instant::now();
    paint()?;
    let cold = cold.elapsed();
    let before = voidui::media::stats();
    let start = Instant::now();
    for _ in 0..frames {
        paint()?;
    }
    let elapsed = start.elapsed();
    ensure!(
        before == voidui::media::stats(),
        "cached frames repeated SVG parsing or rasterization"
    );
    ensure!(
        atlas.0.lock().unwrap().len() == 1,
        "identical icons did not share a texture"
    );
    println!(
        "{count} icons, {frames} cached frames: cold={cold:?}, mean={:?}, atlas_entries=1, {:?}",
        elapsed / frames as u32,
        voidui::media::stats()
    );
    Ok(())
}
