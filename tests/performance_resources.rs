//! Resource and virtualization regressions assert bounded work, not wall-clock speed.
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};
use voidui::{
    core::{
        geometry::Point,
        layout::{AvailableSpace, Size},
        widget_tree::WidgetTree,
    },
    render::{ParleyTextSystem, TextLayoutCache, TextSystem},
    *,
};
#[test]
fn large_virtual_list_mounts_only_visible_and_pinned_rows() {
    let renders = Arc::new(AtomicUsize::new(0));
    let observed = renders.clone();
    let runtime = TaskRuntime::default();
    let mut tree = WidgetTree::with_task_runtime(runtime.clone());
    tree.build_root(
        virtual_list(
            100_000,
            VirtualListOptions {
                row_height: 20.,
                overscan_rows: 2,
                pinned: vec![90_000],
            },
            move |i| {
                observed.fetch_add(1, Ordering::Relaxed);
                div()
                    .height(20)
                    .flex_shrink(0.)
                    .key(format!("row-{i}"))
                    .id(format!("row-{i}"))
                    .into_element()
            },
        )
        .id("list"),
    );
    let cache = TextLayoutCache::new(Arc::new(TextSystem::new(Arc::new(
        ParleyTextSystem::new_without_system_fonts("unused"),
    ))));
    let layout = |tree: &mut WidgetTree| {
        tree.layout(
            Size {
                width: AvailableSpace::Definite(400.),
                height: AvailableSpace::Definite(200.),
            },
            &cache,
        );
    };
    layout(&mut tree);
    runtime.tick();
    tree.flush_updates();
    layout(&mut tree);
    assert!(tree.find_by_id("row-90000").is_some());
    assert!(tree.find_by_id("row-50000").is_none());
    let root = tree.find_by_id("list").unwrap();
    tree.scroll_to(root, Point::new(0., 1000.));
    runtime.tick();
    tree.flush_updates();
    layout(&mut tree);
    assert!(tree.find_by_id("row-50").is_some());
    assert!(tree.find_by_id("row-0").is_none());
    assert!(renders.load(Ordering::Relaxed) < 100);
    assert!(!tree.update_styles(Instant::now()).layout);
}
#[test]
fn image_metadata_does_not_decode_and_variants_respect_byte_budget() {
    let path = std::env::temp_dir().join(format!("voidui-asset-{}.png", std::process::id()));
    image::RgbaImage::from_pixel(512, 512, image::Rgba([1, 2, 3, 255]))
        .save(&path)
        .unwrap();

    let asset = media::ImageAsset::open(&path).unwrap();
    assert_eq!(asset.intrinsic_size(), [512., 512.]);

    media::ImageAsset::set_cache_budget(cache::CacheBudget {
        max_bytes: 16 * 1024,
        max_entries: 2,
    });
    let first = asset.load(64, 64).unwrap();
    assert!(first.retained_bytes() <= 16 * 1024);
    assert!(Arc::ptr_eq(&first, &asset.load(64, 64).unwrap()));
    let large = asset.load(128, 128).unwrap();
    assert_eq!(large.retained_bytes(), 65536);
    assert!(media::ImageAsset::cache_stats().retained_bytes <= 16384);
    media::ImageAsset::set_cache_budget(Default::default());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn virtual_list_pinned_row_keeps_component_state_across_scrolling() {
    use std::{cell::Cell, rc::Rc};
    let mounts = Rc::new(Cell::new(0));
    let observed = mounts.clone();
    let runtime = TaskRuntime::default();
    let mut tree = WidgetTree::with_task_runtime(runtime.clone());
    tree.build_root(
        virtual_list(
            10000,
            VirtualListOptions {
                pinned: vec![0],
                ..Default::default()
            },
            move |i| {
                let observed = observed.clone();
                component(move || {
                    let _ = state(|| {
                        if i == 0 {
                            observed.set(observed.get() + 1);
                        }
                        0
                    });
                    div().height(30).flex_shrink(0.)
                })
                .key(format!("row-{i}"))
                .into_element()
            },
        )
        .id("list"),
    );
    let cache = TextLayoutCache::new(Arc::new(TextSystem::new(Arc::new(
        ParleyTextSystem::new_without_system_fonts("unused"),
    ))));
    for y in [0., 1000., 2000., 0.] {
        tree.layout(
            Size {
                width: AvailableSpace::Definite(400.),
                height: AvailableSpace::Definite(200.),
            },
            &cache,
        );
        let root = tree.find_by_id("list").unwrap();
        tree.scroll_to(root, Point::new(0., y));
        runtime.tick();
        tree.flush_updates();
    }
    assert_eq!(mounts.get(), 1);
}

#[test]
fn mounted_asset_decodes_on_shared_executor_and_publishes_pixels() {
    use std::{
        borrow::Cow,
        collections::HashMap,
        sync::Mutex,
        time::{Duration, Instant},
    };
    use voidui::render::{self, *};
    #[derive(Default)]
    struct Atlas(Mutex<HashMap<AtlasKey, AtlasTile>>);
    impl PlatformAtlas for Atlas {
        fn get_or_insert_with<'a>(
            &self,
            key: &AtlasKey,
            build: &mut dyn FnMut() -> render::Result<
                Option<(render::Size<DevicePixels>, Cow<'a, [u8]>)>,
            >,
        ) -> render::Result<Option<AtlasTile>> {
            let mut tiles = self.0.lock().unwrap();
            if let Some(tile) = tiles.get(key) {
                return Ok(Some(*tile));
            }
            let Some((size, _)) = build()? else {
                return Ok(None);
            };
            let tile = AtlasTile {
                texture_id: AtlasTextureId {
                    index: 0,
                    kind: key.texture_kind(),
                },
                tile_id: TileId(tiles.len() as u32),
                padding: 0,
                bounds: Bounds::new(point(DevicePixels(0), DevicePixels(0)), size),
            };
            tiles.insert(key.clone(), tile);
            Ok(Some(tile))
        }
        fn remove(&self, key: &AtlasKey) {
            self.0.lock().unwrap().remove(key);
        }
    }
    let path = std::env::temp_dir().join(format!("voidui-async-asset-{}.png", std::process::id()));
    image::RgbaImage::from_pixel(256, 256, image::Rgba([255, 0, 0, 255]))
        .save(&path)
        .unwrap();
    let runtime = TaskRuntime::default();
    let mut tree = WidgetTree::with_task_runtime(runtime.clone());
    tree.build_root(
        asset_img(media::ImageAsset::open(&path).unwrap())
            .width(64)
            .height(64),
    );
    let cache = TextLayoutCache::new(Arc::new(TextSystem::new(Arc::new(
        ParleyTextSystem::new_without_system_fonts("unused"),
    ))));
    let atlas = Atlas::default();
    let started = Instant::now();
    loop {
        runtime.tick();
        tree.layout(
            voidui::core::layout::Size {
                width: AvailableSpace::Definite(64.),
                height: AvailableSpace::Definite(64.),
            },
            &cache,
        );
        let mut scene = Scene::default();
        let mut painter = Painter::new(
            &mut scene,
            &atlas,
            cache.system().clone(),
            size(px(64.), px(64.)),
            1.,
        )
        .unwrap();
        tree.draw(&mut painter).unwrap();
        drop(painter);
        if !scene.polychrome_sprites.is_empty() {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "asset did not publish"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(runtime.stats().background_started);
    drop(tree);
    runtime.shutdown();
    std::fs::remove_file(path).unwrap();
}
