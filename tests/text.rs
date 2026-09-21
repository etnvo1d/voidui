//! Real font shaping and CPU scene tests; no window, GPU, or system fonts required.
use std::{
    borrow::Cow,
    collections::HashMap,
    sync::{Arc, Mutex},
};

use voidui::{
    core::{
        context::LayoutContext,
        element::{ElementProps, IntoElement},
        layout::{
            self, AvailableSpace, BoxSizing, Display, LayoutInput, LayoutOutput, LayoutPartialTree,
            RunMode, Size, TaffyMaxContent, TraversePartialTree, fr, length,
        },
        text::{Text as TextContent, TextLayoutOptions},
        widget::{Widget, WidgetBuilder},
        widget_tree::WidgetTree,
    },
    div,
    render::{
        self, AtlasKey, AtlasTextureId, AtlasTile, Bounds, DevicePixels, Hsla, Painter,
        ParleyTextSystem, PlatformAtlas, Result, Scene, SharedString, TextAlign, TextLayoutCache,
        TextSystem, TileId, font, point, px, size,
    },
    style::{color::Rgba8, style::Style, text::LineHeight},
    text,
};

const FONT: &str = "IBM Plex Sans";

fn system() -> Arc<TextSystem> {
    let backend = ParleyTextSystem::new_without_system_fonts(FONT);
    backend
        .add_fonts(vec![Cow::Borrowed(include_bytes!(
            "../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
        ))])
        .unwrap();
    Arc::new(TextSystem::new(Arc::new(backend)))
}

fn options() -> TextLayoutOptions {
    TextLayoutOptions {
        font: font(FONT),
        color: render::black(),
        font_size: 16.0,
        line_height: 24.0,
        wrap_width: None,
        line_clamp: None,
    }
}

fn build(element: impl IntoElement) -> (WidgetTree, TextLayoutCache, Arc<TextSystem>) {
    let system = system();
    let cache = TextLayoutCache::new(system.clone());
    let mut tree = WidgetTree::new();
    tree.build_root(element);
    tree.layout(Size::MAX_CONTENT, &cache);
    (tree, cache, system)
}

fn close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.01,
        "expected {expected}, got {actual}"
    );
}

#[derive(Default)]
struct CpuAtlas(Mutex<HashMap<AtlasKey, AtlasTile>>);

impl PlatformAtlas for CpuAtlas {
    fn get_or_insert_with<'a>(
        &self,
        key: &AtlasKey,
        build: &mut dyn FnMut() -> Result<Option<(render::Size<DevicePixels>, Cow<'a, [u8]>)>>,
    ) -> Result<Option<AtlasTile>> {
        let mut tiles = self.0.lock().unwrap();
        if let Some(tile) = tiles.get(key) {
            return Ok(Some(*tile));
        }
        let Some((dimensions, bytes)) = build()? else {
            return Ok(None);
        };
        assert!(bytes.iter().any(|value| *value != 0));
        let tile = AtlasTile {
            texture_id: AtlasTextureId {
                index: 0,
                kind: key.texture_kind(),
            },
            tile_id: TileId(tiles.len() as u32),
            padding: 0,
            bounds: Bounds::new(point(DevicePixels(0), DevicePixels(0)), dimensions),
        };
        tiles.insert(key.clone(), tile);
        Ok(Some(tile))
    }
    fn remove(&self, key: &AtlasKey) {
        self.0.lock().unwrap().remove(key);
    }
}

fn paint(tree: &WidgetTree, system: Arc<TextSystem>) -> Scene {
    let mut scene = Scene::default();
    let atlas = CpuAtlas::default();
    let mut painter =
        Painter::new(&mut scene, &atlas, system, size(px(1000.), px(1000.)), 1.0).unwrap();
    tree.draw(&mut painter).unwrap();
    drop(painter);
    scene.finish();
    scene
}

#[test]
fn requested_apis_produce_identical_text_layout_and_real_glyphs() {
    let color = Rgba8::new(20, 100, 180, 128);
    let (tree, _, system) = build(
        div()
            .font(FONT)
            .color(color)
            .line_height(24.0)
            .child("Hello ffi")
            .child(text("Hello ffi").color(color).font(FONT)),
    );
    let root = tree.root().unwrap();
    let children = tree.children(root);
    assert_eq!(tree.bounds(children[0]).size, tree.bounds(children[1]).size);
    assert_eq!(tree.text_style(children[0]), tree.text_style(children[1]));
    assert!(tree.bounds(root).size.width > 0.0);
    close(tree.bounds(root).size.height, 48.0);
    let scene = paint(&tree, system);
    assert!(!scene.monochrome_sprites.is_empty());
    let expected: Hsla = voidui::style::color::Color::from(color).into();
    assert!(
        scene
            .monochrome_sprites
            .iter()
            .all(|sprite| sprite.color == expected)
    );
    close(expected.a, 128.0 / 255.0);
}

