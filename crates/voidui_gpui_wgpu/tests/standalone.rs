use std::{
    borrow::Cow,
    collections::HashMap,
    sync::{Arc, Mutex},
};
use voidui_gpui_wgpu::*;

fn text_system() -> Arc<TextSystem> {
    let backend = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    backend
        .add_fonts(vec![Cow::Borrowed(include_bytes!(
            "fonts/IBMPlexSans-Regular.ttf"
        ))])
        .unwrap();
    Arc::new(TextSystem::new(Arc::new(backend)))
}
#[derive(Default)]
struct CpuAtlas(Mutex<HashMap<AtlasKey, AtlasTile>>);
impl PlatformAtlas for CpuAtlas {
    fn get_or_insert_with<'a>(
        &self,
        key: &AtlasKey,
        build: &mut dyn FnMut() -> Result<Option<(Size<DevicePixels>, Cow<'a, [u8]>)>>,
    ) -> Result<Option<AtlasTile>> {
        let mut tiles = self.0.lock().unwrap();
        if let Some(tile) = tiles.get(key) {
            return Ok(Some(*tile));
        }
        let Some((dimensions, bytes)) = build()? else {
            return Ok(None);
        };
        let bpp = if key.texture_kind() == AtlasTextureKind::Monochrome {
            1
        } else {
            4
        };
        assert_eq!(
            bytes.len(),
            dimensions.width.0 as usize * dimensions.height.0 as usize * bpp
        );
        assert!(bytes.iter().any(|&b| b != 0));
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
#[test]
fn shared_strings_hash_and_compare_by_content() {
    let a = SharedString::new_static("same");
    let b = SharedString::new("same");
    assert_eq!(a, b);
    let map = HashMap::from([(a, 42)]);
    assert_eq!(map.get(&b), Some(&42));
    assert_eq!(map.get("same"), Some(&42));
}
#[test]
fn text_shapes_rasterizes_and_becomes_scene_sprites_without_gpui_runtime() {
    let text = text_system();
    let layout = TextLayoutCache::new(text.clone());
    let atlas = CpuAtlas::default();
    let mut scene = Scene::default();
    let label = "office ffi e\u{301}";
    let line = layout
        .shape_paragraph(
            label.into(),
            &[TextRun {
                len: label.len(),
                font: font("IBM Plex Sans"),
                color: white(),
                underline: Some(UnderlineStyle {
                    thickness: px(1.),
                    color: Some(white()),
                    wavy: true,
                }),
                ..Default::default()
            }],
            22.0,
            32.0,
            None,
            None,
        )
        .unwrap();
    assert!(line.width() > 0.0);
    for _ in 0..2 {
        scene.clear();
        let mut painter = Painter::new(
            &mut scene,
            &atlas,
            text.clone(),
            size(px(400.), px(100.)),
            2.,
        )
        .unwrap();
        line.paint(
            &mut painter,
            Bounds::new(point(10.0, 10.0), size(390.0, 90.0)),
            TextAlign::Left,
            None,
            None,
        )
        .unwrap();
        scene.finish();
        assert!(!scene.monochrome_sprites.is_empty());
        assert!(!scene.underlines.is_empty());
    }
    assert!(atlas.0.lock().unwrap().len() <= scene.monochrome_sprites.len());
    layout.finish_frame();
}
#[test]
fn nested_clips_restore_and_scale_exactly() {
    let text = text_system();
    let atlas = CpuAtlas::default();
    let mut scene = Scene::default();
    let rect = Bounds::new(point(px(0.), px(0.)), size(px(100.), px(100.)));
    let mut painter = Painter::new(&mut scene, &atlas, text, rect.size, 2.).unwrap();
    painter.with_clip(
        Bounds::new(point(px(10.), px(10.)), size(px(30.), px(30.))),
        |p| {
            p.with_clip(
                Bounds::new(point(px(20.), px(20.)), size(px(80.), px(80.))),
                |p| p.paint_quad(fill(rect, white())),
            );
            p.paint_quad(fill(rect, white()));
        },
    );
    painter.paint_quad(fill(rect, white()));
    scene.finish();
    assert_eq!(
        scene.quads[0].content_mask.bounds,
        Bounds::new(
            point(ScaledPixels(40.), ScaledPixels(40.)),
            size(ScaledPixels(40.), ScaledPixels(40.))
        )
    );
    assert_eq!(
        scene.quads[1].content_mask.bounds.size.width,
        ScaledPixels(60.)
    );
    assert_eq!(
        scene.quads[2].content_mask.bounds.size.width,
        ScaledPixels(200.)
    );
    assert!(
        scene.quads[0].order < scene.quads[1].order && scene.quads[1].order < scene.quads[2].order
    );
}
#[test]
fn wrapping_and_cache_survive_frame_boundaries() {
    let text = text_system();
    let layout = TextLayoutCache::new(text);
    let label = "A long line wraps into several rows.\nA separate paragraph.";
    let runs = [TextRun {
        len: label.len(),
        font: font("IBM Plex Sans"),
        color: white(),
        ..Default::default()
    }];
    let a = layout
        .prepare(label.into(), &runs[0].font, 18.0, 26.0, Some(120.0), None)
        .unwrap();
    assert!(a.line_count() > 2);
    layout.finish_frame();
    let b = layout
        .prepare(label.into(), &runs[0].font, 18.0, 26.0, Some(120.0), None)
        .unwrap();
    assert!(Arc::ptr_eq(&a, &b));
    assert_eq!(a.width(), b.width());
}
#[test]
fn invalid_dpi_is_rejected() {
    let text = text_system();
    let atlas = CpuAtlas::default();
    let mut scene = Scene::default();
    for scale in [0., -1., f32::NAN, f32::INFINITY] {
        assert!(
            Painter::new(
                &mut scene,
                &atlas,
                text.clone(),
                size(px(10.), px(10.)),
                scale
            )
            .is_err()
        );
    }
}

#[test]
fn rich_foregrounds_decorations_and_selection_reach_the_scene() {
    let text = text_system();
    let atlas = CpuAtlas::default();
    let inherited = hsla(0.33, 1.0, 0.5, 1.0);
    let explicit = hsla(0.0, 1.0, 0.5, 1.0);
    let selected = hsla(0.66, 1.0, 0.5, 1.0);
    let runs: Vec<_> = [false, true]
        .into_iter()
        .enumerate()
        .map(|(index, color_is_explicit)| TextRun {
            len: 1,
            font: font("IBM Plex Sans"),
            font_size: Some(if index == 0 { 16.0 } else { 32.0 }),
            color: explicit,
            color_is_explicit,
            underline: Some(UnderlineStyle {
                thickness: px(1.0),
                color: None,
                wavy: false,
            }),
            strikethrough: Some(StrikethroughStyle {
                thickness: px(1.0),
                color: None,
            }),
            ..Default::default()
        })
        .collect();
    let p = text
        .shape_paragraph("ab".into(), &runs, 16.0, 48.0, None, None)
        .unwrap();
    let before = text.stats().paragraphs_shaped;
    for selection in [None, Some((0..2, selected, white()))] {
        let mut scene = Scene::default();
        let mut painter = Painter::new(
            &mut scene,
            &atlas,
            text.clone(),
            size(px(200.0), px(100.0)),
            1.0,
        )
        .unwrap();
        p.paint(
            &mut painter,
            Bounds::new(point(0.0, 0.0), size(200.0, 100.0)),
            TextAlign::Left,
            Some(inherited),
            selection.clone(),
        )
        .unwrap();
        scene.finish();
        assert!(!scene.monochrome_sprites.is_empty());
        if selection.is_some() {
            assert!(
                scene
                    .monochrome_sprites
                    .iter()
                    .all(|sprite| sprite.color == selected)
            );
        } else {
            assert!(
                scene
                    .monochrome_sprites
                    .iter()
                    .any(|sprite| sprite.color == inherited)
            );
            assert!(
                scene
                    .monochrome_sprites
                    .iter()
                    .any(|sprite| sprite.color == explicit)
            );
        }
        assert_eq!(scene.underlines.len(), 4);
        for (index, item) in p.layout().get(0).unwrap().items().enumerate() {
            let parley::PositionedLayoutItem::GlyphRun(run) = item else {
                continue;
            };
            let expected = if index == 0 { inherited } else { explicit };
            let strike_y = run.baseline() - run.run().metrics().strikethrough_offset;
            // Scene batches may reorder decorations; identify them by geometry.
            assert!(scene.underlines.iter().any(|decoration| {
                decoration.color == expected
                    && decoration.bounds.origin.x == ScaledPixels(run.offset())
                    && decoration.bounds.origin.y == ScaledPixels(strike_y)
            }));
            assert!(scene.underlines.iter().any(|decoration| {
                decoration.color == expected
                    && decoration.bounds.origin.x == ScaledPixels(run.offset())
                    && decoration.bounds.origin.y
                        == ScaledPixels(run.baseline() + run.run().metrics().underline_offset.abs())
            }));
        }
    }
    assert_eq!(text.stats().paragraphs_shaped, before);
}

#[test]
fn mixed_row_paint_matches_independent_uniform_rows() {
    let text = text_system();
    let atlas = CpuAtlas::default();
    let colors = [
        hsla(0.0, 1.0, 0.5, 1.0),
        hsla(0.33, 1.0, 0.5, 1.0),
        hsla(0.66, 1.0, 0.5, 1.0),
    ];
    // Both ordinary and tight leading must move glyphs, decorations, backgrounds
    // and selection rectangles together, without changing raster identities.
    for (sizes, heights) in [
        ([16.0, 32.0, 12.0], [24.0, 48.0, 20.0]),
        ([48.0, 64.0, 32.0], [2.0, 3.0, 4.0]),
    ] {
        let runs: Vec<_> = (0..3)
            .map(|i| TextRun {
                len: if i == 2 { 1 } else { 2 },
                font: font("IBM Plex Sans"),
                font_size: Some(sizes[i]),
                line_height: Some(heights[i]),
                color: colors[i],
                color_is_explicit: true,
                background_color: Some(colors[i]),
                underline: Some(UnderlineStyle {
                    thickness: px(1.0),
                    color: None,
                    wavy: false,
                }),
                strikethrough: Some(StrikethroughStyle {
                    thickness: px(1.0),
                    color: None,
                }),
                ..Default::default()
            })
            .collect();
        let p = text
            .shape_paragraph("a\nb\nc".into(), &runs, 16.0, 24.0, None, None)
            .unwrap();
        for selected in [false, true] {
            let mut combined = Scene::default();
            let mut expected = Scene::default();
            let mut painter = Painter::new(
                &mut combined,
                &atlas,
                text.clone(),
                size(px(300.0), px(300.0)),
                1.0,
            )
            .unwrap();
            p.paint(
                &mut painter,
                Bounds::new(point(0.0, 0.0), size(300.0, 300.0)),
                TextAlign::Left,
                None,
                selected.then_some((0..5, white(), black())),
            )
            .unwrap();
            let mut top = 0.0;
            for (row, ch) in ["a", "b", "c"].into_iter().enumerate() {
                let mut run = runs[row].clone();
                run.len = 1;
                let single = text
                    .shape_paragraph(ch.into(), &[run], 16.0, 24.0, None, None)
                    .unwrap();
                let mut painter = Painter::new(
                    &mut expected,
                    &atlas,
                    text.clone(),
                    size(px(300.0), px(300.0)),
                    1.0,
                )
                .unwrap();
                single
                    .paint(
                        &mut painter,
                        Bounds::new(point(0.0, top), size(300.0, 300.0)),
                        TextAlign::Left,
                        None,
                        selected.then_some((0..1, white(), black())),
                    )
                    .unwrap();
                top += heights[row];
            }
            combined.finish();
            expected.finish();
            assert_eq!(
                combined.monochrome_sprites.len(),
                expected.monochrome_sprites.len()
            );
            for sprite in &combined.monochrome_sprites {
                assert!(expected.monochrome_sprites.iter().any(|other| {
                    sprite.color == other.color
                        && sprite.tile == other.tile
                        && sprite.bounds == other.bounds
                }));
            }
            assert_eq!(combined.underlines.len(), expected.underlines.len());
            for decoration in &combined.underlines {
                assert!(expected.underlines.iter().any(|other| {
                    decoration.color == other.color
                        && (decoration.bounds.origin.y.0 - other.bounds.origin.y.0).abs() < 0.001
                        && decoration.bounds.size == other.bounds.size
                }));
            }
            // Selection adds a newline marker horizontally; its vertical bounds
            // must still equal the independently positioned display row.
            for quad in &combined.quads {
                assert!(expected.quads.iter().any(|other| {
                    quad.background == other.background
                        && (quad.bounds.origin.y.0 - other.bounds.origin.y.0).abs() < 0.001
                        && (quad.bounds.size.height.0 - other.bounds.size.height.0).abs() < 0.001
                }));
            }
        }
    }
}

#[test]
fn externally_positioned_glyphs_keep_ink_outside_the_line_box() {
    let text = text_system();
    let layout = TextLayoutCache::new(text.clone());
    // A line height smaller than the glyph catches accidental paragraph clipping
    // that would cut the tops/bottoms off large math operators and delimiters.
    let glyph = layout
        .prepare("f".into(), &font("IBM Plex Sans"), 40.0, 1.0, None, None)
        .unwrap();
    let atlas = CpuAtlas::default();
    let parent = Bounds::new(point(px(0.0), px(0.0)), size(px(200.0), px(150.0)));
    let mut scenes = Vec::new();
    for baseline in [70.0, 90.0] {
        let mut scene = Scene::default();
        {
            let mut painter =
                Painter::new(&mut scene, &atlas, text.clone(), parent.size, 1.0).unwrap();
            glyph
                .paint_glyphs(&mut painter, point(30.0, baseline), Some(black()))
                .unwrap();
        }
        scene.finish();
        assert_eq!(scene.monochrome_sprites.len(), 1);
        let sprite = &scene.monochrome_sprites[0];
        assert!(sprite.bounds.size.height.0 > 10.0);
        assert_eq!(sprite.content_mask.bounds, parent.scale(1.0));
        scenes.push(scene);
    }
    let a = &scenes[0].monochrome_sprites[0];
    let b = &scenes[1].monochrome_sprites[0];
    assert_eq!(b.bounds.origin.y.0 - a.bounds.origin.y.0, 20.0);
    assert_eq!(
        a.tile, b.tile,
        "baseline changes reuse the same glyph atlas entry"
    );
}
