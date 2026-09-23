//! CPU scene verification for box painting; no GPU or font installation is needed.
use std::{borrow::Cow, sync::Arc};
use voidui::{
    core::{
        layout::{self, Overflow, Size, TaffyMaxContent},
        widget_tree::WidgetTree,
    },
    div,
    render::{
        self, AtlasKey, AtlasTile, DevicePixels, Painter, ParleyTextSystem, PlatformAtlas, Result,
        Scene, TextLayoutCache, TextSystem, px, size,
    },
    style::color::Rgba8,
};

struct NoGlyphs;
impl PlatformAtlas for NoGlyphs {
    fn get_or_insert_with<'a>(
        &self,
        _: &AtlasKey,
        _: &mut dyn FnMut() -> Result<Option<(render::Size<DevicePixels>, Cow<'a, [u8]>)>>,
    ) -> Result<Option<AtlasTile>> {
        panic!("box painting must not rasterize glyphs");
    }
    fn remove(&self, _: &AtlasKey) {}
}

fn paint(element: impl voidui::core::element::IntoElement, scale: f32) -> Scene {
    let system = Arc::new(TextSystem::new(Arc::new(
        ParleyTextSystem::new_without_system_fonts("unused"),
    )));
    let mut tree = WidgetTree::new();
    tree.build_root(element);
    tree.layout(Size::MAX_CONTENT, &TextLayoutCache::new(system.clone()));
    let mut scene = Scene::default();
    let mut painter = Painter::new(
        &mut scene,
        &NoGlyphs,
        system,
        size(px(400.), px(400.)),
        scale,
    )
    .unwrap();
    tree.draw(&mut painter).unwrap();
    drop(painter);
    scene.finish();
    scene
}

#[test]
fn transparent_boxes_do_not_allocate_quads() {
    let scene = paint(div().width(100).height(100).child(div()), 1.0);
    assert!(scene.quads.is_empty());
}

#[test]
fn background_border_radius_and_dpi_are_applied_once() {
    let scene = paint(
        div()
            .width(100)
            .height(40)
            .border_radius(100.0)
            .background(Rgba8::from_rgb8(20, 40, 60))
            .border_color(Rgba8::from_rgb8(80, 90, 100))
            .box_sizing(layout::BoxSizing::BorderBox)
            .border_width(2.0),
        2.0,
    );
    assert_eq!(scene.quads.len(), 1);
    let quad = &scene.quads[0];
    assert_eq!(quad.bounds.size.width.0, 200.0);
    assert_eq!(quad.border_widths.left.0, 4.0);
    assert_eq!(quad.corner_radii.top_left.0, 40.0);
    assert_eq!(
        quad.border_color,
        render::Hsla::from(voidui::style::color::Color::from(Rgba8::from_rgb8(
            80, 90, 100
        )))
    );
}

#[test]
fn default_border_color_inherits_foreground() {
    let color = Rgba8::from_rgb8(50, 100, 150);
    let scene = paint(
        div()
            .color(color)
            .child(div().width(30).height(20).border_width(1.0)),
        1.0,
    );
    assert_eq!(scene.quads.len(), 1);
    assert_eq!(
        scene.quads[0].border_color,
        render::Hsla::from(voidui::style::color::Color::from(color))
    );
}

#[test]
fn hidden_ancestors_suppress_backgrounds_and_descendants() {
    let scene = paint(
        div()
            .display(layout::Display::None)
            .background(Rgba8::from_rgb8(255, 0, 0))
            .child(
                div()
                    .width(100)
                    .height(100)
                    .background(Rgba8::from_rgb8(0, 255, 0)),
            ),
        1.0,
    );
    assert!(scene.quads.is_empty());
}

#[test]
fn descendants_are_painted_after_parent_backgrounds() {
    let scene = paint(
        div()
            .width(100)
            .height(100)
            .background(Rgba8::from_rgb8(255, 0, 0))
            .child(
                div()
                    .width(50)
                    .height(50)
                    .background(Rgba8::from_rgb8(0, 255, 0)),
            ),
        1.0,
    );
    assert_eq!(scene.quads.len(), 2);
    assert!(scene.quads[0].order <= scene.quads[1].order);
    assert_eq!(scene.quads[0].bounds.size.width.0, 100.0);
    assert_eq!(scene.quads[1].bounds.size.width.0, 50.0);
}

#[test]
fn overflow_clips_only_requested_axes_and_restores_sibling_mask() {
    let scene = paint(
        div()
            .child(
                div()
                    .width(100)
                    .height(30)
                    .border_width(2.0)
                    .overflow_x(Overflow::Clip)
                    .child(
                        div()
                            .width(200)
                            .height(100)
                            .background(Rgba8::from_rgb8(255, 0, 0)),
                    ),
            )
            .child(
                div()
                    .width(200)
                    .height(10)
                    .background(Rgba8::from_rgb8(0, 255, 0)),
            ),
        1.0,
    );
    // The parent border, clipped child, and unclipped sibling each form a quad.
    assert_eq!(scene.quads.len(), 3);
    let child = &scene.quads[1];
    assert_eq!(child.content_mask.bounds.origin.x.0, 2.0);
    assert_eq!(child.content_mask.bounds.size.width.0, 100.0);
    assert_eq!(child.content_mask.bounds.size.height.0, 400.0);
    assert_eq!(scene.quads[2].content_mask.bounds.size.width.0, 400.0);
}