#[test]
#[allow(clippy::needless_borrows_for_generic_args)] // Exercise the borrowed SharedString API.
fn borrowed_and_owned_string_children_own_their_content() {
    let element = {
        let value = String::from("same");
        div()
            .font(FONT)
            .child(value.as_str())
            .child(&value)
            .child(value.clone())
            .child(SharedString::new(&value))
            .child(&SharedString::new(&value))
            .child(Arc::<str>::from(value.as_str()))
            .child(Cow::Borrowed(value.as_str()))
    };
    let (tree, _, _) = build(element);
    let children = tree.children(tree.root().unwrap());
    assert_eq!(children.len(), 7);
    let first = tree.bounds(children[0]).size;
    for child in children {
        assert_eq!(tree.bounds(*child).size, first);
    }
    let owned = text(String::from("owned"));
    assert_eq!(owned.widget.content().as_str(), "owned");
}

#[test]
fn typography_inherits_through_divs_and_child_overrides_win() {
    let red = Rgba8::from_rgb8(255, 0, 0);
    let blue = Rgba8::from_rgb8(0, 0, 255);
    let (mut tree, cache, _) = build(
        div()
            .font(FONT)
            .color(red)
            .font_size(20.0)
            .line_height(LineHeight::Relative(1.5))
            .child(
                div()
                    .child("inherited")
                    .child(text("override").color(blue).font_size(10.0)),
            ),
    );
    let root = tree.root().unwrap();
    let container = tree.children(root)[0];
    let a = tree.children(container)[0];
    let b = tree.children(container)[1];
    assert_eq!(tree.text_style(a).font.family.as_str(), FONT);
    assert_eq!(tree.text_style(a).color, red.into());
    assert_eq!(tree.text_style(b).color, blue.into());
    close(tree.bounds(a).size.height, 30.0);
    close(tree.bounds(b).size.height, 15.0);
    tree.style_mut(root).font_size = 30.0.into();
    tree.style_mut(root).color = blue.into();
    tree.layout(Size::MAX_CONTENT, &cache);
    close(tree.bounds(a).size.height, 45.0);
    close(tree.bounds(b).size.height, 15.0);
    assert_eq!(tree.text_style(a).color, blue.into());
}

#[test]
fn text_wraps_in_block_flex_and_grid_and_reflows_after_resize() {
    let label = "alpha beta gamma delta epsilon zeta";
    for display in [Display::Block, Display::Flex, Display::Grid] {
        let (mut tree, cache, _) = build(
            div()
                .display(display)
                .font(FONT)
                .line_height(24.0)
                .width(100)
                .grid_template_columns(vec![fr(1.0)])
                .child(label),
        );
        let root = tree.root().unwrap();
        let child = tree.children(root)[0];
        let mut opt = options();
        opt.wrap_width = Some(100.0);
        let expected = TextContent::new(label).shape(&cache, &opt).unwrap().size();
        assert!(expected.height > 24.0);
        close(tree.bounds(child).size.height, expected.height);
        tree.style_mut(root).layout.size.width = length(300);
        tree.layout(Size::MAX_CONTENT, &cache);
        opt.wrap_width = Some(300.0);
        let expected = TextContent::new(label).shape(&cache, &opt).unwrap().size();
        close(tree.bounds(child).size.height, expected.height);
        assert!(expected.height < 72.0);
    }
}

#[test]
fn nowrap_preserves_hard_breaks_but_does_not_soft_wrap() {
    let (tree, _, system) = build(
        div()
            .font(FONT)
            .line_height(24.0)
            .width(30)
            .child(text("a long line\nsecond line").wrap(false)),
    );
    let child = tree.children(tree.root().unwrap())[0];
    close(tree.bounds(child).size.height, 48.0);
    let scene = paint(&tree, system);
    assert!(!scene.monochrome_sprites.is_empty());
    for sprite in &scene.monochrome_sprites {
        close(sprite.content_mask.bounds.size.width.0, 30.0);
    }
}

