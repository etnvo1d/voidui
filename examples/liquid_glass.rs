use voidui::{Application, MouseEvent, WindowOptions, component, div, glass_group, state};
use voidui_gpui_wgpu::*;

#[component]
fn root() {
    let x = state(|| 0.0);
    let y = state(|| 0.0);
    let r = state(|| 128.0);
    div()
        .child("The quick brown fox jumps over the lazy dog. ".repeat(100))
        .child(
            glass_group(GlassMaterial::unblurred(), 32.0)
                .w_full()
                .h_full()
                .absolute()
                .left(0.)
                .top(0.)
                .z_10()
                .child(
                    div()
                        .width(r.get())
                        .height(r.get())
                        .border_radius(r.get() / 2.0)
                        .absolute()
                        .left(x.get())
                        .top(y.get()),
                )
                .child(
                    div()
                        .width(64.0)
                        .height(64.0)
                        .border_radius(8.0)
                        .absolute()
                        .left(0)
                        .bottom(0),
                ),
        )
        .on_mouse_move(move |e: MouseEvent| {
            x.set(e.position.x - r.get() / 2.0);
            y.set(e.position.y - r.get() / 2.0);
        })
        .on_mouse_scroll(move |e: MouseEvent| {
            r.update(|r| *r -= e.wheel.y);
        })
}

fn main() -> anyhow::Result<()> {
    Application::new()
        .window(
            WindowOptions {
                title: "liquid glass".into(),
                ..Default::default()
            },
            root(),
        )
        .run()
}
