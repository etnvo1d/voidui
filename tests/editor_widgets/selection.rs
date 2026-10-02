//! Verify actual selection paint commands, including transparent and opaque views.
use super::*;
use render::{Bounds, Hsla, Painter, Scene, fill, point, px, size};
use std::ops::Range;

// Keep real glyph rasterization in the CPU scene without opening a GPU or window.
use render::{AtlasKey, AtlasTextureId, AtlasTile, DevicePixels, PlatformAtlas, Result, TileId};
use std::{collections::HashMap, sync::Mutex};
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

#[derive(Clone, PartialEq)]
struct Object {
    metrics: ViewMetrics,
    background: Option<Hsla>,
    painted: Rc<RefCell<Option<Rect<f32>>>>,
}
impl ViewDescription for Object {
    type View = Self;
    fn create(&self) -> Self {
        self.clone()
    }
}
impl EmbeddedView for Object {
    fn measure(&mut self, _: f32, _: &TextLayoutCache) -> render::Result<ViewMetrics> {
        Ok(self.metrics)
    }
    fn paint(&mut self, painter: &mut Painter<'_>, r: Rect<f32>) -> render::Result<()> {
        *self.painted.borrow_mut() = Some(r);
        if let Some(color) = self.background {
            painter.paint_quad(fill(
                Bounds::new(
                    point(px(r.origin.x), px(r.origin.y)),
                    size(px(r.size.width), px(r.size.height)),
                ),
                color,
            ));
        }
        Ok(())
    }
}

fn paint(
    h: &Harness,
    ranges: &[Range<usize>],
    align: TextAlign,
    clip: Rect<f32>,
    bg: Hsla,
) -> Scene {
    let mut scene = Scene::default();
    let atlas = CpuAtlas::default();
    let mut painter = Painter::new(
        &mut scene,
        &atlas,
        h.system.clone(),
        size(px(400.0), px(400.0)),
        1.0,
    )
    .unwrap();
    h.layout
        .paint(
            &mut painter,
            Point::new(11.0, 13.0),
            clip,
            240.0,
            align,
            render::black(),
            ranges,
            render::white(),
            bg,
        )
        .unwrap();
    drop(painter);
    scene.finish();
    scene
}

fn effective_bounds(quad: &render::Quad) -> Bounds<f32> {
    quad.bounds
        .intersect(&quad.content_mask.bounds)
        .map(|n| n.0)
}

