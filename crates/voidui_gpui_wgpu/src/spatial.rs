//! Affine coordinates shared by scene writing, clipping, and input adapters.
use crate::{Bounds, ScaledPixels, point, size};
use std::sync::Arc;

/// CSS `matrix(a, b, c, d, e, f)`, operating on column vectors.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine(pub [f32; 6]);
impl Default for Affine {
    fn default() -> Self {
        Self::IDENTITY
    }
}
impl Affine {
    pub const IDENTITY: Self = Self([1., 0., 0., 1., 0., 0.]);
    pub fn translate(x: f32, y: f32) -> Self {
        Self([1., 0., 0., 1., x, y])
    }
    pub fn scale(x: f32, y: f32) -> Self {
        Self([x, 0., 0., y, 0., 0.])
    }
    pub fn rotate(radians: f32) -> Self {
        let (s, c) = radians.sin_cos();
        Self([c, s, -s, c, 0., 0.])
    }
    /// Compose `self * rhs`: rhs acts on the point first.
    pub fn compose(self, rhs: Self) -> Self {
        let [a, b, c, d, e, f] = self.0;
        let [g, h, i, j, k, l] = rhs.0;
        Self([
            a * g + c * h,
            b * g + d * h,
            a * i + c * j,
            b * i + d * j,
            a * k + c * l + e,
            b * k + d * l + f,
        ])
    }
    pub fn map(self, [x, y]: [f32; 2]) -> [f32; 2] {
        let [a, b, c, d, e, f] = self.0;
        [a * x + c * y + e, b * x + d * y + f]
    }
    /// Singular or overflowing transforms have no visible or interactive area.
    pub fn inverse(self) -> Option<Self> {
        let [a, b, c, d, e, f] = self.0.map(f64::from);
        let det = a * d - b * c;
        if det == 0. || !det.is_finite() {
            return None;
        }
        let result = Self(
            [
                d / det,
                -b / det,
                -c / det,
                a / det,
                (c * f - d * e) / det,
                (b * e - a * f) / det,
            ]
            .map(|v| v as f32),
        );
        result.0.iter().all(|v| v.is_finite()).then_some(result)
    }
    pub fn device(self, scale: f32) -> Self {
        let mut result = self;
        result.0[4] *= scale;
        result.0[5] *= scale;
        result
    }
    pub fn map_bounds(self, b: Bounds<ScaledPixels>) -> Bounds<ScaledPixels> {
        let x = b.origin.x.0;
        let y = b.origin.y.0;
        let r = x + b.size.width.0;
        let bottom = y + b.size.height.0;
        let corners = [[x, y], [r, y], [r, bottom], [x, bottom]].map(|p| self.map(p));
        let low = corners
            .iter()
            .fold([f32::INFINITY; 2], |a, p| [a[0].min(p[0]), a[1].min(p[1])]);
        let high = corners.iter().fold([f32::NEG_INFINITY; 2], |a, p| {
            [a[0].max(p[0]), a[1].max(p[1])]
        });
        Bounds::new(
            point(ScaledPixels(low[0]), ScaledPixels(low[1])),
            size(
                ScaledPixels(high[0] - low[0]),
                ScaledPixels(high[1] - low[1]),
            ),
        )
    }
}

/// One ancestor's clip in its own coordinates. Disabled axes remain unbounded.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpatialClip {
    pub inverse: Affine,
    pub bounds: [f32; 4],
    pub axes: [bool; 2],
    /// Elliptical inner radii in top-left, top-right, bottom-right, bottom-left order.
    /// A zero component makes that corner square. Single-axis clips stay square.
    pub radii: [[f32; 2]; 4],
}
impl SpatialClip {
    pub fn contains(self, point: [f32; 2]) -> bool {
        let [x, y] = self.inverse.map(point);
        let [l, t, w, h] = self.bounds;
        if (self.axes[0] && (x < l || x >= l + w)) || (self.axes[1] && (y < t || y >= t + h)) {
            return false;
        }
        if self.axes != [true, true] {
            return true;
        }
        // Test each corner independently: unequal borders can move an ellipse's
        // center past the middle of the padding box.
        for ([rx, ry], [dx, dy]) in self.radii.into_iter().zip([
            [x - l, y - t],
            [l + w - x, y - t],
            [l + w - x, t + h - y],
            [x - l, t + h - y],
        ]) {
            if rx > 0.0
                && ry > 0.0
                && dx < rx
                && dy < ry
                && ((dx - rx) / rx).powi(2) + ((dy - ry) / ry).powi(2) > 1.0
            {
                return false;
            }
        }
        true
    }
}

