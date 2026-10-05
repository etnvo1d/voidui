#![cfg(feature = "liquid-glass")]

//! Execute the production background function against actual Apple GPU fixtures.
//! Source mips are controlled, so this separates composition from pyramid creation.
use std::path::PathBuf;
use voidui_gpui_wgpu::{block_on, wgpu};
use wgpu::util::DeviceExt;

const SHADER: &str = concat!(
    include_str!("../src/glass_math.wgsl"),
    include_str!("../src/glass_background.wgsl"),
    r#"
@group(0) @binding(0) var source:texture_2d<f32>;
@group(0) @binding(1) var sampling:sampler;
@group(0) @binding(2) var sdf:texture_2d<f32>;
@group(0) @binding(3) var<uniform> params:GlassBackgroundParams;
struct Vertex { @builtin(position) position:vec4<f32>, @location(0) uv:vec2<f32> }
// Use the same quad/interpolants as the native oracle, not a fullscreen
// triangle with fragment-side UV division: that changes texture interpolation.
@vertex fn vertex_main(@builtin(vertex_index) i:u32)->Vertex {
    let uv=vec2<f32>(f32(i&1u),f32(i>>1u));
    return Vertex(vec4<f32>(uv.x*2.0-1.0,1.0-uv.y*2.0,0.0,1.0),uv);
}
@fragment fn fragment_main(input:Vertex)->@location(0) vec4<f32> {
    let dimensions=vec2<f32>(textureDimensions(sdf));let uv=input.uv;
    var field=textureSampleLevel(sdf,sampling,uv,0.0);
    var coverage=saturate(0.5-field.x/max(fwidth(field.x),0.0001))*field.w;
    let shadow=textureSampleLevel(sdf,sampling,uv+params.shadow_geometry.xy/dimensions,0.0);
    return glass_background(source,sampling,uv,field,coverage,shadow.xw,params);
}
"#
);

#[test]
fn background_branches_match_native_gpu() {
    let path = std::env::var_os("VOIDUI_GLASS_BACKGROUND_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/glass-background")
        });
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path.join("manifest.json")).unwrap()).unwrap();
    assert!(
        manifest["precision"].is_null() || manifest["precision"] == "float",
        "this test verifies the FP32 production compositor, not the unverified FP16 experiment"
    );
    let width = manifest["width"].as_u64().unwrap() as u32;
    let height = manifest["height"].as_u64().unwrap() as u32;
    let levels = manifest["mip_count"].as_u64().unwrap() as u32;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter =
        block_on(instance.request_adapter(&Default::default())).expect("GPU adapter required");
    // Only this oracle fixture uses float32 filtering. The actual UI uses RGBA16F.
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: wgpu::Features::FLOAT32_FILTERABLE,
        ..Default::default()
    }))
    .unwrap();
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("native_background_comparison"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("native_background_comparison"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vertex_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fragment_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba32Float,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });
    let make_texture = |mips, usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: mips,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage,
            view_formats: &[],
        })
    };
    let source = make_texture(
        levels,
        wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
    );
    for level in 0..levels {
        let (w, h) = ((width >> level).max(1), (height >> level).max(1));
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                mip_level: level,
                ..source.as_image_copy()
            },
            &std::fs::read(path.join(format!("source-{level}.bin"))).unwrap(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 16),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
    }
    let sdf = make_texture(
        1,
        wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
    );
    queue.write_texture(
        sdf.as_image_copy(),
        &std::fs::read(path.join("sdf.bin")).unwrap(),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 16),
            rows_per_image: None,
        },
        sdf.size(),
    );
    let target = make_texture(
        1,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let target_view = target.create_view(&Default::default());
    let source_view = source.create_view(&Default::default());
    let sdf_view = sdf.create_view(&Default::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        ..Default::default()
    });
    let stride = (width * 16).div_ceil(256) * 256;
    for case in manifest["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(name),
            contents: &std::fs::read(path.join(format!("{name}-portable.bin"))).unwrap(),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(name),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&source_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&sdf_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: params.as_entire_binding(),
                },
            ],
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(stride * height),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &binding, &[]);
            pass.draw(0..4, 0..1);
        }
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: None,
                },
            },
            target.size(),
        );
        queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        readback.map_async(wgpu::MapMode::Read, .., move |r| tx.send(r).unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        rx.recv().unwrap().unwrap();
        let actual = readback.get_mapped_range(..);
        let expected = std::fs::read(path.join(format!("{name}-expected.bin"))).unwrap();
        let mut maximum = 0f32;
        let mut worst = (0, 0, 0, 0f32, 0f32);
        for y in 0..height {
            for x in 0..width {
                for channel in 0..4 {
                    let offset = (y * stride + x * 16 + channel * 4) as usize;
                    let at = ((y * width + x) * 16 + channel * 4) as usize;
                    let a = f32::from_le_bytes(actual[offset..][..4].try_into().unwrap());
                    let b = f32::from_le_bytes(expected[at..][..4].try_into().unwrap());
                    assert!(
                        a.is_finite() && b.is_finite(),
                        "non-finite {name} at ({x},{y})"
                    );
                    let error = (a - b).abs();
                    if error > maximum {
                        maximum = error;
                        worst = (x, y, channel, a, b);
                    }
                }
            }
        }
        println!("{name}: maximum channel error={maximum} worst={worst:?}");
        assert!(
            maximum <= 0.000002,
            "background branch {name} mismatch: {maximum} at {worst:?}"
        );
    }
}
