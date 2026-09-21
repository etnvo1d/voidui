//! cargo run -p voidui_gpui_wgpu --example standalone
//! cargo run -p voidui_gpui_wgpu --example standalone -- --snapshot /tmp/voidui-render.png
use std::{borrow::Cow, sync::Arc};
use voidui_gpui_wgpu::*;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowId},
};

#[derive(Default)]
struct Demo {
    renderer: Option<WgpuRenderer>,
    window: Option<Arc<Window>>,
    text: Option<Arc<TextSystem>>,
    layout: Option<TextLayoutCache>,
    snapshot: Option<String>,
    completed: bool,
}
impl Demo {
    fn draw(&mut self) -> Result<bool> {
        let window = self.window.as_ref().unwrap();
        let renderer = self.renderer.as_mut().unwrap();
        let physical = window.inner_size();
        if physical.width == 0 || physical.height == 0 {
            return Ok(false);
        }
        if renderer.device_lost() {
            renderer.recover(window)?;
        }
        renderer.update_drawable_size(size(
            DevicePixels(physical.width as i32),
            DevicePixels(physical.height as i32),
        ));
        let scale = window.scale_factor() as f32;
        let viewport = size(
            px(physical.width as f32 / scale),
            px(physical.height as f32 / scale),
        );
        let atlas = renderer.sprite_atlas().clone();
        let mut scene = Scene::default();
        let layout = self.layout.as_ref().unwrap();
        {
            let mut painter = Painter::new(
                &mut scene,
                atlas.as_ref(),
                self.text.as_ref().unwrap().clone(),
                viewport,
                scale,
            )?;
            painter.paint_quad(fill(
                Bounds::new(point(px(0.), px(0.)), viewport),
                rgb(0x141923),
            ));
            let mut panel = fill(
                Bounds::new(
                    point(px(32.), px(32.)),
                    size(viewport.width - px(64.), px(350.)),
                ),
                rgb(0x242d3d),
            );
            panel.corner_radii = Corners::all(px(18.));
            panel.border_color = rgb(0x42516a).into();
            panel.border_widths = Edges::all(px(1.));
            painter.paint_quad(panel);
            let title = "Scene + text + GPU, one crate";
            let line = layout.shape_paragraph(
                title.into(),
                &[TextRun {
                    len: title.len(),
                    font: font("IBM Plex Sans"),
                    color: rgb(0xe3ecff).into(),
                    ..Default::default()
                }],
                27.0,
                40.0,
                None,
                None,
            )?;
            line.paint(
                &mut painter,
                Bounds::new(
                    point(56.0, 55.0),
                    size(f32::from(viewport.width) - 112.0, 40.0),
                ),
                TextAlign::Left,
                None,
                None,
            )?;
            let body = "No GPUI App. No GPUI Window. No widget runtime.\nShaping, wrapping, glyph rasterization and atlas uploads are included. Resize this window to reflow the text.";
            let lines = layout.shape_paragraph(
                body.into(),
                &[TextRun {
                    len: body.len(),
                    font: font("IBM Plex Sans"),
                    color: rgb(0xa9b9d1).into(),
                    ..Default::default()
                }],
                18.0,
                28.0,
                Some(f32::from(viewport.width) - 112.0),
                None,
            )?;
            lines.paint(
                &mut painter,
                Bounds::new(
                    point(56.0, 112.0),
                    size(f32::from(viewport.width) - 112.0, 160.0),
                ),
                TextAlign::Left,
                None,
                None,
            )?;
            painter.with_clip(
                Bounds::new(point(px(56.), px(285.)), size(px(240.), px(56.))),
                |p| {
                    let mut q = fill(
                        Bounds::new(point(px(36.), px(285.)), size(px(300.), px(56.))),
                        rgb(0x557de8),
                    );
                    q.corner_radii = Corners::all(px(12.));
                    p.paint_quad(q);
                    p.paint_quad(fill(
                        Bounds::new(point(px(225.), px(300.)), size(px(120.), px(55.))),
                        rgba(0xffa45fa0),
                    ));
                },
            );
            let label = "Clipping + alpha blending";
            let line = layout.shape_paragraph(
                label.into(),
                &[TextRun {
                    len: label.len(),
                    font: font("IBM Plex Sans"),
                    color: white(),
                    underline: Some(UnderlineStyle {
                        thickness: px(1.),
                        color: Some(white()),
                        wavy: false,
                    }),
                    ..Default::default()
                }],
                16.0,
                24.0,
                None,
                None,
            )?;
            line.paint(
                &mut painter,
                Bounds::new(
                    point(68.0, 298.0),
                    size(f32::from(viewport.width) - 112.0, 24.0),
                ),
                TextAlign::Left,
                None,
                None,
            )?;
        }
        scene.finish();
        let presented = renderer.draw(&scene);
        layout.finish_frame();
        if let Some(path) = self.snapshot.as_ref() {
            let rgba = renderer.render_to_rgba(&scene)?;
            let size = renderer.viewport_size();
            let file = std::fs::File::create(path)?;
            let mut encoder = png::Encoder::new(
                std::io::BufWriter::new(file),
                size.width.0 as u32,
                size.height.0 as u32,
            );
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.write_header()?.write_image_data(&rgba)?;
            println!(
                "GPU: {:?}; snapshot: {path}; glyph sprites: {}",
                renderer.gpu_specs(),
                scene.monochrome_sprites.len()
            );
            return Ok(true);
        }
        Ok(presented)
    }
}
impl ApplicationHandler for Demo {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("voidui — standalone voidui_gpui_wgpu")
                        .with_inner_size(winit::dpi::LogicalSize::new(760., 430.)),
                )
                .unwrap(),
        );
        let size_px = window.inner_size();
        let renderer = WgpuRenderer::new(
            Default::default(),
            &window,
            WgpuSurfaceConfig {
                size: size(
                    DevicePixels(size_px.width as i32),
                    DevicePixels(size_px.height as i32),
                ),
                transparent: false,
                preferred_present_mode: None,
            },
            None,
        )
        .expect("initialize WGPU renderer");
        let fonts = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
        fonts
            .add_fonts(vec![Cow::Borrowed(include_bytes!(
                "../tests/fonts/IBMPlexSans-Regular.ttf"
            ))])
            .unwrap();
        let text = Arc::new(TextSystem::new(Arc::new(fonts)));
        self.layout = Some(TextLayoutCache::new(text.clone()));
        self.text = Some(text);
        self.renderer = Some(renderer);
        self.window = Some(window.clone());
        window.request_redraw();
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                self.window.as_ref().unwrap().request_redraw()
            }
            WindowEvent::RedrawRequested if !self.completed => match self.draw() {
                Ok(true) if self.snapshot.is_some() => {
                    self.completed = true;
                    event_loop.exit();
                }
                Ok(false) => self.window.as_ref().unwrap().request_redraw(),
                Ok(true) => {}
                Err(e) => panic!("render failed: {e:#}"),
            },
            _ => {}
        }
    }
}
fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let snapshot = match args.next().as_deref() {
        Some("--snapshot") => Some(args.next().expect("--snapshot requires an output PNG path")),
        None => None,
        _ => anyhow::bail!("usage: standalone [--snapshot output.png]"),
    };
    EventLoop::new()?.run_app(&mut Demo {
        snapshot,
        ..Default::default()
    })?;
    Ok(())
}
