//! Portable glass demo. `--snapshot path.png` captures one frame and exits.
use anyhow::Result;
use voidui::{
    Application, GlassMaterial, WindowOptions, core::geometry::Size, div, glass_group, text,
};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let output = match args.as_slice() {
        [] => None,
        [flag, path] if flag == "--snapshot" || flag == "--smoke" => Some(path.clone()),
        _ => anyhow::bail!("usage: liquid_glass [--snapshot path.png | --smoke directory]"),
    };
    let content = div().id("scene")
        .child(div().id("orb-a"))
        .child(div().id("orb-b"))
        .child(div().id("ribbon"))
        .child(div().id("grid"))
        .child(div().id("header")
            .child(text("VOID / MATERIAL STUDIES").class("eyebrow"))
            .child(text("Light, shaped.").class("title"))
            .child(text("A portable optical surface. Move over the cards to change the view.").class("subtitle")))
        .child(div().id("regular").class("glass")
            .child(text("01 / REGULAR").class("eyebrow"))
            .child(text("A little atmosphere.").class("heading"))
            .child(text("The background softens through the face, then bends along the curved edge.").class("body"))
            .child(div().class("separator"))
            .child(text("SDF REFRACTION   /   BACKDROP BLUR").class("caption")))
        .child(div().id("clear").class("glass")
            .child(text("02 / CLEAR").class("eyebrow"))
            .child(text("Keep the color.").class("heading"))
            .child(text("Less diffusion. The same rounded optics, with a sharper view beneath.").class("body"))
            .child(div().class("separator"))
            .child(text("LIVE BACKGROUND   /   SHARP CONTENT").class("caption")))
        .child(div().id("toolbar").class("glass")
            .child(text("WGPU").class("badge"))
            .child(text("Metal / Vulkan / Direct3D 12").class("toolbar-label"))
            .child(text("Draws only when content changes").class("toolbar-note")))
        .child(glass_group(GlassMaterial::clear(), 48.).id("liquid-group")
            .child(div().class("liquid-button").child("A"))
            .child(div().class("liquid-button moving").child("B")))
        .child(text("Move over A / B to separate the shared surface").id("merge-label"))
        .child(text("VOIDUI     •     GLASS / 001").id("footer"));
    let app = Application::new()
        .css(include_str!("styles/liquid_glass.css"))?
        .window(
            WindowOptions {
                title: "VoidUI — Liquid Glass".into(),
                size: Size::new(1040., 900.),
                ..Default::default()
            },
            content,
        );
    let smoke = args.first().is_some_and(|a| a == "--smoke");
    let app = if let Some(output) = output {
        let mut stage = 0;
        let mut frames = 0;
        let mut baseline = None;
        if smoke {
            std::fs::create_dir_all(&output)?;
        }
        app.on_frame(move |window| {
            frames += 1;
            if smoke
                && stage > 0
                && window
                    .tree()
                    .next_animation_frame(std::time::Instant::now())
                    .is_some()
            {
                return Ok(());
            }
            let name = match stage {
                0 => "merged.png",
                1 => "separated.png",
                _ => "rejoined.png",
            };
            let path = if smoke {
                std::path::Path::new(&output).join(name)
            } else {
                output.clone().into()
            };
            snapshot(window, &path)?;
            if !smoke {
                window.close();
                return Ok(());
            }
            let root = window.tree().root().unwrap();
            let group = window.tree().children(root)[window.tree().children(root).len() - 3];
            if stage == 0 {
                baseline = Some(window.stats());
                window.tree_mut().set_classes(group, "separated");
            } else if stage == 1 {
                window.tree_mut().set_classes(group, "");
            } else {
                let before = baseline.unwrap();
                let after = window.stats();
                anyhow::ensure!(frames > 8, "glass translation did not animate");
                anyhow::ensure!(
                    after.layout_passes == before.layout_passes,
                    "glass translation caused layout: {} -> {}",
                    before.layout_passes,
                    after.layout_passes
                );
                anyhow::ensure!(
                    after.failed_frames == before.failed_frames,
                    "glass frame failed"
                );
                println!(
                    "PASS separation/reunion: frames={frames}, captures={}, layout_passes={}",
                    after.renderer.glass_captures - before.renderer.glass_captures,
                    after.layout_passes - before.layout_passes
                );
                window.close();
            }
            stage += 1;
            Ok(())
        })
    } else {
        app
    };

    app.run()
}

fn snapshot(window: &mut voidui::AppWindow, path: &std::path::Path) -> Result<()> {
    let (size, pixels) = window.snapshot()?;
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, size.width, size.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&pixels)?;
    Ok(())
}
