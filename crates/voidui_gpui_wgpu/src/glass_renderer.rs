//! Scratch textures are owned by the device and reused across scene revisions.
//! Only padded glass regions are filtered. Ordinary scenes allocate no scratch.
use wgpu::util::DeviceExt;

const MATERIAL_FLOATS: usize =
    44 + std::mem::size_of::<crate::glass_background::GlassBackgroundGpu>() / 4;
pub(crate) const GLASS_UNIFORM_BYTES: u64 = (MATERIAL_FLOATS * 4) as u64;

pub(crate) struct GlassRenderer {
    pub size: (u32, u32),
    pub bytes: u64,
    pub frame: wgpu::TextureView,
    low_size: (u32, u32),
    pub divisor: Option<u32>,
    pyramid_texture: Option<wgpu::Texture>,
    mip_views: Vec<wgpu::TextureView>,
    sampler: wgpu::Sampler,
    filter_layout: wgpu::BindGroupLayout,
    downsample: wgpu::RenderPipeline,
    pyramid: wgpu::RenderPipeline,
    frame_texture: wgpu::Texture,
    sharp_texture: wgpu::Texture,
    sharp: wgpu::TextureView,
    blit: crate::frame_cache::FrameCache,
    captures: Vec<Capture>,
}
impl GlassRenderer {
    /// Reserve one pyramid at the finest requested resolution. Each capture uses
    /// a sliced view starting at its own base mip, so a regular lens does not
    /// change appearance merely because a clear lens is present in the frame.
    pub fn resolution_divisor(scene: &crate::Scene) -> Option<u32> {
        scene
            .glasses
            .iter()
            .filter(|g| !g.material.samples_sharp_backdrop())
            .map(|g| g.material.backdrop_resolution.divisor())
            .min()
    }
    fn backdrop_size(size: (u32, u32), divisor: u32) -> (u32, u32) {
        // Match GPU mip dimensions (floor, not ceil). This keeps a material's
        // texel grid identical in single- and mixed-resolution scenes, including
        // odd window sizes and one-pixel surfaces.
        ((size.0 / divisor).max(1), (size.1 / divisor).max(1))
    }
    pub fn required_bytes(size: (u32, u32), divisor: Option<u32>) -> u64 {
        let mut bytes = 8 * u64::from(size.0) * u64::from(size.1);
        let Some(divisor) = divisor else {
            return bytes;
        };
        let mut dimensions = Self::backdrop_size(size, divisor);
        loop {
            bytes += 8 * u64::from(dimensions.0) * u64::from(dimensions.1);
            if dimensions == (1, 1) {
                break;
            }
            dimensions = ((dimensions.0 / 2).max(1), (dimensions.1 / 2).max(1));
        }
        bytes
    }
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        size: (u32, u32),
        divisor: Option<u32>,
    ) -> Self {
        let low_size = divisor.map_or(size, |d| Self::backdrop_size(size, d));
        let texture = |label, dimensions: (u32, u32)| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: dimensions.0,
                    height: dimensions.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let frame_texture = texture("glass_scene", size);
        let frame = frame_texture.create_view(&Default::default());
        let sharp_texture = texture("glass_sharp", size);
        let sharp = sharp_texture.create_view(&Default::default());
        let mip_count = if divisor.is_some() {
            32 - low_size.0.max(low_size.1).leading_zeros()
        } else {
            0
        };
        let pyramid_texture = divisor.map(|_| device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glass_source_pyramid"),
            size: wgpu::Extent3d {
                width: low_size.0,
                height: low_size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: mip_count,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING
                // Permit direct base-mip readback only in GPU regression tests.
                | if cfg!(test) { wgpu::TextureUsages::COPY_SRC } else { wgpu::TextureUsages::empty() },
            view_formats: &[],
        }));
        let mip_views = (0..mip_count)
            .map(|level| {
                pyramid_texture
                    .as_ref()
                    .unwrap()
                    .create_view(&wgpu::TextureViewDescriptor {
                        base_mip_level: level,
                        mip_level_count: Some(1),
                        ..Default::default()
                    })
            })
            .collect();
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("glass_linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let filter_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glass_filter_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("glass_filter"),
            source: wgpu::ShaderSource::Wgsl(
                concat!(
                    include_str!("glass_math.wgsl"),
                    include_str!("glass_filter.wgsl")
                )
                .into(),
            ),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("glass_filter_pipeline_layout"),
            bind_group_layouts: &[Some(&filter_layout)],
            immediate_size: 0,
        });
        let pipeline = |entry| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vertex_main"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba16Float,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        Self {
            size,
            bytes: Self::required_bytes(size, divisor),
            low_size,
            frame,
            divisor,
            pyramid_texture,
            mip_views,
            sampler,
            downsample: pipeline("downsample"),
            pyramid: pipeline("pyramid"),
            filter_layout,
            frame_texture,
            sharp_texture,
            sharp,
            blit: crate::frame_cache::FrameCache::new(device, format, size),
            captures: Vec::new(),
        }
    }
    /// Reuse uniform buffers and bind groups across redraws. Each capture has its
    /// own uniform storage: queue writes must not change earlier encoded passes.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        scene: &crate::Scene,
    ) {
        let mut slot = 0;
        for batch in scene.batches() {
            let crate::PrimitiveBatch::Glass(range) = batch else {
                continue;
            };
            let surface = &scene.glasses[range.start];
            let transform = scene.spatial.transform(surface.quad.spatial_id);
            let count = range.len();
            let m = surface.material;
            let sharp = m.samples_sharp_backdrop();
            let source_level = if sharp {
                0
            } else {
                ((m.backdrop_resolution.divisor() / self.divisor.expect("blur pyramid required"))
                    .ilog2() as usize)
                    .min(self.mip_views.len() - 1)
            };
            let low_size = if sharp {
                self.size
            } else {
                self.level_size(source_level)
            };
            let padding = m.background.map_or(0., |p| p.paint_padding());
            let local_bounds = scene.glasses[range.clone()]
                .iter()
                .map(|g| g.quad.bounds)
                .reduce(|a, b| a.union(&b))
                .unwrap()
                .dilate(crate::ScaledPixels(surface.smoothing * 0.25 + padding));
            let bounds = scene.glasses[range]
                .iter()
                .map(|g| transform.map_bounds(g.quad.bounds.intersect(&g.quad.content_mask.bounds)))
                .reduce(|a, b| a.union(&b))
                .unwrap();
            let [a, b, c, d, _, _] = transform.0;
            // Conservative screen-space kernel support for affine transforms.
            let scale = (a.abs() + c.abs()).max(b.abs() + d.abs());
            let blur = m.background.map_or(m.blur, |p| p.max_blur(m.blur)) * scale;
            let displacement = m
                .background
                .map_or(m.refraction, |p| p.max_displacement(m.refraction));
            let margin =
                (surface.smoothing * 0.25 + padding + displacement) * scale + 8. * blur + 16.;
            let sx = low_size.0 as f32 / self.size.0 as f32;
            let sy = low_size.1 as f32 / self.size.1 as f32;
            let x = ((bounds.origin.x.0 - margin) * sx)
                .floor()
                .clamp(0., low_size.0 as f32);
            let y = ((bounds.origin.y.0 - margin) * sy)
                .floor()
                .clamp(0., low_size.1 as f32);
            let right = ((bounds.origin.x.0 + bounds.size.width.0 + margin) * sx)
                .ceil()
                .clamp(0., low_size.0 as f32);
            let bottom = ((bounds.origin.y.0 + bounds.size.height.0 + margin) * sy)
                .ceil()
                .clamp(0., low_size.1 as f32);
            let filter_values = [
                low_size.0 as f32,
                low_size.1 as f32,
                blur * sx.max(sy),
                if m.background.is_some() { 1. } else { 0. },
                x,
                y,
                right,
                bottom,
            ];
            let key = m.key_light;
            let fill = m.fill_light;
            let matrix = m.tone.map_or(
                [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
                |t| t.matrix().map(|row| row.map(crate::glass::round_half)),
            );
            let mut material_values = [0.; MATERIAL_FLOATS];
            material_values[..44].copy_from_slice(&[
                m.refraction,
                m.thickness,
                m.saturation,
                m.highlight,
                m.tint[0],
                m.tint[1],
                m.tint[2],
                m.tint[3],
                m.opacity,
                blur * sx.max(sy),
                if m.continuous { 1. } else { 0. },
                m.ovalization,
                local_bounds.origin.x.0,
                local_bounds.origin.y.0,
                local_bounds.size.width.0,
                local_bounds.size.height.0,
                count as f32,
                surface.smoothing,
                if surface.merge { 1. } else { 0. },
                if m.background.is_some() { 1. } else { 0. },
                key.height,
                key.spread.cos(),
                1. / key.amount - 2.,
                key.angle.sin(),
                -key.angle.cos(),
                fill.height,
                fill.spread.cos(),
                1. / fill.amount - 2.,
                fill.angle.sin(),
                -fill.angle.cos(),
                m.curvature,
                0.,
                matrix[0][0],
                matrix[0][1],
                matrix[0][2],
                matrix[0][3],
                matrix[1][0],
                matrix[1][1],
                matrix[1][2],
                matrix[1][3],
                matrix[2][0],
                matrix[2][1],
                matrix[2][2],
                matrix[2][3],
            ]);
            let background = crate::glass_background::GlassBackgroundGpu::new(
                m,
                transform,
                self.size,
                sx.max(sy),
            );
            material_values[44..].copy_from_slice(bytemuck::cast_slice(&background.0));
            if self
                .captures
                .get(slot)
                .is_none_or(|c| c.source_level != source_level || c.sharp != sharp)
            {
                let capture = self.create_capture(
                    device,
                    layout,
                    filter_values,
                    material_values,
                    source_level,
                    sharp,
                );
                if slot == self.captures.len() {
                    self.captures.push(capture);
                } else {
                    self.captures[slot] = capture;
                }
            } else {
                let capture = &mut self.captures[slot];
                if capture.filter_values != filter_values {
                    for (level, buffer) in capture.filter_buffers.iter().enumerate() {
                        let values = Self::filter_parameters(low_size, filter_values, level);
                        queue.write_buffer(buffer, 0, bytemuck::cast_slice(&values));
                    }
                    capture.filter_values = filter_values;
                }
                if capture.material_values != material_values {
                    queue.write_buffer(
                        &capture.material_buffer,
                        0,
                        bytemuck::cast_slice(&material_values),
                    );
                    capture.material_values = material_values;
                }
            }
            slot += 1;
        }
        self.captures.truncate(slot);
    }

    fn create_capture(
        &self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        filter_values: [f32; 8],
        material_values: [f32; MATERIAL_FLOATS],
        source_level: usize,
        sharp: bool,
    ) -> Capture {
        let low_size = if sharp {
            self.size
        } else {
            self.level_size(source_level)
        };
        let buffer = |label, values: &[f32]| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::cast_slice(values),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            })
        };
        let filter_levels = if sharp {
            0
        } else {
            self.mip_views.len() - source_level
        };
        let filter_buffers: Vec<_> = (0..filter_levels)
            .map(|level| {
                buffer(
                    "glass_pyramid_params",
                    &Self::filter_parameters(low_size, filter_values, level),
                )
            })
            .collect();
        let material_buffer = buffer("glass_material", &material_values);
        let filters = filter_buffers
            .iter()
            .enumerate()
            .map(|(level, params)| {
                let source = if level == 0 {
                    &self.frame
                } else {
                    &self.mip_views[source_level + level - 1]
                };
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("glass_pyramid_source"),
                    layout: &self.filter_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(source),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: params.as_entire_binding(),
                        },
                    ],
                })
            })
            .collect();
        // A zero-blur lens reads the full-resolution capture, without an RGBA16F
        // conversion, downsample pass or mip allocation. Keep mips for other lenses.
        let backdrop_view = if sharp {
            self.sharp.clone()
        } else {
            self.pyramid_texture
                .as_ref()
                .expect("blur pyramid required")
                .create_view(&wgpu::TextureViewDescriptor {
                    base_mip_level: source_level as u32,
                    mip_level_count: Some((self.mip_views.len() - source_level) as u32),
                    ..Default::default()
                })
        };
        let backdrop = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("glass_backdrop"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&backdrop_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: material_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&self.sharp),
                },
            ],
        });
        Capture {
            sharp,
            source_level,
            low_size,
            filter_values,
            material_values,
            filter_buffers,
            material_buffer,
            filters,
            backdrop,
        }
    }

    /// Copy/filter just the padded footprint. The scissor and sample clamp agree,
    /// so no later draw can sample uninitialized or stale scratch pixels.
    pub fn capture(&self, slot: usize, encoder: &mut wgpu::CommandEncoder) -> u64 {
        let capture = &self.captures[slot];
        let [_, _, sigma, _, x, y, right, bottom] = capture.filter_values;
        if right <= x || bottom <= y {
            return 0;
        }
        let sx = capture.low_size.0 as f32 / self.size.0 as f32;
        let sy = capture.low_size.1 as f32 / self.size.1 as f32;
        let left = (x / sx).floor() as u32;
        let top = (y / sy).floor() as u32;
        let width = ((right / sx).ceil() as u32).min(self.size.0) - left;
        let height = ((bottom / sy).ceil() as u32).min(self.size.1) - top;
        let origin = wgpu::Origin3d {
            x: left,
            y: top,
            z: 0,
        };
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.frame_texture,
                mip_level: 0,
                origin,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: &self.sharp_texture,
                mip_level: 0,
                origin,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        if capture.sharp {
            return 0;
        }
        let lod = (if sigma < 2. { 1. + sigma * 0.5 } else { sigma })
            .log2()
            .max(0.);
        let levels = (lod.ceil() as usize + 1).min(capture.filters.len());
        let mut pixels = 0;
        for level in 0..levels {
            let (width, height) = self.level_size(capture.source_level + level);
            let rx = (x / capture.low_size.0 as f32 * width as f32).floor() as u32;
            let ry = (y / capture.low_size.1 as f32 * height as f32).floor() as u32;
            let rr = (right / capture.low_size.0 as f32 * width as f32)
                .ceil()
                .min(width as f32) as u32;
            let rb = (bottom / capture.low_size.1 as f32 * height as f32)
                .ceil()
                .min(height as f32) as u32;
            if rr <= rx || rb <= ry {
                continue;
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("glass_pyramid_region"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.mip_views[capture.source_level + level],
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_scissor_rect(rx, ry, rr - rx, rb - ry);
            pass.set_pipeline(if level == 0 {
                &self.downsample
            } else {
                &self.pyramid
            });
            pass.set_bind_group(0, &capture.filters[level], &[]);
            pass.draw(0..3, 0..1);
            pixels += u64::from(rr - rx) * u64::from(rb - ry);
        }
        pixels
    }
    fn level_size(&self, level: usize) -> (u32, u32) {
        (
            (self.low_size.0 >> level).max(1),
            (self.low_size.1 >> level).max(1),
        )
    }
    fn filter_parameters(low_size: (u32, u32), values: [f32; 8], level: usize) -> [f32; 8] {
        let width = (low_size.0 >> level).max(1) as f32;
        let height = (low_size.1 >> level).max(1) as f32;
        if level == 0 {
            // The base pass reads the full frame; region bounds are only used by later pyramid passes.
            return [width, height, 0., 0., 0., 0., 0., 0.];
        }
        let source_width = (low_size.0 >> (level - 1)).max(1) as f32;
        let source_height = (low_size.1 >> (level - 1)).max(1) as f32;
        [
            width,
            height,
            0.,
            0.,
            (values[4] / low_size.0 as f32 * source_width).floor(),
            (values[5] / low_size.1 as f32 * source_height).floor(),
            (values[6] / low_size.0 as f32 * source_width)
                .ceil()
                .min(source_width),
            (values[7] / low_size.1 as f32 * source_height)
                .ceil()
                .min(source_height),
        ]
    }
    pub fn binding(&self, slot: usize) -> &wgpu::BindGroup {
        &self.captures[slot].backdrop
    }
    pub fn present(&self, device: &wgpu::Device, queue: &wgpu::Queue, target: &wgpu::TextureView) {
        self.blit.present(device, queue, &self.frame, target);
    }
}

struct Capture {
    sharp: bool,
    source_level: usize,
    low_size: (u32, u32),
    filter_values: [f32; 8],
    material_values: [f32; MATERIAL_FLOATS],
    filter_buffers: Vec<wgpu::Buffer>,
    material_buffer: wgpu::Buffer,
    filters: Vec<wgpu::BindGroup>,
    backdrop: wgpu::BindGroup,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Bounds, ContentMask, Corners, Glass, GlassBackdropResolution as Resolution, GlassMaterial,
        Quad, ScaledPixels, Scene, point, size,
    };

    #[test]
    fn backdrop_dimensions_and_budget_follow_material_resolution() {
        assert_eq!(GlassMaterial::clear().backdrop_resolution, Resolution::Half);
        assert_eq!(
            GlassMaterial::regular().backdrop_resolution,
            Resolution::Quarter
        );
        for dimensions in [(1, 1), (3, 5), (641, 359), (1920, 1080)] {
            let half = GlassRenderer::backdrop_size(dimensions, 2);
            let quarter = GlassRenderer::backdrop_size(dimensions, 4);
            assert_eq!(quarter, ((half.0 / 2).max(1), (half.1 / 2).max(1)));
            let bytes = GlassRenderer::required_bytes(dimensions, Some(2));
            assert!(bytes >= GlassRenderer::required_bytes(dimensions, Some(4)));
            assert!(bytes <= GlassRenderer::required_bytes(dimensions, Some(1)));
        }
        assert_eq!(
            GlassRenderer::required_bytes((640, 360), Some(4)),
            1_996_504
        );
        assert_eq!(
            GlassRenderer::required_bytes((640, 360), Some(2)),
            2_457_304
        );
    }

    #[test]
    fn sharp_materials_do_not_inflate_the_pyramid_plan() {
        let mut scene = scene((640, 360), Resolution::Full);
        scene.glasses[0].material = GlassMaterial::unblurred();
        assert_eq!(GlassRenderer::resolution_divisor(&scene), None);
        assert_eq!(
            GlassRenderer::required_bytes((640, 360), None),
            8 * 640 * 360
        );
        let mut regular = scene.glasses[0].clone();
        regular.material = GlassMaterial::regular();
        scene.glasses.push(regular);
        assert_eq!(GlassRenderer::resolution_divisor(&scene), Some(4));
        // An active shadow/bleed still requires mips even if face blur is zero.
        scene.glasses[0].material.background = Some(crate::GlassBackground::regular());
        assert_eq!(GlassRenderer::resolution_divisor(&scene), Some(1));
        scene.glasses[0].material = GlassMaterial {
            blur: 1.,
            ..GlassMaterial::unblurred()
        };
        assert_eq!(GlassRenderer::resolution_divisor(&scene), Some(1));
    }

    fn scene(dimensions: (u32, u32), resolution: Resolution) -> Scene {
        let bounds = Bounds::new(
            point(ScaledPixels(0.), ScaledPixels(0.)),
            size(
                ScaledPixels(dimensions.0 as f32),
                ScaledPixels(dimensions.1 as f32),
            ),
        );
        let mut scene = Scene::default();
        scene.insert_primitive(Glass {
            quad: Quad {
                bounds,
                content_mask: ContentMask { bounds },
                corner_radii: Corners::default(),
                ..Default::default()
            },
            material: GlassMaterial {
                backdrop_resolution: resolution,
                ..GlassMaterial::clear()
            },
            smoothing: 0.,
            merge: false,
        });
        scene.finish();
        scene
    }

    fn captured_row(
        renderer: &mut GlassRenderer,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        resolution: Resolution,
    ) -> Vec<u16> {
        renderer.prepare(device, queue, layout, &scene(renderer.size, resolution));
        let capture = &renderer.captures[0];
        let (width, height) = capture.low_size;
        let stride = (width * 8).div_ceil(256) * 256;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("glass_detail_readback"),
            size: u64::from(stride * height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        renderer.capture(0, &mut encoder);
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: renderer.pyramid_texture.as_ref().unwrap(),
                mip_level: capture.source_level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let submission = queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                tx.send(result).unwrap();
            });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(std::time::Duration::from_secs(30)),
            })
            .unwrap();
        rx.recv().unwrap().unwrap();
        let mapped = readback.slice(..).get_mapped_range();
        let row = mapped[..width as usize * 8]
            .chunks_exact(8)
            .map(|p| u16::from_le_bytes([p[0], p[1]]))
            .collect();
        drop(mapped);
        readback.unmap();
        row
    }

    #[test]
    fn gpu_backdrop_preserves_stems_and_rebinds_mixed_resolution_captures() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = crate::block_on(instance.request_adapter(&Default::default()))
            .expect("GPU adapter required");
        let (device, queue) = crate::block_on(adapter.request_device(&Default::default())).unwrap();
        let texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glass_detail_layout"),
            entries: &[
                texture(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: std::num::NonZeroU64::new(GLASS_UNIFORM_BYTES),
                    },
                    count: None,
                },
                texture(3),
            ],
        });
        for dimensions in [(64, 8), (65, 9), (1, 1)] {
            let pixels: Vec<u8> = (0..dimensions.1)
                .flat_map(|_| {
                    (0..dimensions.0).flat_map(|x| {
                        let value = if x % 4 < 2 { 0 } else { 255 };
                        [value, value, value, 255]
                    })
                })
                .collect();
            let create = |divisor| {
                let renderer = GlassRenderer::new(
                    &device,
                    wgpu::TextureFormat::Rgba8Unorm,
                    dimensions,
                    Some(divisor),
                );
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &renderer.frame_texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    &pixels,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(dimensions.0 * 4),
                        rows_per_image: Some(dimensions.1),
                    },
                    wgpu::Extent3d {
                        width: dimensions.0,
                        height: dimensions.1,
                        depth_or_array_layers: 1,
                    },
                );
                renderer
            };
            let mut shared = create(1);
            // Reuse the same capture slot while changing its texture view. This
            // also checks identical texel grids when another lens requests Full.
            for resolution in [
                Resolution::Half,
                Resolution::Quarter,
                Resolution::Full,
                Resolution::Half,
            ] {
                let actual = captured_row(&mut shared, &device, &queue, &layout, resolution);
                let mut standalone = create(resolution.divisor());
                let expected = captured_row(&mut standalone, &device, &queue, &layout, resolution);
                assert_eq!(actual, expected, "{dimensions:?} {resolution:?}");
                if dimensions == (64, 8) {
                    for (x, value) in actual.into_iter().enumerate() {
                        // IEEE half encodings: 0, 0.5, 1. Quarter sampling loses
                        // every stem; half sampling retains the black/white grid.
                        let expected = match resolution {
                            Resolution::Full => {
                                if x % 4 < 2 {
                                    0
                                } else {
                                    0x3c00
                                }
                            }
                            Resolution::Half => {
                                if x % 2 == 0 {
                                    0
                                } else {
                                    0x3c00
                                }
                            }
                            Resolution::Quarter => 0x3800,
                        };
                        assert_eq!(value, expected, "{resolution:?} x={x}");
                    }
                }
            }
        }
    }
}