#[test]
fn embedded_selection_is_painted_once() {
    // Cover short/tall formulas, wrapping, display blocks, table cells, alignment,
    // partial viewports, multiple selections, and objects with their own background.
    for source in [
        "before $x$ middle $y$ after",
        "a longer prefix that wraps $x$ and $y$ after",
        "$x$\n$y$\n",
    ] {
        for height in [12.0, 48.0] {
            for align in [TextAlign::Left, TextAlign::Center, TextAlign::Right] {
                for opaque in [false, true] {
                    for in_cell in [false, true] {
                        let mut h = Harness::new();
                        let state = EditorState::new(source);
                        let mut projection = Projection::new();
                        let mut objects = Vec::new();
                        for token in ["$x$", "$y$"] {
                            let start = source.find(token).unwrap();
                            let range = start..start + token.len();
                            let object = Object {
                                metrics: ViewMetrics::new(34.0, height).baseline(height * 0.75),
                                background: opaque.then_some(render::white()),
                                painted: Rc::default(),
                            };
                            projection = projection
                                .replace(Replacement::widget(range.clone(), object.clone()));
                            objects.push((range, object));
                        }
                        if in_cell {
                            h.layout.configure_views(
                                EditorViews::new().block(
                                    ViewId(77),
                                    GridBlock {
                                        rows: vec![vec![0..source.trim_end_matches('\n').len()]],
                                        column_weights: vec![1.0],
                                        padding: 7.0,
                                        gap: 0.0,
                                        rule: None,
                                    },
                                ),
                                None,
                            );
                            projection = projection
                                .block(ViewId(77), 0..source.trim_end_matches('\n').len());
                        }
                        h.prepare(&state, projection, None).unwrap();
                        let full = vec![0..source.len()];
                        let individual = objects.iter().map(|(r, _)| r.clone()).collect::<Vec<_>>();
                        let text_only = vec![0..objects[0].0.start];
                        for ranges in [&full, &individual, &text_only] {
                            for clip in [
                                Rect::from_xywh(0.0, 0.0, 400.0, 400.0),
                                Rect::from_xywh(25.0, 20.0, 120.0, 65.0),
                            ] {
                                let bg = render::hsla(0.7, 0.6, 0.5, 0.2);
                                let scene = paint(&h, ranges, align, clip, bg);
                                let highlights: Vec<_> = scene
                                    .quads
                                    .iter()
                                    .filter(|q| q.background == bg.into())
                                    .collect();
                                // No pair of actual highlight commands may blend over the same pixel.
                                for (i, a) in highlights.iter().enumerate() {
                                    for b in &highlights[i + 1..] {
                                        let overlap =
                                            effective_bounds(a).intersect(&effective_bounds(b));
                                        // Independent local-to-window translations can differ by an f32 ulp.
                                        assert!(
                                            overlap.size.width < 0.001
                                                || overlap.size.height < 0.001,
                                            "duplicate selection: source={source:?}, height={height}, align={align:?}, opaque={opaque}, cell={in_cell}, ranges={ranges:?}, a={:?}, b={:?}",
                                            effective_bounds(a),
                                            effective_bounds(b)
                                        );
                                    }
                                }
                                // The cutout must not remove text selection or the line-height
                                // space around short objects. Compare against public range geometry.
                                for range in ranges {
                                    for r in
                                        h.layout.selection_rectangles(range.clone(), 240.0, align)
                                    {
                                        // Newline selection geometry may extend one pixel past
                                        // the paragraph's own paint clip (inside cell padding).
                                        let inset = if in_cell { 7.0 } else { 0.0 };
                                        let text_clip = Bounds::new(
                                            point(11.0 + inset, 0.0),
                                            size(240.0 - 2.0 * inset, 400.0),
                                        );
                                        let expected = Bounds::new(
                                            point(r.origin.x + 11.0, r.origin.y + 13.0),
                                            size(r.size.width, r.size.height),
                                        )
                                        .intersect(&Bounds::new(
                                            point(clip.origin.x, clip.origin.y),
                                            size(clip.size.width, clip.size.height),
                                        ));
                                        let expected = expected.intersect(&text_clip);
                                        let covered: f32 = highlights
                                            .iter()
                                            .map(|q| {
                                                let r = effective_bounds(q).intersect(&expected);
                                                r.size.width * r.size.height
                                            })
                                            .sum();
                                        assert!(
                                            (covered - expected.size.width * expected.size.height)
                                                .abs()
                                                < 0.02,
                                            "selection coverage lost: {expected:?}, covered={covered}"
                                        );
                                    }
                                }
                                for (range, object) in &objects {
                                    let r = object.painted.borrow().unwrap();
                                    let bounds = Bounds::new(
                                        point(r.origin.x, r.origin.y),
                                        size(r.size.width, r.size.height),
                                    )
                                    .intersect(&Bounds::new(
                                        point(clip.origin.x, clip.origin.y),
                                        size(clip.size.width, clip.size.height),
                                    ));
                                    let selected = ranges
                                        .iter()
                                        .any(|s| s.start < range.end && s.end > range.start);
                                    let covered: f32 = highlights
                                        .iter()
                                        .map(|q| {
                                            let r = effective_bounds(q).intersect(&bounds);
                                            r.size.width * r.size.height
                                        })
                                        .sum();
                                    let expected = if selected {
                                        bounds.size.width * bounds.size.height
                                    } else {
                                        0.0
                                    };
                                    assert!(
                                        (covered - expected).abs() < 0.01,
                                        "object coverage {covered} != {expected}"
                                    );
                                    if opaque && selected && !bounds.is_empty() {
                                        let background = scene
                                            .quads
                                            .iter()
                                            .find(|q| {
                                                q.background == render::white().into()
                                                    && effective_bounds(q).intersect(&bounds)
                                                        == bounds
                                            })
                                            .unwrap();
                                        assert!(
                                            highlights
                                                .iter()
                                                .filter(|q| {
                                                    let overlap =
                                                        effective_bounds(q).intersect(&bounds);
                                                    overlap.size.width > 0.001
                                                        && overlap.size.height > 0.001
                                                })
                                                .all(|q| q.order > background.order),
                                            "selection must remain visible above an opaque object"
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
