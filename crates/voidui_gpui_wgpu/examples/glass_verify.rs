//! Native GPU pixel checks and synchronized timings. No screenshot permission needed.
//! Run with `cargo run --release -p voidui_gpui_wgpu --example glass_verify`.
use std::{sync::Arc, time::Instant};
use voidui_gpui_wgpu::*;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowId},
};

fn rect(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
    Bounds::new(point(px(x), px(y)), size(px(w), px(h)))
}
fn identity() -> GlassMaterial {
    GlassMaterial {
        background: None,
        tone: None,
        blur: 0.,
        refraction: 0.,
        saturation: 1.,
        tint: [0.; 4],
        highlight: 0.,
        ..Default::default()
    }
}
fn scene(
    renderer: &WgpuRenderer,
    material: Option<GlassMaterial>,
    foreground: bool,
    count: usize,
    alpha: f32,
) -> Scene {
    let mut scene = Scene::default();
    let text = Arc::new(TextSystem::new(Arc::new(
        ParleyTextSystem::new_without_system_fonts("unused"),
    )));
    let mut p = Painter::new(
        &mut scene,
        renderer.sprite_atlas().as_ref(),
        text,
        size(px(640.), px(360.)),
        1.,
    )
    .unwrap();
    for x in (0..640).step_by(8) {
        let c = if x % 16 == 0 {
            rgba(0x2060a0ff)
        } else {
            rgba(0xe0c080ff)
        };
        p.paint_quad(fill(rect(x as f32, 0., 8., 360.), c.alpha(alpha)));
    }
    if let Some(material) = material {
        for i in 0..count {
            let (x, y, w, h) = if count == 1 {
                (80., 60., 480., 240.)
            } else {
                (
                    12. + (i % 8) as f32 * 78.,
                    12. + (i / 8) as f32 * 82.,
                    70.,
                    70.,
                )
            };
            p.paint_glass(rect(x, y, w, h), Corners::all(px(24.)), material);
        }
    }
    if foreground {
        p.paint_quad(fill(rect(300., 150., 20., 20.), rgb(0xff0000)));
    }
    drop(p);
    scene.finish();
    scene
}
fn pixel(bytes: &[u8], x: usize, y: usize) -> [u8; 4] {
    bytes[(y * 640 + x) * 4..][..4].try_into().unwrap()
}
fn near(actual: [u8; 4], expected: [u8; 4], tolerance: u8) {
    assert!(
        actual
            .into_iter()
            .zip(expected)
            .all(|(a, b)| a.abs_diff(b) <= tolerance),
        "{actual:?} != {expected:?}"
    );
}
fn verify(renderer: &mut WgpuRenderer) -> Result<()> {
    let baseline = renderer.render_to_rgba(&scene(renderer, None, false, 0, 1.))?;
    let neutral = renderer.render_to_rgba(&scene(renderer, Some(identity()), false, 1, 1.))?;
    for (a, b) in baseline.iter().zip(&neutral) {
        assert!(a.abs_diff(*b) <= 1);
    }
    let half_baseline = renderer.render_to_rgba(&scene(renderer, None, false, 0, 0.5))?;
    let half = renderer.render_to_rgba(&scene(renderer, Some(identity()), false, 1, 0.5))?;
    near(pixel(&half, 320, 180), pixel(&half_baseline, 320, 180), 1);
    let refracted = renderer.render_to_rgba(&scene(
        renderer,
        Some(GlassMaterial {
            refraction: 13.,
            ..identity()
        }),
        false,
        1,
        1.,
    ))?;
    near(pixel(&refracted, 320, 180), pixel(&baseline, 320, 180), 1);
    assert_ne!(pixel(&refracted, 83, 180), pixel(&baseline, 83, 180));
    let blurred = renderer.render_to_rgba(&scene(
        renderer,
        Some(GlassMaterial {
            blur: 8.,
            ..identity()
        }),
        true,
        1,
        1.,
    ))?;
    near(pixel(&blurred, 310, 160), [255, 0, 0, 255], 1);
    near(pixel(&blurred, 82, 62), pixel(&baseline, 82, 62), 1); // rounded corner
    near(pixel(&blurred, 20, 180), pixel(&baseline, 20, 180), 1); // outside
    assert!(
        pixel(&blurred, 320, 180)[0].abs_diff(pixel(&blurred, 328, 180)[0]) < 12,
        "blur did not suppress stripes"
    );

    // Overlap must capture the already composed lower material, never stale pixels.
    let mut layers = Scene::default();
    let text = Arc::new(TextSystem::new(Arc::new(
        ParleyTextSystem::new_without_system_fonts("unused"),
    )));
    let mut p = Painter::new(
        &mut layers,
        renderer.sprite_atlas().as_ref(),
        text.clone(),
        size(px(640.), px(360.)),
        1.,
    )?;
    p.paint_quad(fill(rect(0., 0., 640., 360.), rgb(0x000000)));
    p.paint_glass(
        rect(60., 60., 400., 240.),
        Corners::all(px(20.)),
        GlassMaterial {
            tint: [1., 0., 0., 0.5],
            ..identity()
        },
    );
    p.paint_glass(
        rect(160., 60., 400., 240.),
        Corners::all(px(20.)),
        GlassMaterial {
            tint: [0., 0., 1., 0.5],
            ..identity()
        },
    );
    drop(p);
    layers.finish();
    let pixels = renderer.render_to_rgba(&layers)?;
    near(pixel(&pixels, 100, 180), [128, 0, 0, 255], 1);
    near(pixel(&pixels, 300, 180), [64, 0, 128, 255], 1);
    near(pixel(&pixels, 500, 180), [0, 0, 128, 255], 1);

    let mut clipped = Scene::default();
    let mut p = Painter::new(
        &mut clipped,
        renderer.sprite_atlas().as_ref(),
        text,
        size(px(640.), px(360.)),
        1.,
    )?;
    p.paint_quad(fill(rect(0., 0., 640., 360.), rgb(0x000000)));
    p.with_space(
        &PaintSpace::new(
            Affine::translate(200., 100.).compose(Affine::rotate(0.4)),
            None,
        ),
        |p| {
            p.with_clip(rect(0., 0., 40., 100.), |p| {
                p.paint_glass(
                    rect(0., 0., 100., 100.),
                    Corners::all(px(12.)),
                    GlassMaterial {
                        tint: [1., 0., 0., 1.],
                        ..identity()
                    },
                )
            });
        },
    );
    drop(p);
    clipped.finish();
    let pixels = renderer.render_to_rgba(&clipped)?;
    near(pixel(&pixels, 201, 128), [255, 0, 0, 255], 1);
    near(pixel(&pixels, 258, 147), [0, 0, 0, 255], 1);

    let retained = scene(renderer, Some(GlassMaterial::regular()), true, 1, 1.);
    // Warm presentation; a swapchain can be temporarily unavailable at startup.
    let mut presented = false;
    for _ in 0..10 {
        if renderer.draw(&retained) {
            presented = true;
            break;
        }
    }
    anyhow::ensure!(presented, "surface unavailable during retained-frame check");
    let before = renderer.stats();
    anyhow::ensure!(renderer.draw(&retained), "retained presentation failed");
    let after = renderer.stats();
    assert_eq!(before.glass_captures, after.glass_captures);
    assert!(after.retained_presentations > before.retained_presentations);
    renderer.set_cache_options(RenderCacheOptions {
        glass_bytes: 0,
        ..Default::default()
    });
    renderer.render_to_rgba(&retained)?;
    assert_eq!(renderer.stats().glass_bytes, 0);
    assert!(renderer.stats().glass_fallbacks > 0);
    renderer.set_cache_options(Default::default());
    println!(
        "PASS identity, transparent alpha, refraction, blur, foreground, corners, overlap, affine clip, retained frame, memory fallback"
    );

    let text = Arc::new(TextSystem::new(Arc::new(
        ParleyTextSystem::new_without_system_fonts("unused"),
    )));
    let mut grouped = scene(renderer, None, false, 0, 1.);
    let mut p = Painter::new(
        &mut grouped,
        renderer.sprite_atlas().as_ref(),
        text,
        size(px(640.), px(360.)),
        1.,
    )?;
    let shapes: Vec<_> = (0..32)
        .map(|i| {
            (
                rect(
                    12. + (i % 8) as f32 * 78.,
                    12. + (i / 8) as f32 * 82.,
                    70.,
                    70.,
                ),
                Corners::all(px(24.)),
            )
        })
        .collect();
    p.paint_glass_group(&shapes, GlassMaterial::regular());
    drop(p);
    grouped.finish();
    let before = renderer.stats();
    renderer.render_to_rgba(&grouped)?;
    assert_eq!(renderer.stats().glass_captures - before.glass_captures, 1);
    println!("PASS 32 sibling lenses share one capture");

    // Geometry regression: a shared backdrop alone cannot paint between boxes.
    for (gap, merged) in [(6., true), (40., false)] {
        let mut lens = scene(renderer, None, false, 0, 1.);
        let text = Arc::new(TextSystem::new(Arc::new(
            ParleyTextSystem::new_without_system_fonts("unused"),
        )));
        let mut p = Painter::new(
            &mut lens,
            renderer.sprite_atlas().as_ref(),
            text,
            size(px(640.), px(360.)),
            1.,
        )?;
        let shapes = [
            (rect(180., 100., 100., 120.), Corners::all(px(40.))),
            (rect(280. + gap, 100., 100., 120.), Corners::all(px(40.))),
        ];
        p.paint_liquid_glass_group(
            &shapes,
            GlassMaterial {
                tint: [1., 0., 0., 1.],
                ..identity()
            },
            32.,
        );
        drop(p);
        lens.finish();
        let result = renderer.render_to_rgba(&lens)?;
        let x = (280. + gap * 0.5) as usize;
        if merged {
            near(pixel(&result, x, 160), [255, 0, 0, 255], 1);
        } else {
            near(pixel(&result, x, 160), pixel(&baseline, x, 160), 1);
        }
    }
    println!("PASS liquid bridge appears at gap 6 and disappears at gap 40");

    // Transparent shadow padding must not replace a previous sibling's face.
    let mut optical = scene(renderer, None, false, 0, 1.);
    let text = Arc::new(TextSystem::new(Arc::new(
        ParleyTextSystem::new_without_system_fonts("unused"),
    )));
    let mut p = Painter::new(
        &mut optical,
        renderer.sprite_atlas().as_ref(),
        text,
        size(px(640.), px(360.)),
        1.,
    )?;
    let mut background = GlassBackground::clear();
    let mut shadow = GlassBackground::regular().shadow.unwrap();
    shadow.opacity = 1e-8;
    shadow.radius = 100.;
    background.shadow = Some(shadow);
    background.holding = None;
    let material = GlassMaterial {
        background: Some(background),
        tone: Some(GlassTone {
            white: 1.,
            black: 0.,
            saturation: 1.,
            fill: [1., 0., 0., 1.],
        }),
        ..identity()
    };
    p.paint_glass_group(
        &[
            (rect(100., 100., 80., 80.), Corners::all(px(20.))),
            (rect(200., 100., 80., 80.), Corners::all(px(20.))),
        ],
        material,
    );
    drop(p);
    optical.finish();
    let pixels = renderer.render_to_rgba(&optical)?;
    near(pixel(&pixels, 140, 140), [255, 0, 0, 255], 1);
    near(pixel(&pixels, 240, 140), [255, 0, 0, 255], 1);
    println!("PASS optical source-over preserves sibling faces beneath transparent padding");

    // These are end-to-end render + readback timings, NOT isolated GPU timestamps.
    // Warm each workload before measuring to exclude shader compilation.
    for (count, shared) in [(0, false), (1, false), (8, false), (32, false), (32, true)] {
        let workload = scene(renderer, Some(GlassMaterial::regular()), false, count, 1.);
        let workload = if shared { &grouped } else { &workload };
        for _ in 0..3 {
            renderer.render_to_rgba(workload)?;
        }
        let before = renderer.stats();
        let mut elapsed = Vec::new();
        for _ in 0..30 {
            let start = Instant::now();
            renderer.render_to_rgba(&workload)?;
            elapsed.push(start.elapsed().as_secs_f64() * 1000.);
        }
        elapsed.sort_by(f64::total_cmp);
        let after = renderer.stats();
        println!(
            "640x360 glass={count} shared={shared}: synchronized render/readback median={:.3}ms p95={:.3}ms scratch={}B filtered_pixels/frame={}",
            elapsed[15],
            elapsed[28],
            after.glass_bytes,
            (after.glass_filter_pixels - before.glass_filter_pixels) / 30
        );
    }
    Ok(())
}
#[derive(Default)]
struct Check {
    renderer: Option<WgpuRenderer>,
    window: Option<Arc<Window>>,
    result: Option<Result<()>>,
    attempts: usize,
    retry_at: Option<Instant>,
}
impl ApplicationHandler for Check {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("VoidUI glass GPU verification")
                        .with_inner_size(winit::dpi::PhysicalSize::new(640, 360)),
                )
                .unwrap(),
        );
        let renderer = WgpuRenderer::new(
            Default::default(),
            &window,
            WgpuSurfaceConfig {
                size: size(DevicePixels(640), DevicePixels(360)),
                transparent: true,
                preferred_present_mode: None,
            },
            None,
        )
        .unwrap();
        println!("GPU: {:?}", renderer.gpu_specs());
        self.renderer = Some(renderer);
        window.request_redraw();
        self.window = Some(window);
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        if matches!(event, WindowEvent::RedrawRequested) && self.result.is_none() {
            let renderer = self.renderer.as_mut().unwrap();
            let warm = scene(renderer, None, false, 0, 1.);
            if !renderer.draw(&warm) {
                self.attempts += 1;
                if self.attempts >= 100 {
                    self.result = Some(Err(anyhow::anyhow!("native surface did not become ready")));
                    event_loop.exit();
                } else {
                    // Give the native compositor time to publish a drawable. Tight
                    // redraw retries can exhaust their count before AppKit commits.
                    self.retry_at = Some(Instant::now() + std::time::Duration::from_millis(20));
                }
                return;
            }
            self.result = Some(verify(renderer));
            event_loop.exit();
        }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(deadline) = self.retry_at {
            if Instant::now() >= deadline {
                self.retry_at = None;
                self.window.as_ref().unwrap().request_redraw();
            } else {
                event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(deadline));
            }
        }
    }
}
struct Diagnostics;
impl log::Log for Diagnostics {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        if record.level() <= log::Level::Info {
            eprintln!("{}", record.args());
        }
    }
    fn flush(&self) {}
}
fn main() -> Result<()> {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Info);
    let mut check = Check::default();
    EventLoop::new()?.run_app(&mut check)?;
    check
        .result
        .unwrap_or_else(|| anyhow::bail!("verification did not run"))
}
