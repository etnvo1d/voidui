//! `cargo run --example effects` shows gradients, color spaces, shadows and transitions.
//! `--smoke <directory>` checks native animation, cached layout and idle scheduling.
use anyhow::Result;
use std::{
    borrow::Cow,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use voidui::{
    Application, WindowOptions,
    core::geometry::Size,
    div,
    render::{ParleyTextSystem, TextSystem},
    text,
};
fn snapshot(window: &mut voidui::AppWindow, path: &Path) -> Result<()> {
    let (size, pixels) = window.snapshot()?;
    // A flat conic sector must reproduce its authored sRGB bytes. This catches
    // accidental transfer-function conversion on an UNORM presentation target.
    let root = window.tree().root().unwrap();
    let cards = window.tree().children(root)[2];
    let conic = window.tree().children(cards)[2];
    let bounds = window.tree().bounds(conic);
    let scale = size.width as f32 / window.logical_size().width;
    let x = ((bounds.origin.x + bounds.size.width * 0.9) * scale) as usize;
    let y = ((bounds.origin.y + bounds.size.height * 0.5) * scale) as usize;
    let pixel = &pixels[(y * size.width as usize + x) * 4..][..4];
    anyhow::ensure!(
        pixel
            .iter()
            .zip([112u8, 232, 192, 255])
            .all(|(a, b)| a.abs_diff(b) <= 1),
        "incorrect conic color: {pixel:?}"
    );
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, size.width, size.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&pixels)?;
    Ok(())
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.as_slice() == ["--pixels"] {
        return verify_pixels();
    }
    let output = match args.as_slice() {
        [] => None,
        [mode, path] if mode == "--smoke" => {
            std::fs::create_dir_all(path)?;
            Some(std::path::PathBuf::from(path))
        }
        _ => anyhow::bail!("usage: effects [--smoke directory]"),
    };
    let fonts = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    fonts.add_fonts(vec![Cow::Borrowed(include_bytes!(
        "../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
    ))])?;
    let content=div().id("app")
        .child(text("Color, light, and motion.").class("title"))
        .child(text("CSS color spaces and multi-stop gradients, painted directly by the GPU.").class("subtitle"))
        .child(div().class("cards")
            .child(div().class("card linear").child("Linear / OKLCH"))
            .child(div().class("card radial").child("Radial / Oklab"))
            .child(div().class("card conic").child("Conic / hard stops")))
        .child(div().id("animated").child(text("Hover to transition").class("heading"))
            .child("Inherited color, rounded corners and layered shadows animate without reflowing text."))
        .child(text("sRGB output / floating-point colors / cached ramps / demand-driven frames.").class("footer"));
    let app = Application::new()
        .text_system(Arc::new(TextSystem::new(Arc::new(fonts))))
        .css(include_str!("styles/effects.css"))?
        .window(
            WindowOptions {
                title: "voidui — CSS effects".into(),
                size: Size::new(860.0, 510.0),
                ..Default::default()
            },
            content,
        );
    let mut stage = 0;
    let mut baseline = None;
    let mut idle = None;
    let mut times = Vec::with_capacity(120);
    let mut done = None;
    let app = if let Some(output) = output {
        app.on_frame(move |window| {
        match stage {
            0=>{
                snapshot(window,&output.join("before.png"))?;
                let root=window.tree().root().unwrap();let animated=window.tree().children(root)[3];
                window.tree_mut().set_classes(animated,"active");stage=1;
            }
            1=>{baseline=Some(window.stats());stage=2;}
            2=>{
                times.push(window.stats().last_present_time.as_secs_f64()*1000.0);
                if window.tree().next_animation_frame(Instant::now()).is_none(){
                    let initial=baseline.unwrap();let final_stats=window.stats();
                    anyhow::ensure!(final_stats.layout_passes==initial.layout_passes,"paint-only transition caused layout");
                    anyhow::ensure!(final_stats.scene_builds>initial.scene_builds+3,"transition did not animate");
                    anyhow::ensure!(final_stats.failed_frames==initial.failed_frames,"presentation failed");
                    snapshot(window,&output.join("after.png"))?;
                    println!("PASS animation: {:?}",final_stats);
                    idle=Some(final_stats);stage=3;
                    let (tx,rx)=std::sync::mpsc::channel();done=Some(rx);
                    let native=window.native_window().clone();
                    // Diagnostics only: one explicit wake after an idle interval.
                    std::thread::spawn(move||{std::thread::sleep(Duration::from_secs(2));let _=tx.send(());native.request_redraw();});
                }
            }
            3 if done.as_ref().is_some_and(|rx|rx.try_recv().is_ok())=>{
                let before=idle.unwrap();let after=window.stats();
                anyhow::ensure!(after.presented_frames==before.presented_frames+1,"unexpected redraw while idle");
                anyhow::ensure!(after.layout_passes==before.layout_passes && after.scene_builds==before.scene_builds,"idle content was rebuilt");
                times.sort_by(f64::total_cmp);
                println!("PASS idle: 2s, zero layout/scene rebuilds; present p50={:.3}ms p95={:.3}ms",times[times.len()/2],times[times.len()*95/100]);window.close();
            }
            _=>{}
        }
        Ok(())
    })
    } else {
        app
    };
    app.run()
}

