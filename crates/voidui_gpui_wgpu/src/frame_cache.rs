//! Explicit backing textures preserve swapchain independence and paint order.
//! Two lazily populated variants retain the caret's original stacking position.
use crate::Scene;

pub(crate) struct FrameCache {
    pub revision: u64,
    pub size: (u32, u32),
    pub textures: [Option<wgpu::Texture>; 2],
    pub ready: [bool; 2],
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
}
impl FrameCache {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, size: (u32, u32)) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("retained_frame_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("retained_frame_shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "frame_cache.wgsl"
            ))),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("retained_frame_pipeline_layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("retained_frame_blit"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        Self {
            revision: 0,
            size,
            textures: [None, None],
            ready: [false; 2],
            layout,
            pipeline,
        }
    }
    pub fn slot(scene: &Scene) -> usize {
        usize::from(scene.caret_visible && scene.quads.iter().any(|q| q.spatial_pad & 1 != 0))
    }
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        scene: &Scene,
    ) -> (wgpu::TextureView, bool) {
        if self.revision != scene.revision {
            self.revision = scene.revision;
            self.ready = [false; 2];
        }
        let slot = Self::slot(scene);
        if !scene.quads.iter().any(|q| q.spatial_pad & 1 != 0) {
            self.textures[1] = None;
            self.ready[1] = false;
        }
        let texture = self.textures[slot].get_or_insert_with(|| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("retained_frame"),
                size: wgpu::Extent3d {
                    width: self.size.0,
                    height: self.size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        });
        (texture.create_view(&Default::default()), !self.ready[slot])
    }
    pub fn present(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &wgpu::TextureView,
        target: &wgpu::TextureView,
    ) {
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("retained_frame_binding"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(source),
            }],
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("retained_frame_present"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &binding, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit(Some(encoder.finish()));
    }
}