#[test]
fn rounded_overflow_attaches_shared_spatial_clips_only_to_its_subtree() {
    let scene = paint(
        div()
            .child(
                div()
                    .size(100, 100)
                    .border_width(4)
                    .border_radius(24.)
                    .overflow(voidui::Overflow::Hidden)
                    .child(div().size(100, 50).background(Rgba8::from_rgb8(255, 0, 0)))
                    .child(div().size(100, 50).background(Rgba8::from_rgb8(0, 255, 0))),
            )
            .child(div().size(100, 20).background(Rgba8::from_rgb8(0, 0, 255))),
        2.0,
    );
    assert_eq!(scene.quads.len(), 4);
    // Scene batching can reorder non-overlapping quads; identify the boxes by
    // geometry rather than depending on their submission order.
    let parent = scene
        .quads
        .iter()
        .find(|q| q.border_widths.left.0 > 0.)
        .unwrap()
        .spatial_id;
    let children: Vec<_> = scene
        .quads
        .iter()
        .filter(|q| q.bounds.size.height.0 == 100.)
        .collect();
    let child = children[0].spatial_id;
    assert_ne!(child, 0);
    assert_ne!(
        child, parent,
        "the parent's border must not receive its own content clip"
    );
    assert_eq!(children.len(), 2);
    assert_eq!(children[1].spatial_id, child);
    assert_eq!(
        scene
            .quads
            .iter()
            .find(|q| q.bounds.size.height.0 == 40.)
            .unwrap()
            .spatial_id,
        0
    );
}

#[test]
fn inherited_currentcolor_is_resolved_by_the_receiving_element() {
    use voidui::style::{CssValue, color::Color};
    let foreground = Rgba8::from_rgb8(20, 80, 140);
    let scene = paint(
        div()
            .color(Rgba8::from_rgb8(200, 30, 30))
            .background(Color::CurrentColor)
            .child(
                div()
                    .size(60, 20)
                    .color(foreground)
                    .background(CssValue::Inherit)
                    .border_width(1)
                    .border_color(Color::CurrentColor),
            ),
        1.0,
    );
    let expected: render::Hsla = Color::from(foreground).into();
    assert_eq!(scene.quads.len(), 2);
    assert_eq!(scene.quads[1].border_color, expected);
    assert_eq!(
        scene.quads[1].background,
        render::Background::from(expected)
    );
}

#[test]
fn inherited_overflow_clip_reaches_painting_not_only_layout() {
    use voidui::style::LayoutProperty;
    let scene = paint(
        div().size(200, 100).overflow(Overflow::Clip).child(
            div()
                .size(50, 30)
                .inherit(LayoutProperty::OverflowX)
                .child(div().size(100, 20).background(Rgba8::from_rgb8(255, 0, 0))),
        ),
        1.0,
    );
    assert_eq!(scene.quads.len(), 1);
    assert_eq!(scene.quads[0].content_mask.bounds.size.width.0, 50.0);
}

#[test]
fn shadow_geometry_spread_sigma_and_inset_padding_clip_scale_once() {
    use voidui::style::shadow::BoxShadow;
    let scene = paint(
        div()
            .width(100)
            .height(60)
            .box_sizing(layout::BoxSizing::BorderBox)
            .border_width(4)
            .border_radius(12.0)
            .box_shadow([
                BoxShadow::new(5.0, 7.0, 8.0, 3.0, Rgba8::from_rgb8(0, 0, 0)),
                BoxShadow::new(2.0, 3.0, 6.0, 1.0, Rgba8::from_rgb8(0, 0, 255)).inset(),
            ]),
        2.0,
    );
    assert_eq!(scene.shadows.len(), 2);
    let outer = scene.shadows.iter().find(|s| s.inset == 0).unwrap();
    assert_eq!(outer.blur_radius.0, 8.0);
    assert_eq!(outer.bounds.origin.x.0, 4.0);
    assert_eq!(outer.bounds.size.width.0, 212.0);
    assert_eq!(outer.element_bounds.size.width.0, 200.0);
    let inner = scene.shadows.iter().find(|s| s.inset == 1).unwrap();
    assert_eq!(inner.blur_radius.0, 6.0);
    assert_eq!(inner.element_bounds.origin.x.0, 8.0);
    assert_eq!(inner.element_bounds.size.width.0, 184.0);
}
#[test]
fn gradient_ramp_is_variable_length_and_scene_replay_remaps_it() {
    use voidui::style::gradient::Gradient;
    let gradient:Gradient="linear-gradient(to right in srgb,red 0% 20%,blue 20% 40%,green 40% 60%,yellow 60% 80%,white 80% 100%)".parse().unwrap();
    let scene = paint(div().width(100).height(100).background_image(gradient), 1.0);
    assert_eq!(scene.quads.len(), 1);
    assert!(scene.gradient_data.len() > 4 + 2 * 2);
    let mut replay = Scene::default();
    replay.replay(0..scene.len(), &scene);
    replay.replay(0..scene.len(), &scene);
    assert_eq!(replay.quads.len(), 2);
    assert_eq!(replay.gradient_data.len(), scene.gradient_data.len() * 2);
    replay.clear();
    assert!(replay.gradient_data.is_empty());
}