#[test]
fn empty_unicode_and_explicit_blank_lines_are_measured() {
    let (tree, _, _) = build(
        div()
            .font(FONT)
            .line_height(24.0)
            .child("")
            .child("\n")
            .child("first\r\n\r\nlast\n")
            .child("office ffi e\u{301} 你好 👩‍💻"),
    );
    let children = tree.children(tree.root().unwrap());
    close(tree.bounds(children[0]).size.height, 0.0);
    close(tree.bounds(children[1]).size.height, 48.0);
    close(tree.bounds(children[2]).size.height, 96.0);
    assert!(tree.bounds(children[3]).size.width > 0.0);
    close(tree.bounds(children[3]).size.height, 24.0);
}

#[test]
fn padding_border_and_margin_use_content_box_for_measurement_and_paint() {
    let (plain, _, plain_system) = build(div().font(FONT).line_height(24.0).child("Hello"));
    let plain_scene = paint(&plain, plain_system);
    let (tree, _, system) = build(
        div()
            .font(FONT)
            .line_height(24.0)
            .padding(7.0)
            .child(text("Hello").padding(10.0).border_width(2.0).margin(3.0)),
    );
    let child = tree.children(tree.root().unwrap())[0];
    close(tree.bounds(child).size.height, 48.0);
    let scene = paint(&tree, system);
    assert_eq!(
        scene.monochrome_sprites.len(),
        plain_scene.monochrome_sprites.len()
    );
    for (actual, expected) in scene
        .monochrome_sprites
        .iter()
        .zip(&plain_scene.monochrome_sprites)
    {
        close((actual.bounds.origin.x - expected.bounds.origin.x).0, 22.0);
        close((actual.bounds.origin.y - expected.bounds.origin.y).0, 22.0);
        close(actual.content_mask.bounds.origin.x.0, 22.0);
        close(actual.content_mask.bounds.origin.y.0, 22.0);
    }
}

#[test]
fn border_box_wraps_at_width_after_padding_and_border() {
    let label = "alpha beta gamma delta epsilon";
    let (tree, cache, _) = build(
        div().font(FONT).line_height(24.0).child(
            text(label)
                .width(100)
                .box_sizing(BoxSizing::BorderBox)
                .padding(8.0)
                .border_width(2.0),
        ),
    );
    let mut opt = options();
    opt.wrap_width = Some(80.0);
    let expected = TextContent::new(label).shape(&cache, &opt).unwrap().size();
    let child = tree.children(tree.root().unwrap())[0];
    close(tree.bounds(child).size.width, 100.0);
    close(tree.bounds(child).size.height, expected.height + 20.0);
}

#[test]
fn min_content_is_nonzero_and_smaller_than_max_content() {
    let (mut tree, cache, _) = build(text("alpha beta gamma").font(FONT).line_height(24.0));
    let root = tree.root().unwrap();
    let maximum = tree.bounds(root).size.width;
    tree.layout(
        Size {
            width: AvailableSpace::MinContent,
            height: AvailableSpace::MaxContent,
        },
        &cache,
    );
    let minimum = tree.bounds(root).size.width;
    assert!(
        minimum > 20.0 && minimum < maximum,
        "min {minimum}, max {maximum}"
    );
    let opt = options();
    close(
        minimum,
        TextContent::new("alpha beta gamma").min_content_width(&cache, &opt),
    );
    assert!(tree.bounds(root).size.height > 24.0);
}

#[test]
fn nonbreaking_space_is_not_a_min_content_break() {
    let cache = TextLayoutCache::new(system());
    let value = TextContent::new("alpha\u{a0}beta");
    let opt = options();
    close(
        value.min_content_width(&cache, &opt),
        value.shape(&cache, &opt).unwrap().size().width,
    );
}

#[test]
fn flex_uses_text_baselines_including_padding() {
    let (tree, _, _) = build(
        div()
            .flex()
            .font(FONT)
            .align_items(layout::AlignItems::BASELINE)
            .child(
                text("small")
                    .font_size(12.0)
                    .line_height(18.0)
                    .padding_top(4),
            )
            .child(text("large").font_size(28.0).line_height(40.0)),
    );
    let children = tree.children(tree.root().unwrap());
    let a = tree.bounds(children[0]).origin.y + 4.0;
    let b = tree.bounds(children[1]).origin.y;
    // Taffy must align the baselines used by the painter's shaped lines. The
    // backend's global font metrics can differ from per-line fallback metrics.
    let cache = TextLayoutCache::new(system());
    let small = TextContent::new("small")
        .shape(
            &cache,
            &TextLayoutOptions {
                font_size: 12.0,
                line_height: 18.0,
                ..options()
            },
        )
        .unwrap();
    let large = TextContent::new("large")
        .shape(
            &cache,
            &TextLayoutOptions {
                font_size: 28.0,
                line_height: 40.0,
                ..options()
            },
        )
        .unwrap();
    close(
        a + small.first_baseline().unwrap(),
        b + large.first_baseline().unwrap(),
    );
}

