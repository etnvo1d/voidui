//! Shared replaced-content sizing and paint geometry for img and SVG viewports.
use crate::{
    core::layout::{self, LayoutInput, LayoutOutput},
    render::{self, Painter, Result},
};

/// Resolve the replaced object's preferred content size before Taffy's generic
/// leaf algorithm. Taffy clamps axes independently and applies a height floor
/// for aspect-ratio; images need coupled min/max constraints on auto dimensions.
/// The box engine still owns padding, borders, positioning and final layout.
pub(crate) fn replaced_layout(
    style: &layout::LayoutStyle,
    input: LayoutInput,
    intrinsic: [f32; 2],
) -> LayoutOutput {
    use taffy::util::{MaybeResolve, ResolveOrZero};
    let inset = (style
        .padding
        .resolve_or_zero(input.parent_size.width, crate::core::layout::resolve_calc)
        + style
            .border
            .resolve_or_zero(input.parent_size.width, crate::core::layout::resolve_calc))
    .sum_axes();
    let ratio = style.aspect_ratio.unwrap_or(intrinsic[0] / intrinsic[1]);
    let mut preferred = layout::Size::NONE;
    let mut minimum = layout::Size::NONE;
    let mut maximum = layout::Size::NONE;
    if input.sizing_mode == layout::SizingMode::InherentSize {
        preferred = style
            .size
            .maybe_resolve(input.parent_size, crate::core::layout::resolve_calc);
        minimum = style
            .min_size
            .maybe_resolve(input.parent_size, crate::core::layout::resolve_calc);
        maximum = style
            .max_size
            .maybe_resolve(input.parent_size, crate::core::layout::resolve_calc);
        if style.box_sizing == layout::BoxSizing::BorderBox {
            for value in [&mut preferred, &mut minimum, &mut maximum] {
                value.width = value.width.map(|v| (v - inset.width).max(0.));
                value.height = value.height.map(|v| (v - inset.height).max(0.));
            }
        }
    }
    let w = input
        .known_dimensions
        .width
        .map(|v| (v - inset.width).max(0.))
        .or(preferred.width);
    let h = input
        .known_dimensions
        .height
        .map(|v| (v - inset.height).max(0.))
        .or(preferred.height);
    let min = [minimum.width.unwrap_or(0.), minimum.height.unwrap_or(0.)];
    let max = [
        maximum.width.unwrap_or(f32::INFINITY).max(min[0]),
        maximum.height.unwrap_or(f32::INFINITY).max(min[1]),
    ];
    let clamp = |v: f32, i: usize| v.max(min[i]).min(max[i]);
    let [width, height] = match (w, h) {
        (Some(w), Some(h)) => [clamp(w, 0), clamp(h, 1)],
        (Some(w), None) => {
            let w = clamp(w, 0);
            [w, clamp(w / ratio, 1)]
        }
        (None, Some(h)) => {
            let h = clamp(h, 1);
            [clamp(h * ratio, 0), h]
        }
        (None, None) => {
            let [w, h] = [intrinsic[0], intrinsic[0] / ratio];
            let lower = (min[0] / w).max(min[1] / h);
            let upper = (max[0] / w).min(max[1] / h);
            if lower <= upper {
                let scale = 1f32.max(lower).min(upper);
                [w * scale, h * scale]
            } else {
                [clamp(w, 0), clamp(h, 1)]
            }
        }
    };
    let mut resolved = style.clone();
    resolved.aspect_ratio = None;
    let border_box = style.box_sizing == layout::BoxSizing::BorderBox;
    resolved.size = layout::Size {
        width: layout::length(width + if border_box { inset.width } else { 0. }),
        height: layout::length(height + if border_box { inset.height } else { 0. }),
    };
    layout::layout_leaf(&resolved, input, |_, _| layout::Size { width, height })
}
pub(crate) fn bounds_from_rect(
    b: crate::core::geometry::Rect<f32>,
) -> render::Bounds<render::Pixels> {
    render::Bounds::new(
        render::point(render::px(b.origin.x), render::px(b.origin.y)),
        render::size(render::px(b.size.width), render::px(b.size.height)),
    )
}
pub(crate) fn physical_size(w: f32, h: f32, scale: f32) -> Result<(u32, u32)> {
    anyhow::ensure!(
        w.is_finite() && h.is_finite() && w > 0. && h > 0.,
        "invalid image viewport"
    );
    Ok((
        (w * scale).ceil().max(1.) as u32,
        (h * scale).ceil().max(1.) as u32,
    ))
}
pub(crate) fn visible(p: &Painter<'_>, b: render::Bounds<render::Pixels>) -> bool {
    !b.intersect(&p.content_mask().bounds).is_empty()
}
