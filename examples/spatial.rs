//! Sticky positioning and CSS 2D transforms. Use --snapshot path.png for GPU QA.
use std::{borrow::Cow, sync::Arc};
use voidui::{
    core::geometry::Size,
    render::{ParleyTextSystem, TextSystem},
    *,
};
/// A small custom paint probe covers vector paths and underline primitives.
struct PathProbe;
impl core::widget::Widget for PathProbe {
    fn layout(
        &mut self,
        input: core::layout::LayoutInput,
        cx: core::context::LayoutContext<'_, '_>,
    ) -> core::layout::LayoutOutput {
        cx.layout_children(input)
    }
    fn draw(
        &self,
        p: &mut render::Painter<'_>,
        cx: core::context::DrawContext,
    ) -> anyhow::Result<()> {
        use render::{point, px};
        let x = cx.bounds.origin.x;
        let y = cx.bounds.origin.y;
        let mut path = render::Path::new(point(px(x), px(y)));
        path.line_to(point(px(x + 20.), px(y)));
        path.line_to(point(px(x), px(y + 30.)));
        p.paint_path(
            path,
            render::Hsla {
                h: 1. / 3.,
                s: 1.,
                l: 0.5,
                a: 1.,
            },
        );
        p.paint_underline(
            point(px(x), px(y + 35.)),
            px(20.),
            &render::UnderlineStyle {
                thickness: px(4.),
                color: Some(render::Hsla {
                    h: 1. / 6.,
                    s: 1.,
                    l: 0.5,
                    a: 1.,
                }),
                wavy: false,
            },
        );
        Ok(())
    }
}
fn path_probe() -> core::widget::WidgetBuilder<PathProbe> {
    core::widget::WidgetBuilder {
        widget: PathProbe,
        props: div().props,
        children: Vec::new(),
        events: Default::default(),
    }
}
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let snapshot = match args.as_slice() {
        [] => None,
        [flag, path] if flag == "--snapshot" => Some(path.clone()),
        _ => anyhow::bail!("usage: spatial [--snapshot path.png]"),
    };
    let fonts = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
    fonts.add_fonts(vec![Cow::Borrowed(include_bytes!(
        "../crates/voidui_gpui_wgpu/tests/fonts/IBMPlexSans-Regular.ttf"
    ))])?;
    let view=div().id("app")
  .child(text("Sticky + transforms").class("title"))
  .child(text("Scroll the left panel. Hover over the card on the right.").class("subtitle"))
  .child(div().class("row")
   .child(div().id("scroller").child(div().class("header").child("Sticky header"))
    .children((0..15).map(|i|div().class("item").child(format!("Scroll row {i:02}")))))
   .child(div().class("demo").child(div().id("card").child(text("Affine coordinates").font_size(22.)).child("Borders, gradients, text and hit testing share the same transform."))
    .child(div().id("clip").child(div().id("clipped").child("Clipped in local coordinates")))))
  .child(div().id("probe").child("GPU pixel probe"))
  .child(path_probe().id("path-probe"))
  .child(svg().id("svg-probe").view_box(0,0,20,30).child(svg::rect().width(20).height(30).fill("blue")))
  .child(div().id("clip-probe").child(div().id("clip-fill")));
    let css = include_str!("styles/spatial.css");
    let app = Application::new()
        .text_system(Arc::new(TextSystem::new(Arc::new(fonts))))
        .css(css)?
        .window(
            WindowOptions {
                title: "voidui — Sticky and transforms".into(),
                size: Size::new(740., 510.),
                ..Default::default()
            },
            view,
        );
    let mut stage = 0;
    let app = if let Some(path) = snapshot {
        app.on_frame(move |window| {
            if stage == 0 {
                let sc = window.tree().find_by_id("scroller").unwrap();
                window
                    .tree_mut()
                    .scroll_to(sc, core::geometry::Point::new(0., 120.));
                stage = 1;
                return Ok(());
            }
            let (dimensions, rgba) = window.snapshot()?;
            let scale = dimensions.width as f32 / window.logical_size().width;
            let pixel = |x: f32, y: f32| {
                let i =
                    (((y * scale) as usize) * dimensions.width as usize + (x * scale) as usize) * 4;
                &rgba[i..i + 4]
            };
            anyhow::ensure!(
                pixel(665., 460.)[0] > 240 && pixel(665., 460.)[1] < 15,
                "transformed quad failed GPU pixel check: {:?}",
                pixel(665., 460.)
            );
            anyhow::ensure!(pixel(590.,455.)[1]>240 && pixel(590.,455.)[0]<15,"transformed vector path failed: {:?}",pixel(590.,455.));
            anyhow::ensure!(pixel(563.,460.)[0]>240 && pixel(563.,460.)[1]>240,"transformed underline failed: {:?}",pixel(563.,460.));
            anyhow::ensure!(pixel(625.,460.)[2]>240 && pixel(625.,460.)[0]<15,"transformed image failed: {:?}",pixel(625.,460.));
            anyhow::ensure!(pixel(580.,424.)[1]>240,"rotated clipping lost interior");
            anyhow::ensure!(pixel(566.,411.)[1]<80,"rotated clipping used an axis-aligned bounding box");
            let file = std::fs::File::create(&path)?;
            let mut encoder = png::Encoder::new(file, dimensions.width, dimensions.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.write_header()?.write_image_data(&rgba)?;
            println!(
                "PASS: transformed quad/path/underline/image pixels, rotated clipping, sticky scroll; {:?}",
                window.stats()
            );
            window.close();
            Ok(())
        })
    } else {
        app
    };
    app.run()
}