#[test]
fn text_alignment_changes_glyph_positions_without_changing_layout() {
    let (mut tree, cache, system) = build(div().font(FONT).width(200).child("align"));
    let root = tree.root().unwrap();
    let before = tree.bounds(root);
    let left = paint(&tree, system.clone());
    tree.style_mut(root).text_align = TextAlign::Right.into();
    tree.layout(Size::MAX_CONTENT, &cache);
    let right = paint(&tree, system);
    assert_eq!(before, tree.bounds(root));
    assert!(
        right.monochrome_sprites[0].bounds.origin.x
            > left.monochrome_sprites[0].bounds.origin.x + render::ScaledPixels(100.0)
    );
}

#[test]
fn hiding_and_showing_text_clears_layout_and_skips_drawing() {
    let (mut tree, cache, system) = build(div().font(FONT).child("visible"));
    let root = tree.root().unwrap();
    tree.style_mut(root).layout.display = Display::None;
    tree.layout(Size::MAX_CONTENT, &cache);
    assert!(paint(&tree, system.clone()).monochrome_sprites.is_empty());
    tree.style_mut(root).layout.display = Display::Block;
    tree.layout(Size::MAX_CONTENT, &cache);
    assert!(!paint(&tree, system).monochrome_sprites.is_empty());
}

#[test]
fn repeated_layout_and_frame_changes_preserve_glyph_positions() {
    let (mut tree, cache, system) =
        build(div().font(FONT).width(100).child("alpha beta gamma delta"));
    let first = paint(&tree, system.clone());
    for _ in 0..3 {
        cache.finish_frame();
        tree.layout(Size::MAX_CONTENT, &cache);
        let current = paint(&tree, system.clone());
        let first_positions: Vec<_> = first.monochrome_sprites.iter().map(|s| s.bounds).collect();
        let current_positions: Vec<_> = current
            .monochrome_sprites
            .iter()
            .map(|s| s.bounds)
            .collect();
        assert_eq!(first_positions, current_positions);
    }
}

struct ProbeAfterLayout;
impl Widget for ProbeAfterLayout {
    fn layout(&mut self, inputs: LayoutInput, mut ctx: LayoutContext<'_, '_>) -> LayoutOutput {
        let root = ctx.node_id();
        let output = layout::layout_flex(&mut ctx, root, inputs);
        if inputs.run_mode == RunMode::PerformLayout {
            let child = ctx.get_child_id(root, 0);
            // A later size-only query must not replace the child's final wrapped lines.
            ctx.compute_child_layout(
                child,
                LayoutInput {
                    run_mode: RunMode::ComputeSize,
                    available_space: Size {
                        width: AvailableSpace::Definite(71.0),
                        height: AvailableSpace::MaxContent,
                    },
                    ..LayoutInput::HIDDEN
                },
            );
        }
        output
    }
}

#[test]
fn late_measurement_probes_do_not_replace_prepared_drawing() {
    let content = "alpha beta gamma delta epsilon";
    let (ordinary, _, ordinary_system) = build(div().flex().font(FONT).width(120).child(content));
    let element = WidgetBuilder {
        events: Default::default(),
        widget: ProbeAfterLayout,
        props: ElementProps::new(Style::default()),
        children: vec![content.into_element()],
    }
    .font(FONT)
    .flex()
    .width(120);
    let (probed, _, probed_system) = build(element);
    let ordinary_scene = paint(&ordinary, ordinary_system);
    let probed_scene = paint(&probed, probed_system);
    let ordinary_positions: Vec<_> = ordinary_scene
        .monochrome_sprites
        .iter()
        .map(|s| s.bounds)
        .collect();
    let probed_positions: Vec<_> = probed_scene
        .monochrome_sprites
        .iter()
        .map(|s| s.bounds)
        .collect();
    assert_eq!(ordinary_positions, probed_positions);
}

