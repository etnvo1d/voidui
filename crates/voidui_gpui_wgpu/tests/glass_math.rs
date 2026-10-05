#![cfg(feature = "liquid-glass")]

//! Differential arithmetic checks against extracted AIR with host intrinsics.
//! This does not assert WindowServer pixel parity or GPU fast-math identity.
use voidui_gpui_wgpu::{block_on, wgpu};
use wgpu::util::DeviceExt;

const SOURCE: &str = concat!(
    include_str!("../src/glass_math.wgsl"),
    r#"
struct Case { header: vec4<f32>, input: array<vec4<f32>,7>, expected: vec4<f32> }
@group(0) @binding(0) var<storage,read> cases: array<Case>;
@group(0) @binding(1) var<storage,read_write> results: array<vec4<f32>>;
@compute @workgroup_size(64) fn verify(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= arrayLength(&cases) { return; }
    let c=cases[id.x];
    var value:vec4<f32>;
    switch u32(c.header.x) {
        case 0u: { value=glass_union(c.input[0],c.input[1],c.input[2].x); }
        case 1u: { value=vec4<f32>(glass_supercircle(c.input[0].xy,c.input[0].zw,c.input[1].x,c.input[1].yz),1.0); }
        case 2u: { value=glass_key_fill(c.input[0],c.input[6].x,c.input[1],c.input[2],c.input[3],c.input[4],c.input[5]); }
        default: { value=vec4<f32>(c.input[0].w+glass_refraction(c.input[0].x,c.input[0].y,c.input[0].z)/256.0,c.input[1].x,0.25,1.0); }
    }
    results[id.x]=value;
}
"#
);

#[test]
fn gpu_math_matches_recovered_air_fixtures() {
    // Override allows a larger locally generated corpus without shipping native IR.
    let bytes = std::env::var_os("VOIDUI_GLASS_FIXTURES")
        .map(|p| std::fs::read(p).unwrap())
        .unwrap_or_else(|| include_bytes!("fixtures/glass-math.bin").to_vec());
    assert_eq!(bytes.len() % 144, 0);
    let count = bytes.len() / 144;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter =
        block_on(instance.request_adapter(&Default::default())).expect("GPU adapter required");
    let (device, queue) =
        block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("glass_math_verify"),
        source: wgpu::ShaderSource::Wgsl(SOURCE.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("glass_math_verify"),
        layout: None,
        module: &module,
        entry_point: Some("verify"),
        compilation_options: Default::default(),
        cache: None,
    });
    let input = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("native_math_fixtures"),
        contents: &bytes,
        usage: wgpu::BufferUsages::STORAGE,
    });
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("translated_math"),
        size: (count * 16) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("math_readback"),
        size: (count * 16) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: input.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: output.as_entire_binding(),
            },
        ],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &binding, &[]);
        pass.dispatch_workgroups((count as u32).div_ceil(64), 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &staging, 0, (count * 16) as u64);
    queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    staging.map_async(wgpu::MapMode::Read, .., move |r| tx.send(r).unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    rx.recv().unwrap().unwrap();
    let actual = staging.get_mapped_range(..);
    let mut maximum = [0f32; 4];
    for i in 0..count {
        let op = f32::from_le_bytes(bytes[i * 144..i * 144 + 4].try_into().unwrap()) as usize;
        for component in 0..4 {
            let a = f32::from_le_bytes(actual[i * 16 + component * 4..][..4].try_into().unwrap());
            let expected = f32::from_le_bytes(
                bytes[i * 144 + 128 + component * 4..][..4]
                    .try_into()
                    .unwrap(),
            );
            let error = (a - expected).abs();
            maximum[op] = maximum[op].max(error);
            // One half-ULP boundary can move a continuous-corner distance by r/2048.
            // Report absolute error too; no screenshot tolerance is inferred here.
            let tolerance = if op == 1 {
                bytes[i * 144 + 32..i * 144 + 36]
                    .try_into()
                    .map(f32::from_le_bytes)
                    .unwrap()
                    .abs()
                    / 1024.
                    + 0.0001
            } else if op == 3 {
                0.00001
            } else {
                0.00005
            };
            assert!(
                a.is_finite() && error <= tolerance,
                "case={i} op={op} component={component}: {a} != {expected}, error={error}, tolerance={tolerance}"
            );
        }
    }
    println!(
        "{} cases on {}: maximum error union/shape/highlight/native refraction={maximum:?}",
        count,
        adapter.get_info().name
    );
}

#[test]
fn tone_matrices_match_native_color_matrix_calls() {
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/glass-tone.json")).unwrap();
    let mut maximum = 0f32;
    for case in fixtures["cases"].as_array().unwrap() {
        let input: Vec<f32> = case["input"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap() as f32)
            .collect();
        let actual = voidui_gpui_wgpu::GlassTone {
            white: input[0],
            black: input[1],
            saturation: input[2],
            fill: input[3..].try_into().unwrap(),
        }
        .matrix();
        for row in 0..3 {
            for column in 0..4 {
                let expected = case["expected"][row][column].as_f64().unwrap() as f32;
                let error = (actual[row][column] - expected).abs();
                maximum = maximum.max(error);
                assert!(
                    error < 0.000001,
                    "matrix [{row},{column}]: {} != {expected}; input={input:?}",
                    actual[row][column]
                );
            }
        }
    }
    println!("64 native color-matrix comparisons: maximum error={maximum}");
}

#[test]
fn pyramid_kernel_matches_installed_native_gpu_kernel() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) = block_on(adapter.request_device(&Default::default())).unwrap();
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("native_blur_comparison"),
        source: wgpu::ShaderSource::Wgsl(
            concat!(
                include_str!("../src/glass_math.wgsl"),
                include_str!("../src/glass_filter.wgsl")
            )
            .into(),
        ),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("native_blur_comparison"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vertex_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("pyramid"),
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
    });
    let texture = |w, h, usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage,
            view_formats: &[],
        })
    };
    let source = texture(
        128,
        96,
        wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
    );
    queue.write_texture(
        source.as_image_copy(),
        include_bytes!("fixtures/glass-blur-source.rgba16f"),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(128 * 8),
            rows_per_image: None,
        },
        source.size(),
    );
    let target = texture(
        64,
        48,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let view = target.create_view(&Default::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&[64f32, 48., 0., 0., 0., 0., 128., 96.]),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(
                    &source.create_view(&Default::default()),
                ),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: params.as_entire_binding(),
            },
        ],
    });
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 64 * 48 * 8,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
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
        pass.draw(0..3, 0..1);
    }
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &staging,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(64 * 8),
                rows_per_image: None,
            },
        },
        target.size(),
    );
    queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    staging.map_async(wgpu::MapMode::Read, .., move |r| tx.send(r).unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    rx.recv().unwrap().unwrap();
    let actual = staging.get_mapped_range(..);
    let expected = include_bytes!("fixtures/glass-blur-native.rgba16f");
    // Fixture channels are nonnegative, finite and normalized: adjacent half bit
    // patterns are adjacent representable values, making ULP comparison exact.
    let mut max_ulp = 0;
    for (a, b) in actual.chunks_exact(2).zip(expected.chunks_exact(2)) {
        let a = u16::from_le_bytes(a.try_into().unwrap());
        let b = u16::from_le_bytes(b.try_into().unwrap());
        assert!(a < 0x7c00 && b < 0x7c00);
        max_ulp = max_ulp.max(a.abs_diff(b));
    }
    println!("native blur kernel: max half ULP={max_ulp}");
    assert!(
        max_ulp <= 2,
        "native blur kernel differs by {max_ulp} half ULPs"
    );
}