/// Persistent clipping ancestry. Siblings share their parents' clip records.
#[derive(Clone, Debug)]
pub struct ClipChain {
    pub clip: SpatialClip,
    pub parent: Option<Arc<ClipChain>>,
}
impl ClipChain {
    pub fn contains(&self, point: [f32; 2]) -> bool {
        let mut current = Some(self);
        while let Some(c) = current {
            if !c.clip.contains(point) {
                return false;
            }
            current = c.parent.as_deref();
        }
        true
    }
}
/// Immutable per-element paint coordinates, also retained by native hit regions.
#[derive(Clone, Debug)]
pub struct PaintSpace {
    pub transform: Affine,
    pub inverse: Option<Affine>,
    pub clips: Option<Arc<ClipChain>>,
}
impl Default for PaintSpace {
    fn default() -> Self {
        Self {
            transform: Affine::IDENTITY,
            inverse: Some(Affine::IDENTITY),
            clips: None,
        }
    }
}
impl PaintSpace {
    pub fn new(transform: Affine, clips: Option<Arc<ClipChain>>) -> Self {
        Self {
            transform,
            inverse: transform.inverse(),
            clips,
        }
    }
}

/// Packed GPU records. Index zero takes the identity fast path. Clip records
/// are uploaded once per scene, regardless of the number of painted descendants.
#[derive(Default)]
pub struct SpatialData {
    pub(crate) words: Vec<[u32; 4]>,
    clips: std::collections::HashMap<(usize, u32), u32>,
    owners: Vec<Arc<ClipChain>>,
    spaces: std::collections::HashMap<[u32; 11], u32>,
    last_space: Option<([u32; 11], u32)>,
}
impl SpatialData {
    pub(crate) fn clear(&mut self) {
        self.words.clear();
        self.clips.clear();
        self.owners.clear();
        self.spaces.clear();
        self.last_space = None;
    }
    fn clip(&mut self, c: SpatialClip, scale: f32, parent: u32) -> u32 {
        // Five vec4 words: two inverse-matrix rows (with parent/flags), bounds,
        // then four radius pairs. Scene replay and both shader transports share
        // this layout; bit 2 lets rectangular clips skip the radius loads.
        let index = self.words.len() as u32;
        let [a, b, cx, d, e, f] = c.inverse.device(scale).0;
        self.words
            .push([a.to_bits(), cx.to_bits(), e.to_bits(), parent]);
        self.words.push([
            b.to_bits(),
            d.to_bits(),
            f.to_bits(),
            u32::from(c.axes[0])
                | (u32::from(c.axes[1]) << 1)
                | (u32::from(
                    c.axes == [true, true] && c.radii.iter().any(|r| r[0] > 0.0 && r[1] > 0.0),
                ) << 2),
        ]);
        self.words.push(c.bounds.map(|v| (v * scale).to_bits()));
        for corners in c.radii.chunks_exact(2) {
            self.words.push([
                (corners[0][0] * scale).to_bits(),
                (corners[0][1] * scale).to_bits(),
                (corners[1][0] * scale).to_bits(),
                (corners[1][1] * scale).to_bits(),
            ]);
        }
        index
    }
    fn chain(&mut self, chain: &Arc<ClipChain>, scale: f32) -> u32 {
        let key = (Arc::as_ptr(chain) as usize, scale.to_bits());
        if let Some(id) = self.clips.get(&key) {
            return *id;
        }
        let parent = chain.parent.as_ref().map_or(0, |p| self.chain(p, scale));
        let id = self.clip(chain.clip, scale, parent);
        self.clips.insert(key, id);
        self.owners.push(chain.clone());
        id
    }
    pub(crate) fn push(&mut self, space: &PaintSpace, scale: f32, viewport: SpatialClip) -> u32 {
        if self.words.is_empty() {
            self.words.push([0; 4]);
        }
        let parent = space.clips.as_ref().map_or(0, |c| self.chain(c, scale));
        let [a, b, c, d, e, f] = space.transform.device(scale).0;
        let bounds = viewport.bounds.map(|v| (v * scale).to_bits());
        let key = [
            a.to_bits(),
            b.to_bits(),
            c.to_bits(),
            d.to_bits(),
            e.to_bits(),
            f.to_bits(),
            parent,
            bounds[0],
            bounds[1],
            bounds[2],
            bounds[3],
        ];
        if let Some((last, id)) = self.last_space
            && last == key
        {
            return id;
        }
        if let Some(id) = self.spaces.get(&key) {
            self.last_space = Some((key, *id));
            return *id;
        }
        let clip = self.clip(viewport, scale, parent);
        let index = self.words.len() as u32;
        self.spaces.insert(key, index);
        self.last_space = Some((key, index));
        self.words
            .push([a.to_bits(), c.to_bits(), e.to_bits(), clip]);
        self.words.push([b.to_bits(), d.to_bits(), f.to_bits(), 0]);
        index
    }
    /// Replay copies only records reachable from the requested paint range.
    /// The new scene owns the words; source scenes may be dropped immediately.
    pub(crate) fn copy_space(
        &mut self,
        source: &Self,
        id: u32,
        copied: &mut std::collections::HashMap<u32, u32>,
    ) -> u32 {
        fn record(
            target: &mut SpatialData,
            source: &SpatialData,
            id: u32,
            count: usize,
            copied: &mut std::collections::HashMap<u32, u32>,
        ) -> u32 {
            if id == 0 {
                return 0;
            }
            if let Some(id) = copied.get(&id) {
                return *id;
            }
            if target.words.is_empty() {
                target.words.push([0; 4]);
            }
            let parent = record(target, source, source.words[id as usize][3], 5, copied);
            let next = target.words.len() as u32;
            target
                .words
                .extend_from_slice(&source.words[id as usize..id as usize + count]);
            target.words[next as usize][3] = parent;
            copied.insert(id, next);
            next
        }
        record(self, source, id, 2, copied)
    }
    pub(crate) fn transform(&self, id: u32) -> Affine {
        if id == 0 {
            return Affine::IDENTITY;
        }
        let a = self.words[id as usize].map(f32::from_bits);
        let b = self.words[id as usize + 1].map(f32::from_bits);
        Affine([a[0], b[0], a[1], b[1], a[2], b[2]])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;
    use std::{borrow::Cow, sync::Arc};
    struct NoGlyphs;
    impl PlatformAtlas for NoGlyphs {
        fn get_or_insert_with<'a>(
            &self,
            _: &AtlasKey,
            _: &mut dyn FnMut() -> Result<Option<(Size<DevicePixels>, Cow<'a, [u8]>)>>,
        ) -> Result<Option<AtlasTile>> {
            panic!("no text in scene fixture")
        }
        fn remove(&self, _: &AtlasKey) {}
    }
    fn text() -> Arc<TextSystem> {
        Arc::new(TextSystem::new(Arc::new(
            ParleyTextSystem::new_without_system_fonts("unused"),
        )))
    }
    #[test]
    fn affine_inverse_roundtrips_reflections_and_skew() {
        let m = Affine::translate(40., 60.)
            .compose(Affine::rotate(0.7))
            .compose(Affine([2., 0.4, -0.6, -3., 0., 0.]));
        let p = [17., -34.];
        let r = m.inverse().unwrap().map(m.map(p));
        for i in 0..2 {
            assert!((p[i] - r[i]).abs() < 0.0001);
        }
        assert!(Affine::scale(0., 1.).inverse().is_none());
    }
    #[test]
    fn elliptical_clips_handle_zero_radii_and_disabled_axes() {
        let mut clip = SpatialClip {
            inverse: Affine::translate(-10., -20.),
            bounds: [0., 0., 100., 100.],
            axes: [true, true],
            radii: [[30., 46.], [0., 10.], [12., 0.], [8., 20.]],
        };
        assert!(!clip.contains([14., 36.]));
        assert!(clip.contains([30., 36.]));
        assert!(clip.contains([109., 21.]));
        assert!(clip.contains([109., 119.]));
        clip.axes = [true, false];
        assert!(clip.contains([11., -100.]));
        assert!(!clip.contains([9., 30.]));
    }

    /// Run the production clipping shader against packed, replayed scene data.
    /// This catches CPU/GPU layout, DPI and ellipse-equation disagreements.
    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn gpu_rounded_coverage_matches_hit_testing_after_replay() -> anyhow::Result<()> {
        use wgpu::util::DeviceExt;
        let (device, queue) = crate::block_on(async {
            let instance =
                wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions::default())
                .await?;
            Ok::<_, anyhow::Error>(
                adapter
                    .request_device(&wgpu::DeviceDescriptor::default())
                    .await?,
            )
        })?;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rounded_clip_coverage_test"),
            source: wgpu::ShaderSource::Wgsl(
                concat!(
                    include_str!("shaders.wgsl"),
                    include_str!("shaders_storage.wgsl"),
                    "\n@group(1) @binding(0) var<storage, read> test_points: array<vec4<f32>>;
                @group(2) @binding(0) var<storage, read_write> test_coverage: array<f32>;
                @compute @workgroup_size(64)
                fn test_clip(@builtin(global_invocation_id) id: vec3<u32>) {
                    if id.x < arrayLength(&test_points) {
                        let p = test_points[id.x];
                        test_coverage[id.x] = spatial_coverage(p.xy, bitcast<u32>(p.w));
                    }
                }"
                )
                .into(),
            ),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &shader,
            entry_point: Some("test_clip"),
            compilation_options: Default::default(),
            cache: None,
        });
        let mut replay = SpatialData::default();
        let mut samples = Vec::new();
        let mut expected = Vec::new();
        for scale in [1., 2.] {
            let matrix = Affine::translate(80., 60.)
                .compose(Affine::rotate(0.3))
                .compose(Affine::scale(1.5, 0.8));
            let clip = SpatialClip {
                inverse: matrix.inverse().unwrap(),
                bounds: [0., 0., 60., 50.],
                axes: [true, true],
                radii: [[20., 24.], [12., 24.], [12., 16.], [20., 16.]],
            };
            let chain = Arc::new(ClipChain {
                clip,
                parent: Some(Arc::new(ClipChain {
                    clip: SpatialClip {
                        bounds: [2., 0., 58., 50.],
                        radii: [[0.; 2]; 4],
                        ..clip
                    },
                    parent: None,
                })),
            });
            let space = PaintSpace::new(matrix, Some(chain.clone()));
            let mut source = SpatialData::default();
            let id = source.push(
                &space,
                scale,
                SpatialClip {
                    inverse: Affine::IDENTITY,
                    bounds: [0., 0., 300., 300.],
                    axes: [true, true],
                    radii: [[0.; 2]; 4],
                },
            );
            let id = replay.copy_space(&source, id, &mut Default::default());
            drop(source);
            for y in -1..51 {
                for x in -1..61 {
                    let p = matrix.map([x as f32 + 0.37, y as f32 + 0.61]);
                    samples.push([p[0] * scale, p[1] * scale, 0., f32::from_bits(id)]);
                    expected.push(chain.contains(p));
                }
            }
        }
        let buffer = |data: &[u8], usage| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: data,
                usage,
            })
        };
        let spatial = buffer(
            bytemuck::cast_slice(&replay.words),
            wgpu::BufferUsages::STORAGE,
        );
        let points = buffer(bytemuck::cast_slice(&samples), wgpu::BufferUsages::STORAGE);
        let bytes = (samples.len() * std::mem::size_of::<f32>()) as u64;
        let output = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let groups: Vec<_> = [(2, &spatial), (0, &points), (0, &output)]
            .into_iter()
            .enumerate()
            .map(|(index, (binding, buffer))| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &pipeline.get_bind_group_layout(index as u32),
                    entries: &[wgpu::BindGroupEntry {
                        binding,
                        resource: buffer.as_entire_binding(),
                    }],
                })
            })
            .collect();
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            for (index, group) in groups.iter().enumerate() {
                pass.set_bind_group(index as u32, group, &[]);
            }
            pass.dispatch_workgroups((samples.len() as u32).div_ceil(64), 1, 1);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, bytes);
        let submission = queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
        device.poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(std::time::Duration::from_secs(30)),
        })?;
        rx.recv_timeout(std::time::Duration::from_secs(30))??;
        let mapped = readback.slice(..).get_mapped_range();
        let coverage: &[f32] = bytemuck::cast_slice(&mapped);
        assert!(
            coverage.iter().any(|a| *a > 0. && *a < 1.),
            "rounded edges must be antialiased"
        );
        for (index, (&alpha, inside)) in coverage.iter().zip(expected).enumerate() {
            assert!(alpha.is_finite() && (0.0..=1.0).contains(&alpha));
            assert_eq!(alpha >= 0.5, inside, "sample {index}: alpha {alpha}");
        }
        Ok(())
    }
    #[test]
    fn shared_space_upload_is_constant_in_descendant_count() {
        let clip = Arc::new(ClipChain {
            clip: SpatialClip {
                inverse: Affine::IDENTITY,
                bounds: [0., 0., 100., 100.],
                axes: [true, true],
                radii: [[0.; 2]; 4],
            },
            parent: None,
        });
        let space = PaintSpace::new(Affine::translate(10., 20.), Some(clip));
        let mut scene = Scene::default();
        let mut painter =
            Painter::new(&mut scene, &NoGlyphs, text(), size(px(300.), px(200.)), 2.).unwrap();
        for _ in 0..1000 {
            painter.with_space(&space, |p| {
                p.paint_quad(fill(
                    Bounds::new(point(px(0.), px(0.)), size(px(20.), px(20.))),
                    black(),
                ))
            });
        }
        // Sentinel + ancestor clip + viewport clip + matrix; no per-glyph matrices.
        assert_eq!(scene.spatial.words.len(), 1 + 5 + 5 + 2);
        assert!(
            scene
                .quads
                .iter()
                .all(|q| q.spatial_id == scene.quads[0].spatial_id)
        );
        scene.clear();
        assert!(scene.spatial.words.is_empty());
        let mut painter =
            Painter::new(&mut scene, &NoGlyphs, text(), size(px(300.), px(200.)), 1.).unwrap();
        painter.paint_quad(fill(
            Bounds::new(point(px(0.), px(0.)), size(px(20.), px(20.))),
            black(),
        ));
        assert!(scene.spatial.words.is_empty());
        assert_eq!(scene.quads[0].spatial_id, 0);
    }
    #[test]
    fn transformed_paths_and_quads_keep_local_geometry_and_replay_owned_records() {
        let mut source = Scene::default();
        let space = PaintSpace::new(
            Affine::translate(120., 30.).compose(Affine::rotate(std::f32::consts::FRAC_PI_2)),
            None,
        );
        let bounds = Bounds::new(point(px(0.), px(0.)), size(px(20.), px(30.)));
        let mut painter =
            Painter::new(&mut source, &NoGlyphs, text(), size(px(300.), px(200.)), 1.).unwrap();
        painter.with_space(&space, |p| {
            p.paint_quad(fill(bounds, black()));
            let mut path = Path::new(point(px(0.), px(0.)));
            path.line_to(point(px(20.), px(0.)));
            path.line_to(point(px(0.), px(30.)));
            p.paint_path(path, black());
        });
        assert_eq!(source.quads[0].bounds, bounds.scale(1.));
        let b = source.paths[0].clipped_bounds();
        assert!((b.origin.x.0 - 90.).abs() < 0.001);
        assert!((b.origin.y.0 - 30.).abs() < 0.001);
        let mut replay = Scene::default();
        replay.replay(0..source.len(), &source);
        drop(source);
        let id = replay.quads[0].spatial_id;
        assert_eq!(replay.spatial.transform(id), space.transform);
        assert_eq!(replay.spatial.words[id as usize][3], 1);
        assert_eq!(replay.paths[0].clipped_bounds(), b);
    }
}