#[test]
fn drawing_before_layout_or_after_style_changes_reports_an_error() {
    let system = system();
    let mut tree = WidgetTree::new();
    let root = tree.build_root(div().font(FONT).child("text"));
    let atlas = CpuAtlas::default();
    let mut scene = Scene::default();
    let mut painter = Painter::new(
        &mut scene,
        &atlas,
        system.clone(),
        size(px(300.), px(300.)),
        1.0,
    )
    .unwrap();
    assert!(tree.draw(&mut painter).is_err());
    tree.layout(Size::MAX_CONTENT, &TextLayoutCache::new(system));
    assert!(tree.draw(&mut painter).is_ok());
    tree.style_mut(root).font_size = 20.0.into();
    assert!(tree.draw(&mut painter).is_err());
}

#[test]
fn changing_text_content_invalidates_prepared_measurement() {
    let mut widget = text("old").font(FONT);
    widget.widget.set_content(String::from("much longer label"));
    assert_eq!(widget.widget.content().as_str(), "much longer label");
    let (tree, cache, _) = build(widget);
    let expected = TextContent::new("much longer label")
        .shape(
            &cache,
            &TextLayoutOptions {
                line_height: 19.2,
                ..options()
            },
        )
        .unwrap()
        .size();
    close(tree.bounds(tree.root().unwrap()).size.width, expected.width);
}

#[test]
fn core_text_line_clamp_counts_hard_and_soft_breaks() {
    let cache = TextLayoutCache::new(system());
    let prepared = TextContent::new("alpha beta gamma delta\nsecond\nthird")
        .shape(
            &cache,
            &TextLayoutOptions {
                wrap_width: Some(60.0),
                line_clamp: Some(2),
                ..options()
            },
        )
        .unwrap();
    close(prepared.size().height, 48.0);
}

#[test]
fn changing_font_descriptor_and_size_reflows_and_repaints() {
    let (mut tree, cache, system) = build(
        text("Hello")
            .font_options(font(FONT).italic())
            .font_size(12.0),
    );
    let root = tree.root().unwrap();
    assert_eq!(tree.text_style(root).font.style, render::FontStyle::Italic);
    let small = tree.bounds(root).size;
    tree.style_mut(root).set_font(font(FONT));
    tree.style_mut(root).font_size = 28.0.into();
    tree.layout(Size::MAX_CONTENT, &cache);
    assert!(tree.bounds(root).size.width > small.width);
    assert!(tree.bounds(root).size.height > small.height);
    assert!(!paint(&tree, system).monochrome_sprites.is_empty());
}

#[test]
fn core_measure_errors_clear_previous_prepared_data() {
    let system = system();
    let cache = TextLayoutCache::new(system.clone());
    let mut text = TextContent::new("prepared");
    text.measure(&cache, &options()).unwrap();
    assert!(
        text.measure(
            &cache,
            &TextLayoutOptions {
                font_size: f32::NAN,
                ..options()
            }
        )
        .is_err()
    );
    let atlas = CpuAtlas::default();
    let mut scene = Scene::default();
    let mut painter =
        Painter::new(&mut scene, &atlas, system, size(px(300.), px(300.)), 1.0).unwrap();
    assert!(
        text.paint(
            &mut painter,
            voidui::core::geometry::Rect::from_xywh(0.0, 0.0, 200.0, 100.0),
            TextAlign::Left
        )
        .is_err()
    );
}

#[test]
fn glyph_atlas_errors_reach_the_draw_caller() {
    struct FailingAtlas;
    impl PlatformAtlas for FailingAtlas {
        fn get_or_insert_with<'a>(
            &self,
            _: &AtlasKey,
            _: &mut dyn FnMut() -> Result<Option<(render::Size<DevicePixels>, Cow<'a, [u8]>)>>,
        ) -> Result<Option<AtlasTile>> {
            Err(std::io::Error::other("test atlas failure").into())
        }
        fn remove(&self, _: &AtlasKey) {}
    }
    let (tree, _, system) = build(text("glyph").font(FONT));
    let mut scene = Scene::default();
    let mut painter = Painter::new(
        &mut scene,
        &FailingAtlas,
        system,
        size(px(300.), px(300.)),
        1.0,
    )
    .unwrap();
    let error = tree.draw(&mut painter).unwrap_err();
    assert!(error.to_string().contains("test atlas failure"));
}

