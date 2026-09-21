//! Pixel-level CPU tests of the complete CSS -> SVG/image -> atlas flow.
use std::{
    borrow::Cow,
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Instant,
};
use voidui::{
    core::{
        element::IntoElement,
        layout::{Size, TaffyMaxContent},
        widget::WidgetStatus,
        widget_tree::WidgetTree,
    },
    div, img,
    media::{Image, MediaLimits, SvgOptions},
    render::{
        self, AtlasKey, AtlasTextureId, AtlasTextureKind, AtlasTile, DevicePixels, Painter,
        ParleyTextSystem, PlatformAtlas, Scene, TextLayoutCache, TextSystem,
    },
    style::{
        css::Stylesheet,
        media::{ObjectFit, ObjectPosition, object_rect},
    },
    svg, svg_from_str,
};
#[derive(Default)]
struct CpuAtlas(Mutex<HashMap<AtlasKey, (AtlasTile, Vec<u8>)>>);
impl PlatformAtlas for CpuAtlas {
    fn get_or_insert_with<'a>(
        &self,
        key: &AtlasKey,
        build: &mut dyn FnMut() -> anyhow::Result<
            Option<(render::Size<DevicePixels>, Cow<'a, [u8]>)>,
        >,
    ) -> anyhow::Result<Option<AtlasTile>> {
        let mut entries = self.0.lock().unwrap();
        if let Some((tile, _)) = entries.get(key) {
            return Ok(Some(*tile));
        }
        let Some((size, bytes)) = build()? else {
            return Ok(None);
        };
        let tile = AtlasTile {
            texture_id: AtlasTextureId {
                kind: AtlasTextureKind::Polychrome,
                index: entries.len() as u32,
            },
            tile_id: render::TileId(entries.len() as u32),
            padding: 0,
            bounds: render::Bounds::new(render::point(DevicePixels(0), DevicePixels(0)), size),
        };
        entries.insert(key.clone(), (tile, bytes.into_owned()));
        Ok(Some(tile))
    }
    fn remove(&self, key: &AtlasKey) {
        self.0.lock().unwrap().remove(key);
    }
}
fn fonts() -> Arc<TextSystem> {
    Arc::new(TextSystem::new(Arc::new(
        ParleyTextSystem::new_without_system_fonts("unused"),
    )))
}
fn tree(element: impl IntoElement, css: &str) -> WidgetTree {
    let mut tree = WidgetTree::new();
    tree.build_root(element);
    tree.set_stylesheets(vec![Stylesheet::parse(css).unwrap()]);
    tree.layout(Size::MAX_CONTENT, &TextLayoutCache::new(fonts()));
    tree
}
fn draw(tree: &WidgetTree, atlas: &CpuAtlas, scale: f32) -> Scene {
    let mut scene = Scene::default();
    let mut painter = Painter::new(
        &mut scene,
        atlas,
        fonts(),
        render::size(render::px(1000.), render::px(1000.)),
        scale,
    )
    .unwrap();
    tree.draw(&mut painter).unwrap();
    drop(painter);
    scene.finish();
    scene
}
fn pixel(atlas: &CpuAtlas, scene: &Scene, sprite: usize, x: usize, y: usize) -> [u8; 4] {
    let tile = scene.polychrome_sprites[sprite].tile;
    let entries = atlas.0.lock().unwrap();
    let (_, bytes) = entries
        .values()
        .find(|(t, _)| t.texture_id == tile.texture_id)
        .unwrap();
    bytes[(y * tile.bounds.size.width.0 as usize + x) * 4..][..4]
        .try_into()
        .unwrap()
}
#[test]
fn image_intrinsic_ratio_css_width_padding_and_fit() {
    let image = Image::from_rgba(4, 2, vec![255; 32]).unwrap();
    let tree = tree(img(image), "img { width: 80px; padding: 5px; }");
    let b = tree.bounds(tree.root().unwrap());
    assert_eq!([b.size.width, b.size.height], [90., 50.]);
    let atlas = CpuAtlas::default();
    let scene = draw(&tree, &atlas, 2.);
    let b = scene.polychrome_sprites[0].bounds;
    assert_eq!(
        [b.origin.x.0, b.origin.y.0, b.size.width.0, b.size.height.0],
        [10., 10., 160., 80.]
    );
}
#[test]
fn object_fit_modes_and_position_use_free_space() {
    let p = ObjectPosition::default();
    assert_eq!(
        object_rect([100., 100.], [200., 100.], ObjectFit::Contain, p),
        [0., 25., 100., 50.]
    );
    assert_eq!(
        object_rect([100., 100.], [200., 100.], ObjectFit::Cover, p),
        [-50., 0., 200., 100.]
    );
    assert_eq!(
        object_rect([100., 100.], [20., 10.], ObjectFit::ScaleDown, p),
        [40., 45., 20., 10.]
    );
    assert_eq!(
        object_rect([100., 100.], [200., 100.], ObjectFit::Fill, p),
        [0., 0., 100., 100.]
    );
    assert_eq!(
        object_rect(
            [100., 100.],
            [200., 100.],
            ObjectFit::None,
            "right 10px bottom 20px".parse().unwrap()
        ),
        [-110., -20., 200., 100.]
    );
    let p: ObjectPosition = "calc(100% - 10px) 25%".parse().unwrap();
    assert_eq!(p.x.resolve(50.), 40.);
    assert_eq!(p.y.resolve(80.), 20.);
}
#[test]
fn image_style_and_pixels_preserve_alpha() {
    let image = Image::from_rgba(2, 1, vec![255, 0, 0, 128, 0, 255, 0, 255]).unwrap();
    let tree = tree(
        img(image).alt("two pixels"),
        "img {width: 100px;height:100px;object-fit:contain;object-position:bottom;image-rendering:pixelated;opacity:50%;border-radius:12px;}",
    );
    let atlas = CpuAtlas::default();
    let scene = draw(&tree, &atlas, 1.);
    let s = scene.polychrome_sprites[0];
    assert_eq!(s.bounds.origin.y.0, 50.);
    assert_eq!(s.pad, 6);
    assert_eq!(s.opacity, 0.5);
    assert_eq!(s.rounded_bounds.size.height.0, 100.);
    assert_eq!(pixel(&atlas, &scene, 0, 0, 0), [0, 0, 128, 128]);
    assert_eq!(
        tree.attribute(tree.root().unwrap(), "alt").unwrap(),
        "two pixels"
    );
}
#[test]
fn shared_images_upload_once_and_recover_after_clear() {
    let image = Image::from_rgba(1, 1, vec![255, 0, 0, 255]).unwrap();
    let tree = tree(
        div()
            .flex()
            .children((0..100).map(|_| img(image.clone()).width(8).height(8))),
        "",
    );
    let atlas = CpuAtlas::default();
    let scene = draw(&tree, &atlas, 1.);
    assert_eq!(scene.polychrome_sprites.len(), 100);
    assert_eq!(atlas.0.lock().unwrap().len(), 1);
    let _ = draw(&tree, &atlas, 1.);
    assert_eq!(atlas.0.lock().unwrap().len(), 1);
    atlas.0.lock().unwrap().clear();
    let _ = draw(&tree, &atlas, 1.);
    assert_eq!(atlas.0.lock().unwrap().len(), 1);
}
#[test]
fn inline_svg_inherits_color_and_keeps_explicit_colors() {
    let tree = tree(
        div()
            .color(voidui::style::color::Rgba8::from_rgb8(0, 128, 255))
            .child(
                svg()
                    .view_box(0, 0, 20, 10)
                    .width(20)
                    .height(10)
                    .fill("currentColor")
                    .child(svg::rect().width(10).height(10))
                    .child(svg::rect().x(10).width(10).height(10).fill("red")),
            ),
        "",
    );
    let atlas = CpuAtlas::default();
    let scene = draw(&tree, &atlas, 1.);
    assert_eq!(pixel(&atlas, &scene, 0, 5, 5), [255, 128, 0, 255]);
    assert_eq!(pixel(&atlas, &scene, 0, 15, 5), [0, 0, 255, 255]);
    assert_eq!(
        tree.children(tree.children(tree.root().unwrap())[0]).len(),
        0,
        "SVG paths must not allocate widget nodes"
    );
}
#[test]
fn svg_css_selectors_cross_the_host_boundary() {
    let tree = tree(
        div().class("toolbar").child(
            svg()
                .view_box(0, 0, 20, 10)
                .width(20)
                .height(10)
                .class("icon")
                .child(
                    svg::rect()
                        .class("first")
                        .width(10)
                        .height(10)
                        .fill("green"),
                )
                .child(svg::rect().x(10).width(10).height(10).class("last")),
        ),
        ".toolbar:has(svg > .first) {color:red} .toolbar > svg.icon .first {fill:currentColor} .first + .last:nth-child(2) {fill:blue}",
    );
    let atlas = CpuAtlas::default();
    let scene = draw(&tree, &atlas, 1.);
    assert_eq!(pixel(&atlas, &scene, 0, 5, 5), [0, 0, 255, 255]);
    assert_eq!(pixel(&atlas, &scene, 0, 15, 5), [255, 0, 0, 255]);
}
#[test]
fn imported_styles_and_important_follow_the_css_cascade() {
    let svg=svg_from_str(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 10" width="20" height="10" class="icon"><style>.a {fill: red !important} .b {fill:green}</style><rect class="a" width="10" height="10" style="fill:blue !important"/><rect class="b" x="10" width="10" height="10"/></svg>"#).unwrap();
    let tree = tree(svg, ".icon .b {fill:blue}");
    let atlas = CpuAtlas::default();
    let scene = draw(&tree, &atlas, 1.);
    assert_eq!(pixel(&atlas, &scene, 0, 5, 5), [255, 0, 0, 255]);
    assert_eq!(pixel(&atlas, &scene, 0, 15, 5), [255, 0, 0, 255]);
}
#[test]
fn paint_only_theme_changes_rerasterize_without_layout() {
    let mut tree = tree(
        svg()
            .view_box(0, 0, 10, 10)
            .width(10)
            .height(10)
            .child(svg::rect().width(10).height(10)),
        "svg {fill:red} svg:hover rect {fill:blue}",
    );
    let atlas = CpuAtlas::default();
    let scene = draw(&tree, &atlas, 1.);
    assert_eq!(pixel(&atlas, &scene, 0, 5, 5), [0, 0, 255, 255]);
    tree.set_status(tree.root().unwrap(), WidgetStatus::Hover);
    let change = tree.update_styles(Instant::now());
    assert!(!change.layout);
    assert!(change.paint);
    let scene = draw(&tree, &atlas, 1.);
    assert_eq!(pixel(&atlas, &scene, 0, 5, 5), [255, 0, 0, 255]);
}
#[test]
fn identical_inline_documents_share_rasters_and_dpi_variants() {
    let tree = tree(
        div().flex().children((0..50).map(|_| {
            svg()
                .view_box(0, 0, 10, 10)
                .width(10)
                .height(10)
                .child(svg::rect().width(10).height(10).fill("blue"))
        })),
        "",
    );
    let atlas = CpuAtlas::default();
    let scene = draw(&tree, &atlas, 1.);
    assert_eq!(scene.polychrome_sprites.len(), 50);
    assert_eq!(atlas.0.lock().unwrap().len(), 1);
    let second = draw(&tree, &atlas, 2.);
    assert_eq!(second.polychrome_sprites[0].tile.bounds.size.width.0, 20);
    assert_eq!(atlas.0.lock().unwrap().len(), 2);
}
#[test]
fn svg_css_geometry_and_d_use_standard_syntax() {
    let tree = tree(
        svg()
            .view_box(0, 0, 20, 10)
            .width(20)
            .height(10)
            .child(svg::circle())
            .child(svg::path()),
        "circle {cx:5px;cy:5px;r:4px;fill:blue} path {d:path('M10 0 H20 V10 H10 Z');fill:red}",
    );
    let atlas = CpuAtlas::default();
    let scene = draw(&tree, &atlas, 1.);
    assert_eq!(pixel(&atlas, &scene, 0, 5, 5), [255, 0, 0, 255]);
    assert_eq!(pixel(&atlas, &scene, 0, 15, 5), [0, 0, 255, 255]);
    assert!(Stylesheet::parse("path {d:'M0 0 L10 10'}").is_err());
}

#[test]
fn imported_native_paths_survive_and_css_can_override_them() {
    let source = r#"<svg viewBox="0 0 20 10" width="20" height="10"><path d="M0 0H10V10H0Z" fill="red"/></svg>"#;
    for (css, left, right) in [
        ("", [0, 0, 255, 255], [0, 0, 0, 0]),
        (
            "path {d:path('M10 0H20V10H10Z');fill:blue}",
            [0, 0, 0, 0],
            [255, 0, 0, 255],
        ),
    ] {
        let tree = tree(svg_from_str(source).unwrap(), css);
        let atlas = CpuAtlas::default();
        let scene = draw(&tree, &atlas, 1.);
        assert_eq!(pixel(&atlas, &scene, 0, 5, 5), left, "{css}");
        assert_eq!(pixel(&atlas, &scene, 0, 15, 5), right, "{css}");
    }
}
#[test]
fn gradients_clips_group_opacity_and_transforms_render() {
    let svg=svg_from_str(r##"<svg viewBox="0 0 20 10" width="20" height="10"><defs><linearGradient id="r"><stop stop-color="red"/><stop offset="1" stop-color="red"/></linearGradient><clipPath id="c"><rect width="10" height="10"/></clipPath></defs><g opacity=".5" clip-path="url(#c)"><rect width="20" height="10" fill="url(#r)"/><rect width="20" height="10" fill="url(#r)"/></g></svg>"##).unwrap();
    let tree = tree(svg, "");
    let atlas = CpuAtlas::default();
    let scene = draw(&tree, &atlas, 1.);
    let p = pixel(&atlas, &scene, 0, 5, 5);
    assert!((127..=128).contains(&p[3]), "{p:?}");
    assert_eq!(pixel(&atlas, &scene, 0, 15, 5), [0; 4]);
}
#[test]
fn viewbox_meet_and_none_have_distinct_geometry() {
    let a = svg()
        .view_box(0, 0, 20, 10)
        .width(20)
        .height(20)
        .child(svg::rect().width(20).height(10).fill("red"));
    let b = svg()
        .view_box(0, 0, 20, 10)
        .width(20)
        .height(20)
        .preserve_aspect_ratio("none")
        .child(svg::rect().width(20).height(10).fill("red"));
    let tree = tree(div().flex().child(a).child(b), "");
    let atlas = CpuAtlas::default();
    let scene = draw(&tree, &atlas, 1.);
    assert_eq!(pixel(&atlas, &scene, 0, 10, 1), [0; 4]);
    assert_eq!(pixel(&atlas, &scene, 1, 10, 1), [0, 0, 255, 255]);
}
#[test]
fn image_svg_is_isolated_from_host_color() {
    let image=Image::from_bytes(br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="currentColor"/></svg>"#).unwrap();
    let tree = tree(img(image), "img {color:red;fill:blue}");
    let atlas = CpuAtlas::default();
    let scene = draw(&tree, &atlas, 1.);
    assert_eq!(pixel(&atlas, &scene, 0, 5, 5), [0, 0, 0, 255]);
}
#[test]
fn standard_css_rejects_invalid_values_and_custom_syntax() {
    for (name, value) in [
        ("object-fit", "stretch"),
        ("object-position", "left right"),
        ("stroke-width", "-1"),
        ("stroke-dasharray", "3 -1"),
        ("fill-rule", "winding"),
        ("paint-order", "fill fill"),
        ("image-rendering", "nearest"),
        ("view-box", "0 0 24 24"),
        ("preserve-aspect-ratio", "none"),
    ] {
        assert!(
            Stylesheet::parse(&format!("svg {{{name}:{value}}}")).is_err(),
            "{name}:{value}"
        );
    }
    for p in [
        "object-fit",
        "object-position",
        "image-rendering",
        "fill",
        "stroke",
        "stroke-width",
        "opacity",
    ] {
        for value in ["initial", "inherit", "unset"] {
            Stylesheet::parse(&format!("svg {{{p}:{value}}}")).unwrap();
        }
    }
}
#[test]
fn hidden_media_does_not_allocate_atlas_entries() {
    let tree = tree(
        svg()
            .view_box(0, 0, 10, 10)
            .width(10)
            .height(10)
            .child(svg::rect().width(10).height(10)),
        "svg {display:none}",
    );
    let atlas = CpuAtlas::default();
    let scene = draw(&tree, &atlas, 1.);
    assert!(scene.polychrome_sprites.is_empty());
    assert!(atlas.0.lock().unwrap().is_empty());
}
#[test]
fn source_and_raster_limits_fail_before_large_allocations() {
    assert!(Image::from_rgba(2, 2, vec![0; 8]).is_err());
    let options = SvgOptions {
        limits: MediaLimits {
            max_input_bytes: 8,
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(Image::from_bytes_with_options(b"<svg width='1' height='1'/>", &options).is_err());
    assert!(
        Image::from_rgba_with_limits(
            100,
            100,
            vec![],
            MediaLimits {
                max_pixels: 10,
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(svg_from_str("<svg><path></svg>").is_err());
}
#[test]
fn enabled_bitmap_formats_decode() {
    use image::{DynamicImage, ImageFormat};
    use std::io::Cursor;
    for format in [
        ImageFormat::Png,
        ImageFormat::Jpeg,
        ImageFormat::Gif,
        ImageFormat::WebP,
    ] {
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::new_rgb8(3, 2)
            .write_to(&mut bytes, format)
            .unwrap();
        let image = Image::from_bytes(bytes.get_ref()).unwrap();
        assert_eq!(image.intrinsic_size(), [3., 2.]);
    }
}

#[test]
fn image_svg_respects_viewbox_when_object_fit_changes_viewport() {
    let image=Image::from_bytes(br#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10" viewBox="0 0 20 10"><rect width="20" height="10" fill="red"/></svg>"#).unwrap();
    let tree = tree(img(image), "img {width:20px;height:20px;object-fit:fill}");
    let atlas = CpuAtlas::default();
    let scene = draw(&tree, &atlas, 1.);
    assert_eq!(pixel(&atlas, &scene, 0, 10, 1), [0; 4]);
    assert_eq!(pixel(&atlas, &scene, 0, 10, 10), [0, 0, 255, 255]);
}
#[test]
fn svg_child_can_override_inherited_visibility() {
    let tree = tree(
        svg()
            .view_box(0, 0, 10, 10)
            .width(10)
            .height(10)
            .child(svg::rect().width(10).height(10)),
        "svg {visibility:hidden} rect {visibility:visible;fill:blue}",
    );
    let atlas = CpuAtlas::default();
    let scene = draw(&tree, &atlas, 1.);
    assert_eq!(pixel(&atlas, &scene, 0, 5, 5), [255, 0, 0, 255]);
}
#[test]
fn local_hover_styles_trigger_paint_without_global_stylesheets() {
    let svg=svg_from_str(r#"<svg viewBox="0 0 10 10" width="10" height="10"><style>svg:hover rect{fill:blue}</style><rect width="10" height="10" fill="red"/></svg>"#).unwrap();
    let mut tree = WidgetTree::new();
    tree.build_root(svg);
    tree.layout(Size::MAX_CONTENT, &TextLayoutCache::new(fonts()));
    assert!(tree.has_dynamic_css());
    tree.set_status(tree.root().unwrap(), WidgetStatus::Hover);
    assert!(tree.update_styles(Instant::now()).paint);
    let atlas = CpuAtlas::default();
    let scene = draw(&tree, &atlas, 1.);
    assert_eq!(pixel(&atlas, &scene, 0, 5, 5), [255, 0, 0, 255]);
}
#[test]
fn max_width_preserves_intrinsic_ratio() {
    let image = Image::from_rgba(100, 50, vec![255; 100 * 50 * 4]).unwrap();
    let tree = tree(img(image), "img{max-width:20px}");
    let b = tree.bounds(tree.root().unwrap());
    assert_eq!([b.size.width, b.size.height], [20., 10.]);
}
#[test]
fn svg_explicit_initial_overrides_presentation_and_inheritance() {
    let tree = tree(
        svg()
            .view_box(0, 0, 10, 10)
            .width(10)
            .height(10)
            .fill("red")
            .child(svg::rect().width(10).height(10).fill("blue")),
        "rect {fill:initial}",
    );
    let atlas = CpuAtlas::default();
    let scene = draw(&tree, &atlas, 1.);
    assert_eq!(pixel(&atlas, &scene, 0, 5, 5), [0, 0, 0, 255]);
}

#[test]
fn use_instances_inherit_the_instance_paint() {
    let node = svg::defs().child(svg::g().id("shape").child(svg::rect().width(10).height(10)));
    let tree = tree(
        svg()
            .view_box(0, 0, 10, 10)
            .width(10)
            .height(10)
            .fill("blue")
            .child(node)
            .child(svg::use_node().href("#shape").fill("red")),
        "",
    );
    let atlas = CpuAtlas::default();
    let scene = draw(&tree, &atlas, 1.);
    assert_eq!(pixel(&atlas, &scene, 0, 5, 5), [0, 0, 255, 255]);
}
#[test]
fn svg_case_sensitive_type_selectors_are_indexed() {
    let node = svg::defs().child(
        svg::linear_gradient()
            .id("g")
            .child(svg::stop().offset(0))
            .child(svg::stop().offset(1)),
    );
    let tree = tree(
        svg()
            .view_box(0, 0, 10, 10)
            .width(10)
            .height(10)
            .child(node)
            .child(svg::rect().width(10).height(10).fill("url(#g)")),
        "linearGradient > stop {stop-color:red} lineargradient > stop {stop-color:blue}",
    );
    let atlas = CpuAtlas::default();
    let scene = draw(&tree, &atlas, 1.);
    assert_eq!(pixel(&atlas, &scene, 0, 5, 5), [0, 0, 255, 255]);
}
#[test]
fn svg_zero_viewbox_suppresses_paint() {
    let tree = tree(
        svg()
            .view_box(0, 0, 0, 10)
            .width(10)
            .height(10)
            .child(svg::rect().width(10).height(10)),
        "",
    );
    let atlas = CpuAtlas::default();
    let scene = draw(&tree, &atlas, 1.);
    assert!(scene.polychrome_sprites.is_empty());
}
#[test]
fn image_assets_and_raw_graphs_are_send_and_sync() {
    fn check<T: Send + Sync>() {}
    check::<Image>();
    check::<voidui::svg::SvgNode>();
}
#[test]
fn unchanged_reconciliation_reuses_svg_raster() {
    let make = || {
        svg()
            .view_box(0, 0, 10, 10)
            .width(10)
            .height(10)
            .child(svg::rect().width(10).height(10))
    };
    let mut tree = tree(make(), "");
    let atlas = CpuAtlas::default();
    let first = draw(&tree, &atlas, 1.);
    tree.reconcile_root(make());
    tree.update_styles(Instant::now());
    let next = draw(&tree, &atlas, 1.);
    assert_eq!(atlas.0.lock().unwrap().len(), 1);
    assert_eq!(
        first.polychrome_sprites[0].tile,
        next.polychrome_sprites[0].tile
    );
}

#[test]
fn svg_text_uses_explicit_fonts_and_preserves_tspan_text() {
    let mut fonts = voidui::media::fontdb::Database::new();
    fonts.load_font_data(
        include_bytes!("../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf").to_vec(),
    );
    let svg=svg_from_str(r#"<svg width="100" height="24"><text x="0" y="18" font-size="18" font-family="IBM Plex Sans">A<!-- omitted --><tspan>B</tspan>C</text></svg>"#).unwrap().options(SvgOptions {fontdb:Arc::new(fonts),..Default::default()});
    let tree = tree(svg, "");
    let atlas = CpuAtlas::default();
    let scene = draw(&tree, &atlas, 1.);
    let painted = (0..24)
        .flat_map(|y| (0..100).map(move |x| (x, y)))
        .filter(|&(x, y)| pixel(&atlas, &scene, 0, x, y)[3] > 0)
        .count();
    assert!(
        painted > 100,
        "SVG text must render with the supplied font database"
    );
    let doc =
        voidui::SvgDocument::parse(r#"<svg><text>A<!-- omitted --><tspan>B</tspan>C</text></svg>"#)
            .unwrap();
    assert!(!doc.to_xml().contains("omitted"));
    assert!(doc.to_xml().contains("</tspan>C"));
}

// Drive the same UI executor used by native windows. The timeout only detects
// stalled work; assertions below use task completion rather than elapsed time.
fn finish_media_tasks(tree: &WidgetTree) {
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    while tree.task_runtime().stats().active != 0 {
        assert!(Instant::now() < deadline, "SVG task did not complete");
        tree.task_runtime().tick();
        std::thread::yield_now();
    }
}

#[test]
fn background_svg_resize_reuses_texture_then_publishes_latest_size() {
    use voidui::media::SvgRenderPolicy;
    let mut tree = tree(
        svg_from_str(r#"<svg viewBox="0 0 20 10"><defs><filter id="blur"><feGaussianBlur stdDeviation="1"/></filter></defs><rect width="20" height="10" fill="red" filter="url(#blur)"/></svg>"#)
            .unwrap().render_policy(SvgRenderPolicy::ScaleWhileRendering),
        "svg {width:40px;height:20px}",
    );
    let atlas = CpuAtlas::default();
    assert!(draw(&tree, &atlas, 1.).polychrome_sprites.is_empty());
    assert!(!tree.task_runtime().stats().background_started);
    finish_media_tasks(&tree);
    assert!(tree.update_styles(Instant::now()).paint);
    let original = draw(&tree, &atlas, 1.);
    let old_tile = original.polychrome_sprites[0].tile;
    let spawned = tree.task_runtime().stats().spawned;
    // Without ticking, all intermediate requests must collapse into one task.
    for width in 41..81 {
        tree.set_stylesheets(vec![
            Stylesheet::parse(&format!("svg {{width:{width}px;height:30px}}")).unwrap(),
        ]);
        tree.layout(Size::MAX_CONTENT, &TextLayoutCache::new(fonts()));
        let scene = draw(&tree, &atlas, 2.);
        assert_eq!(scene.polychrome_sprites[0].tile, old_tile);
        assert_eq!(
            scene.polychrome_sprites[0].bounds.size.width.0,
            width as f32 * 2.
        );
    }
    assert_eq!(tree.task_runtime().stats().spawned, spawned + 1);
    finish_media_tasks(&tree);
    tree.update_styles(Instant::now());
    let latest = draw(&tree, &atlas, 2.);
    let tile = latest.polychrome_sprites[0].tile;
    assert_eq!(
        tile.bounds.size,
        render::size(DevicePixels(160), DevicePixels(60))
    );
    assert_ne!(tile, old_tile);
    assert!(pixel(&atlas, &latest, 0, 80, 30)[3] > 0);
    // A new atlas simulates device loss. The retained buffer rebuilds it without
    // scheduling work or falling back to synchronous SVG rasterization.
    let recovered_atlas = CpuAtlas::default();
    let recovered = draw(&tree, &recovered_atlas, 2.);
    assert_eq!(
        pixel(&atlas, &latest, 0, 80, 30),
        pixel(&recovered_atlas, &recovered, 0, 80, 30)
    );
    assert_eq!(tree.task_runtime().stats().spawned, spawned + 1);
}

#[test]
fn background_svg_image_restores_aspect_ratio_and_css_appearance() {
    use voidui::media::SvgRenderPolicy;
    let image = Image::from_bytes(
        br#"<svg viewBox="0 0 10 10"><rect width="10" height="10" fill="blue"/></svg>"#,
    )
    .unwrap();
    let mut tree = tree(
        img(image).render_policy(SvgRenderPolicy::ScaleWhileRendering),
        "img {width:10px;height:10px;object-fit:fill}",
    );
    let atlas = CpuAtlas::default();
    assert!(draw(&tree, &atlas, 1.).polychrome_sprites.is_empty());
    finish_media_tasks(&tree);
    tree.update_styles(Instant::now());
    let old = draw(&tree, &atlas, 1.);
    tree.set_stylesheets(vec![
        Stylesheet::parse("img {width:30px;height:10px;object-fit:fill;opacity:0.5}").unwrap(),
    ]);
    tree.layout(Size::MAX_CONTENT, &TextLayoutCache::new(fonts()));
    let pending = draw(&tree, &atlas, 1.);
    assert_eq!(
        pending.polychrome_sprites[0].tile,
        old.polychrome_sprites[0].tile
    );
    assert_eq!(pending.polychrome_sprites[0].opacity, 0.5);
    finish_media_tasks(&tree);
    tree.update_styles(Instant::now());
    let ready = draw(&tree, &atlas, 1.);
    assert_eq!(pixel(&atlas, &ready, 0, 2, 5), [0; 4]);
    assert_eq!(pixel(&atlas, &ready, 0, 15, 5), [255, 0, 0, 255]);
}

#[test]
fn background_svg_style_change_and_unmount() {
    use voidui::media::SvgRenderPolicy;
    let mut tree = tree(
        svg()
            .view_box(0, 0, 10, 10)
            .width(10)
            .height(10)
            .render_policy(SvgRenderPolicy::ScaleWhileRendering)
            .child(svg::rect().width(10).height(10)),
        "svg {fill:red} svg:hover rect {fill:blue}",
    );
    let atlas = CpuAtlas::default();
    draw(&tree, &atlas, 1.);
    finish_media_tasks(&tree);
    tree.update_styles(Instant::now());
    let old = draw(&tree, &atlas, 1.);
    tree.set_status(tree.root().unwrap(), WidgetStatus::Hover);
    tree.update_styles(Instant::now());
    let pending = draw(&tree, &atlas, 1.);
    assert_eq!(pixel(&atlas, &pending, 0, 5, 5), [0, 0, 255, 255]);
    finish_media_tasks(&tree);
    tree.update_styles(Instant::now());
    let ready = draw(&tree, &atlas, 1.);
    assert_ne!(
        ready.polychrome_sprites[0].tile,
        old.polychrome_sprites[0].tile
    );
    assert_eq!(pixel(&atlas, &ready, 0, 5, 5), [255, 0, 0, 255]);
    draw(&tree, &atlas, 2.);
    assert_eq!(tree.task_runtime().stats().active, 1);
    tree.build_root(div());
    assert_eq!(tree.task_runtime().stats().active, 0);
    assert_eq!(tree.task_runtime().stats().cancelled, 1);
}

#[test]
fn completed_stale_svg_is_discarded_before_ui_publication() {
    use voidui::{
        media::SvgRenderPolicy,
        tasks::{TaskOptions, TaskRuntime},
    };
    let runtime = TaskRuntime::new(TaskOptions {
        polls_per_tick: 1,
        ..Default::default()
    })
    .unwrap();
    let mut tree = WidgetTree::with_task_runtime(runtime.clone());
    tree.build_root(
        svg()
            .width(10)
            .height(10)
            .view_box(0, 0, 10, 10)
            .render_policy(SvgRenderPolicy::ScaleWhileRendering)
            .child(svg::rect().width(10).height(10).fill("red")),
    );
    tree.layout(Size::MAX_CONTENT, &TextLayoutCache::new(fonts()));
    let atlas = CpuAtlas::default();
    draw(&tree, &atlas, 1.);
    finish_media_tasks(&tree);
    tree.update_styles(Instant::now());
    let first = draw(&tree, &atlas, 1.);
    tree.update_styles(Instant::now());
    draw(&tree, &atlas, 2.);
    runtime.tick();
    // Leave a completed CPU result waiting for its UI continuation. A newer
    // draw must invalidate it even though cancellation can no longer stop it.
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    while runtime.stats().compute_in_flight != 0 || runtime.stats().queued == 0 {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    draw(&tree, &atlas, 3.);
    runtime.tick();
    assert!(!tree.update_styles(Instant::now()).paint);
    let pending = draw(&tree, &atlas, 3.);
    assert_eq!(
        pending.polychrome_sprites[0].tile,
        first.polychrome_sprites[0].tile
    );
    finish_media_tasks(&tree);
    assert!(tree.update_styles(Instant::now()).paint);
    let last = draw(&tree, &atlas, 3.);
    assert_eq!(last.polychrome_sprites[0].tile.bounds.size.width.0, 30);
    assert_eq!(atlas.0.lock().unwrap().len(), 2);
}

#[test]
fn background_svg_admission_failure_is_reported_without_retry_loop() {
    use voidui::{
        media::SvgRenderPolicy,
        tasks::{TaskOptions, TaskRuntime},
    };
    let runtime = TaskRuntime::new(TaskOptions {
        max_tasks: 1,
        ..Default::default()
    })
    .unwrap();
    let occupied = runtime.scope();
    occupied.spawn(std::future::pending::<()>());
    let mut tree = WidgetTree::with_task_runtime(runtime.clone());
    tree.build_root(
        svg()
            .width(10)
            .height(10)
            .render_policy(SvgRenderPolicy::ScaleWhileRendering),
    );
    tree.layout(Size::MAX_CONTENT, &TextLayoutCache::new(fonts()));
    let atlas = CpuAtlas::default();
    let mut scene = Scene::default();
    let mut painter = Painter::new(
        &mut scene,
        &atlas,
        fonts(),
        render::size(render::px(100.), render::px(100.)),
        1.,
    )
    .unwrap();
    for _ in 0..2 {
        assert!(
            tree.draw(&mut painter)
                .unwrap_err()
                .to_string()
                .contains("task capacity")
        );
    }
    assert_eq!(runtime.stats().spawned, 1);
}

#[test]
fn svg_policy_reconciliation_preserves_or_cancels_pending_work() {
    use voidui::media::SvgRenderPolicy;
    let make = |policy| {
        svg()
            .width(10)
            .height(10)
            .view_box(0, 0, 10, 10)
            .render_policy(policy)
            .child(svg::rect().width(10).height(10).fill("blue"))
    };
    let mut tree = tree(make(SvgRenderPolicy::ScaleWhileRendering), "");
    let atlas = CpuAtlas::default();
    draw(&tree, &atlas, 1.);
    tree.reconcile_root(make(SvgRenderPolicy::ScaleWhileRendering));
    tree.layout(Size::MAX_CONTENT, &TextLayoutCache::new(fonts()));
    assert_eq!(tree.task_runtime().stats().cancelled, 0);
    assert!(draw(&tree, &atlas, 1.).polychrome_sprites.is_empty());
    tree.reconcile_root(make(SvgRenderPolicy::Exact));
    tree.layout(Size::MAX_CONTENT, &TextLayoutCache::new(fonts()));
    assert_eq!(tree.task_runtime().stats().cancelled, 1);
    let exact = draw(&tree, &atlas, 1.);
    assert_eq!(pixel(&atlas, &exact, 0, 5, 5), [255, 0, 0, 255]);
    assert!(!tree.task_runtime().stats().background_started);
}

#[test]
fn background_svg_image_checks_limits_before_scheduling() {
    use voidui::media::SvgRenderPolicy;
    let image = Image::from_bytes_with_options(
        br#"<svg width="10" height="10"><rect width="10" height="10"/></svg>"#,
        &SvgOptions {
            limits: MediaLimits {
                max_dimension: 10,
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .unwrap();
    let tree = tree(
        img(image)
            .width(11)
            .height(10)
            .render_policy(SvgRenderPolicy::ScaleWhileRendering),
        "",
    );
    let atlas = CpuAtlas::default();
    let mut scene = Scene::default();
    let mut painter = Painter::new(
        &mut scene,
        &atlas,
        fonts(),
        render::size(render::px(100.), render::px(100.)),
        1.,
    )
    .unwrap();
    assert!(
        tree.draw(&mut painter)
            .unwrap_err()
            .to_string()
            .contains("MediaLimits")
    );
    assert_eq!(tree.task_runtime().stats().spawned, 0);
}
