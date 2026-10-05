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
    blur_a: wgpu::TextureView,
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
    pub fn required_bytes(size: (u32, u32)) -> u64 {
        let mut dimensions = (size.0.div_ceil(4), size.1.div_ceil(4));
        let mut bytes = 8 * u64::from(size.0) * u64::from(size.1);
        loop {
            bytes += 8 * u64::from(dimensions.0) * u64::from(dimensions.1);
            if dimensions == (1, 1) {
                break;
            }
            dimensions = ((dimensions.0 / 2).max(1), (dimensions.1 / 2).max(1));
        }
        bytes
    }
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, size: (u32, u32)) -> Self {
        let low_size = (size.0.div_ceil(4), size.1.div_ceil(4));
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
        let mip_count = 32 - low_size.0.max(low_size.1).leading_zeros();
        let pyramid_texture = device.create_texture(&wgpu::TextureDescriptor {
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
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let blur_a = pyramid_texture.create_view(&Default::default());
        let mip_views = (0..mip_count)
            .map(|level| {
                pyramid_texture.create_view(&wgpu::TextureViewDescriptor {
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
            bytes: Self::required_bytes(size),
            low_size,
            frame,
            blur_a,
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
            let sx = self.low_size.0 as f32 / self.size.0 as f32;
            let sy = self.low_size.1 as f32 / self.size.1 as f32;
            let x = ((bounds.origin.x.0 - margin) * sx)
                .floor()
                .clamp(0., self.low_size.0 as f32);
            let y = ((bounds.origin.y.0 - margin) * sy)
                .floor()
                .clamp(0., self.low_size.1 as f32);
            let right = ((bounds.origin.x.0 + bounds.size.width.0 + margin) * sx)
                .ceil()
                .clamp(0., self.low_size.0 as f32);
            let bottom = ((bounds.origin.y.0 + bounds.size.height.0 + margin) * sy)
                .ceil()
                .clamp(0., self.low_size.1 as f32);
            let filter_values = [
                self.low_size.0 as f32,
                self.low_size.1 as f32,
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
            if slot == self.captures.len() {
                self.captures.push(self.create_capture(
                    device,
                    layout,
                    filter_values,
                    material_values,
                ));
            } else {
                let capture = &mut self.captures[slot];
                if capture.filter_values != filter_values {
                    for (level, buffer) in capture.filter_buffers.iter().enumerate() {
                        let values = Self::filter_parameters(self.low_size, filter_values, level);
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
    ) -> Capture {
        let buffer = |label, values: &[f32]| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::cast_slice(values),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            })
        };
        let filter_buffers: Vec<_> = (0..self.mip_views.len())
            .map(|level| {
                buffer(
                    "glass_pyramid_params",
                    &Self::filter_parameters(self.low_size, filter_values, level),
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
                    &self.mip_views[level - 1]
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
        let backdrop = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("glass_backdrop"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&self.blur_a),
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
        let [_, _, sigma, native, x, y, right, bottom] = capture.filter_values;
        if right <= x || bottom <= y {
            return 0;
        }
        let sx = self.low_size.0 as f32 / self.size.0 as f32;
        let sy = self.low_size.1 as f32 / self.size.1 as f32;
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
        if sigma == 0. && native == 0. {
            return 0;
        }
        let lod = (if sigma < 2. { 1. + sigma * 0.5 } else { sigma })
            .log2()
            .max(0.);
        let levels = (lod.ceil() as usize + 1).min(self.mip_views.len());
        let mut pixels = 0;
        for level in 0..levels {
            let (width, height) = self.level_size(level);
            let rx = (x / self.low_size.0 as f32 * width as f32).floor() as u32;
            let ry = (y / self.low_size.1 as f32 * height as f32).floor() as u32;
            let rr = (right / self.low_size.0 as f32 * width as f32)
                .ceil()
                .min(width as f32) as u32;
            let rb = (bottom / self.low_size.1 as f32 * height as f32)
                .ceil()
                .min(height as f32) as u32;
            if rr <= rx || rb <= ry {
                continue;
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("glass_pyramid_region"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.mip_views[level],
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
            return [width, height, 0., 0., 0., 0., width * 4., height * 4.];
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
    filter_values: [f32; 8],
    material_values: [f32; MATERIAL_FLOATS],
    filter_buffers: Vec<wgpu::Buffer>,
    material_buffer: wgpu::Buffer,
    filters: Vec<wgpu::BindGroup>,
    backdrop: wgpu::BindGroup,
}
