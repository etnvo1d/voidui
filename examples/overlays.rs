//! Ordinary div/text elements + standard CSS + Rust top-layer lifecycle.
//! `cargo run --example overlays [--smoke directory]`
use anyhow::Result;
use std::{
    borrow::Cow,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use voidui::{
    Application, WindowOptions,
    core::{
        geometry::{Point, Size},
        top_layer::HitTarget,
        widget::WidgetId,
    },
    div,
    render::{ParleyTextSystem, TextSystem},
    text,
};
use winit::event::{ElementState, MouseButton, WindowEvent};

fn open(window: &mut voidui::AppWindow, number: &mut usize) -> Result<WidgetId> {
    *number += 1;
    let parent = window
        .tree()
        .active_modal()
        .unwrap_or(window.tree().find_by_id("clip").unwrap());
    let element = div()
        .tag("dialog")
        .id(format!("dialog-{number}"))
        .child(text(format!("Modal {number}")).class("heading"))
        .child(
            text("Open another modal. Each new entry is above the previous top layer.")
                .class("description"),
        )
        .child(
            div()
                .class("row")
                .child(
                    div()
                        .tag("button")
                        .class("spawn")
                        .attr("autofocus", "")
                        .attr("data-action", "open")
                        .child("Open another"),
                )
                .child(
                    div()
                        .tag("button")
                        .attr("data-action", "close")
                        .child("Close"),
                ),
        );
    let id = window.tree_mut().append_child(parent, element)?;
    window.show_modal(id)?;
    Ok(id)
}
fn close(window: &mut voidui::AppWindow, id: WidgetId) {
    window.close_top_layer(id);
    window.tree_mut().remove_subtree(id);
}
fn snapshot(window: &mut voidui::AppWindow, path: &Path) -> Result<()> {
    let (size, pixels) = window.snapshot()?;
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
    let smoke = match args.as_slice() {
        [] => None,
        [flag, path] if flag == "--smoke" => {
            std::fs::create_dir_all(path)?;
            Some(std::path::PathBuf::from(path))
        }
        _ => anyhow::bail!("usage: overlays [--smoke directory]"),
    };
    let fonts = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    fonts.add_fonts(vec![Cow::Borrowed(include_bytes!(
        "../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
    ))])?;
    let content=div().id("app").child(text("Stacking without components.").class("title"))
        .child(text("Standard CSS for appearance. Rust for modal/popover lifecycle. Tab stays inside the active modal.").class("subtitle"))
        .child(div().class("row")
            .child(div().class("trigger").id("trigger").child(div().tag("button").attr("data-action","open").child("Open modal / hover for tooltip"))
                .child(div().class("tooltip").id("tooltip").child("This tooltip uses only :hover, visibility, positioning and z-index.")))
            .child(div().tag("button").attr("data-action","popover").child("Manual popover")))
        .child(div().id("clip").child(div().id("popover").attr("popover","manual").child("Outside ancestor clipping. Press Escape to dismiss.")))
        .child(div().id("high").child("z-index: 2147483647 / still below the top layer"));
    let mut cursor = None;
    let mut counter = 0;
    let app = Application::new()
        .text_system(Arc::new(TextSystem::new(Arc::new(fonts))))
        .css(include_str!("styles/overlays.css"))?
        .window(
            WindowOptions {
                title: "voidui — stacking and top layer".into(),
                size: Size::new(900.0, 580.0),
                ..Default::default()
            },
            content,
        )
        .on_window_event(move |event, window| {
            match event {
                WindowEvent::CursorMoved { position, .. } => {
                    let scale = window.native_window().scale_factor();
                    cursor = Some(Point::new(
                        (position.x / scale) as f32,
                        (position.y / scale) as f32,
                    ));
                }
                WindowEvent::KeyboardInput { event, .. }
                    if event.state == ElementState::Pressed
                        && event.logical_key
                            == winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape) =>
                {
                    let top = window.tree().top_layer().next_back();
                    if let Some(id) = top {
                        if window.tree().top_layer_kind(id)
                            == Some(voidui::core::top_layer::TopLayerKind::Modal)
                        {
                            close(window, id);
                        } else {
                            window.close_top_layer(id);
                        }
                    }
                }
                WindowEvent::MouseInput {
                    state: ElementState::Released,
                    button: MouseButton::Left,
                    ..
                } => match cursor.and_then(|p| window.tree().hit_test(p)) {
                    Some(HitTarget::Backdrop(id)) => close(window, id),
                    Some(HitTarget::Element(id)) => {
                        let mut current = Some(id);
                        let mut action = None;
                        while let Some(id) = current {
                            if let Some(value) = window.tree().attribute(id, "data-action") {
                                action = Some(value.into_owned());
                                break;
                            }
                            current = window.tree().parent(id);
                        }
                        match action.as_deref() {
                            Some("open") => {
                                open(window, &mut counter)?;
                            }
                            Some("close") => {
                                if let Some(id) = window.tree().active_modal() {
                                    close(window, id);
                                }
                            }
                            Some("popover") => {
                                let id = window.tree().find_by_id("popover").unwrap();
                                if window.tree().is_top_layer(id) {
                                    window.close_top_layer(id);
                                } else {
                                    window.show_popover(id)?;
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
            Ok(())
        });
    let mut stage = 0;
    let mut first = None;
    let mut second = None;
    let mut number = 0;
    let mut idle = None;
    let mut wake = None;
    let app = if let Some(output) = smoke {
        app.on_frame(move|window| {
        match stage {
            0=>{snapshot(window,&output.join("base.png"))?;first=Some(open(window,&mut number)?);stage=1;}
            1=>{let id=first.unwrap();anyhow::ensure!(window.tree().active_modal()==Some(id),"first modal not active");snapshot(window,&output.join("first.png"))?;second=Some(open(window,&mut number)?);stage=2;}
            2=>{
                let id=second.unwrap();anyhow::ensure!(window.tree().active_modal()==Some(id),"nested modal not active");
                anyhow::ensure!(window.tree().top_layer().count()==2,"missing nested layer");
                anyhow::ensure!(window.tree().hit_test(Point::new(10.0,10.0))==Some(HitTarget::Backdrop(id)),"backdrop did not intercept input");
                snapshot(window,&output.join("nested.png"))?;close(window,id);stage=3;
            }
            3=>{let id=first.unwrap();anyhow::ensure!(window.tree().active_modal()==Some(id),"parent modal was not restored");close(window,id);window.tree_mut().pointer_moved(None);stage=4;}
            4 if window.tree().next_animation_frame(Instant::now()).is_none()=>{
                anyhow::ensure!(window.tree().top_layer().count()==0,"layer leak");idle=Some(window.stats());
                let (tx,rx)=std::sync::mpsc::channel();wake=Some(rx);let native=window.native_window().clone();
                std::thread::spawn(move||{std::thread::sleep(Duration::from_secs(2));let _=tx.send(());native.request_redraw();});stage=5;
            }
            5 if wake.as_ref().is_some_and(|rx|rx.try_recv().is_ok())=>{
                let before=idle.unwrap();let after=window.stats();anyhow::ensure!(after.presented_frames==before.presented_frames+1,"unsolicited idle frames");
                anyhow::ensure!(after.layout_passes==before.layout_passes&&after.scene_builds==before.scene_builds,"idle rebuild");
                println!("PASS nested modal, backdrop hit testing, parent restore, runtime removal, 2s idle: {after:?}");window.close();
            },_=>{}
        }Ok(())
    })
    } else {
        app
    };
    app.run()
}

/// Verify top-layer composition against ordinary maximum z-index using GPU pixels.
fn verify_pixels() -> Result<()> {
    let text = Arc::new(TextSystem::new(Arc::new(
        ParleyTextSystem::new_without_system_fonts("unused"),
    )));
    let mut stage = 0;
    Application::new().text_system(text).css("#page{width:220px;height:180px;background:white}#high{position:fixed;inset:0;z-index:2147483647;background:lime}dialog{position:fixed;margin:0;left:20px;top:20px;right:auto;bottom:auto;width:100px;height:80px;background:red}#inner{left:30px;top:30px;width:40px;height:20px;background:blue;z-index:-200}dialog::backdrop{background:rgb(0 0 0 / .5)}")?
        .window(WindowOptions {title:"voidui overlay pixels".into(),size:Size::new(220.0,180.0),..Default::default()},
            div().id("page").child(div().id("outer").tag("dialog").child(div().id("inner").tag("dialog"))).child(div().id("high")))
        .on_frame(move|window| {
            if stage==0 {window.show_modal(window.tree().find_by_id("outer").unwrap())?;window.show_modal(window.tree().find_by_id("inner").unwrap())?;stage=1;return Ok(());}
            let (size,pixels)=window.snapshot()?;let scale=size.width as f32/window.logical_size().width;
            for (x,y,expected) in [(5.0,5.0,[0u8,64,0]),(25.0,25.0,[128,0,0]),(35.0,35.0,[0,0,255])] {
                let i=((y*scale) as usize*size.width as usize+(x*scale) as usize)*4;let actual=&pixels[i..i+3];
                anyhow::ensure!(actual.iter().zip(expected).all(|(a,b)|a.abs_diff(b)<=1),"overlay pixel ({x},{y}): {actual:?} != {expected:?}");
            }
            println!("PASS GPU pixels: maximum z-index stays below both backdrops; negative-z nested modal stays above its parent");window.close();Ok(())
        }).run()
}