/// Small GPU-readback fixture for effects whose bugs can hide in attractive demos.
fn verify_pixels() -> Result<()> {
    let css="
        #root {width:360px;height:220px;background:white;position:relative;}
        #outer {position:absolute;left:20px;top:20px;width:40px;height:40px;box-shadow:10px 0 0 red;}
        #inner {position:absolute;left:90px;top:20px;width:40px;height:40px;background:white;border:4px solid black;box-shadow:inset 10px 0 0 blue;}
        #layers {position:absolute;left:160px;top:20px;width:40px;height:40px;box-shadow:10px 0 0 red,15px 0 0 blue;}
        #linear {position:absolute;left:20px;top:100px;width:100px;height:50px;background:linear-gradient(to right in srgb,red 50%,blue 50%);}
        #alpha {position:absolute;left:160px;top:100px;width:100px;height:50px;background:linear-gradient(to right in srgb,transparent,red) white;}
        #radial {position:absolute;left:280px;top:100px;width:60px;height:60px;background:radial-gradient(circle 20px,red 50%,blue 50%);}
    ";
    Application::new().text_system(Arc::new(TextSystem::new(Arc::new(ParleyTextSystem::new_without_system_fonts("unused")))))
        .css(css)?.window(WindowOptions {title:"voidui pixel checks".into(),size:Size::new(360.0,220.0),..Default::default()},
            div().id("root").child(div().id("outer")).child(div().id("inner")).child(div().id("layers"))
                .child(div().id("linear")).child(div().id("alpha")).child(div().id("radial")))
        .on_frame(|window| {
            let (size,pixels)=window.snapshot()?;let scale=size.width as f32/window.logical_size().width;
            let pixel=|x:f32,y:f32| {let offset=((y*scale) as usize*size.width as usize+(x*scale) as usize)*4;[pixels[offset],pixels[offset+1],pixels[offset+2]]};
            for (x,y,expected) in [(40.0,40.0,[255,255,255]),(65.0,40.0,[255,0,0]),
                (92.0,40.0,[0,0,0]),(98.0,40.0,[0,0,255]),(112.0,40.0,[255,255,255]),
                (205.0,40.0,[255,0,0]),(213.0,40.0,[0,0,255]),
                (69.0,125.0,[255,0,0]),(71.0,125.0,[0,0,255]),
                (310.0,130.0,[255,0,0]),(331.0,130.0,[0,0,255])] {
                let actual=pixel(x,y);anyhow::ensure!(actual.iter().zip(expected).all(|(a,b)|a.abs_diff(b)<=1),"pixel ({x},{y}) {actual:?} != {expected:?}");
            }
            let [r,g,b]=pixel(210.0,125.0);anyhow::ensure!(r==255 && (125..=129).contains(&g) && g==b,"premultiplied midpoint: {:?}",[r,g,b]);
            println!("PASS GPU pixels: shadow knockout, inset padding clip, list order, hard stops, radial geometry, alpha interpolation");window.close();Ok(())
        }).run()
}