#[test]
fn zero_width_text_remains_finite_and_does_not_emit_visible_glyphs() {
    let (tree, _, system) = build(div().width(0).font(FONT).child("text"));
    let bounds = tree.bounds(tree.children(tree.root().unwrap())[0]);
    assert_eq!(bounds.size.width, 0.0);
    assert!(bounds.size.height.is_finite());
    assert!(paint(&tree, system).monochrome_sprites.is_empty());
}

#[test]
#[should_panic(expected = "font_size must be finite and positive")]
fn invalid_font_size_is_rejected_at_the_builder() {
    text("text").font_size(-1.0);
}

#[test]
#[should_panic(expected = "line_height must be finite and positive")]
fn invalid_line_height_is_rejected_at_the_builder() {
    text("text").line_height(f32::INFINITY);
}

#[test]
fn fontless_empty_and_hidden_text_do_not_resolve_fonts() {
    let system = Arc::new(TextSystem::new(Arc::new(
        ParleyTextSystem::new_without_system_fonts("unused"),
    )));
    let cache = TextLayoutCache::new(system.clone());
    let mut tree = WidgetTree::new();
    tree.build_root(
        div()
            .child("")
            .child(div().display(Display::None).child("hidden")),
    );
    tree.layout(Size::MAX_CONTENT, &cache);
    assert!(paint(&tree, system).monochrome_sprites.is_empty());
}

#[test]
fn paragraph_shaping_matches_independent_lines_with_different_byte_lengths() {
    let (tree, cache, system) = build(
        div()
            .font(FONT)
            .line_height(24.0)
            .child("a\nlong middle paragraph\nffi e\u{301}"),
    );
    let root = tree.root().unwrap();
    let expected_width = ["a", "long middle paragraph", "ffi e\u{301}"]
        .into_iter()
        .map(|text| {
            TextContent::new(text)
                .shape(&cache, &options())
                .unwrap()
                .size()
                .width
        })
        .fold(0.0f32, f32::max);
    close(tree.bounds(root).size.width, expected_width);
    close(tree.bounds(root).size.height, 72.0);
    let scene = paint(&tree, system);
    assert!(!scene.monochrome_sprites.is_empty());
}

#[test]
fn bare_text_uses_backend_system_font_defaults() {
    let (tree, _, system) = build(div().child("text"));
    let child = tree.children(tree.root().unwrap())[0];
    assert!(tree.bounds(child).size.width > 0.0);
    close(tree.bounds(child).size.height, 16.0 * 1.2);
    assert!(!paint(&tree, system).monochrome_sprites.is_empty());
}

#[test]
fn retained_parley_paragraph_does_not_reshape_for_width_color_or_selection() {
    let (mut tree, cache, system) = build(
        text("A retained paragraph changes width without another shape.")
            .font(FONT)
            .width(200),
    );
    let id = tree.root().unwrap();
    let _ = paint(&tree, system.clone());
    let initial = system.stats().paragraphs_shaped;
    for width in [120.0, 240.0, 180.0] {
        tree.style_mut(id).layout.size.width = layout::length(width);
        tree.layout(Size::MAX_CONTENT, &cache);
        let _ = paint(&tree, system.clone());
        assert_eq!(system.stats().paragraphs_shaped, initial);
    }
    tree.style_mut(id).color = Rgba8::from_rgb8(200, 30, 20).into();
    let change = tree.update_styles(std::time::Instant::now());
    assert!(!change.layout);
    tree.select_all();
    let _ = paint(&tree, system.clone());
    assert_eq!(system.stats().paragraphs_shaped, initial);
}

#[test]
fn identical_labels_share_parley_shaping_across_intrinsic_probes() {
    let s = system();
    let cache = TextLayoutCache::new(s.clone());
    s.prewarm_fonts(&[font(FONT), render::Font::default()]);
    let before = s.stats().paragraphs_shaped;
    let mut content = div().font(FONT).width(300);
    for _ in 0..100 {
        content = content.child(text("The same retained label."));
    }
    let mut tree = WidgetTree::new();
    tree.build_root(content);
    tree.layout(Size::MAX_CONTENT, &cache);
    assert_eq!(
        s.stats().paragraphs_shaped - before,
        1,
        "identical labels reshaped instead of using the paragraph pool"
    );
    cache.finish_frame();
    tree.style_mut(tree.root().unwrap()).layout.size.width = layout::length(180.0);
    tree.layout(Size::MAX_CONTENT, &cache);
    assert_eq!(
        s.stats().paragraphs_shaped - before,
        1,
        "resizing repeated labels reshaped their text"
    );
}
