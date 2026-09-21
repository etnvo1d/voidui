//! CSS gradient geometry and color-stop fixup. Gradients are resolved against the
//! painting box, never rasterized into a viewport-sized CPU image.
use super::{
    color::{Color, ColorSpace, HueDirection},
    list::StyleList,
};
use voidui_gpui_wgpu::{
    GradientGeometry, GradientPaint, GradientStop as PaintStop, point, px, size,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    Pixels(f32),
    Percent(f32),
}
impl Length {
    pub fn resolve(self, basis: f32) -> f32 {
        match self {
            Self::Pixels(v) => v,
            Self::Percent(v) => v * basis,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position {
    pub x: Length,
    pub y: Length,
}
impl Default for Position {
    fn default() -> Self {
        Self {
            x: Length::Percent(0.5),
            y: Length::Percent(0.5),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LinearDirection {
    Angle(f32),
    Corner(i8, i8),
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RadialExtent {
    ClosestSide,
    FarthestSide,
    ClosestCorner,
    FarthestCorner,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RadialSize {
    Extent(RadialExtent),
    Explicit(Length, Length),
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GradientKind {
    Linear(LinearDirection),
    Radial {
        circle: bool,
        extent: RadialSize,
        center: Position,
    },
    Conic {
        angle: f32,
        center: Position,
    },
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GradientItem {
    Stop {
        color: Color,
        position: Option<Length>,
    },
    Hint(Length),
}
#[derive(Debug, Clone, PartialEq)]
pub struct Gradient {
    pub kind: GradientKind,
    pub repeating: bool,
    pub space: ColorSpace,
    pub hue: HueDirection,
    pub stops: StyleList<GradientItem>,
}
#[derive(Debug, Clone, PartialEq)]
pub enum BackgroundImage {
    None,
    Gradient(Gradient),
}
pub type BackgroundImages = StyleList<BackgroundImage>;
impl From<Gradient> for BackgroundImage {
    fn from(value: Gradient) -> Self {
        Self::Gradient(value)
    }
}
impl From<Gradient> for BackgroundImages {
    fn from(value: Gradient) -> Self {
        vec![value.into()].into()
    }
}

impl std::str::FromStr for Gradient {
    type Err = String;
    fn from_str(raw: &str) -> Result<Self, String> {
        crate::style::css::gradient::parse(raw)
    }
}
impl Gradient {
    /// Build a linear gradient with CSS angles (0 points up, 90 points right).
    pub fn linear(angle: f32, stops: impl Into<StyleList<GradientItem>>) -> Self {
        Self {
            kind: GradientKind::Linear(LinearDirection::Angle(angle)),
            repeating: false,
            space: ColorSpace::Oklab,
            hue: HueDirection::Shorter,
            stops: stops.into(),
        }
    }
    pub fn color_space(mut self, space: ColorSpace) -> Self {
        self.space = space;
        self
    }
    pub fn hue_direction(mut self, hue: HueDirection) -> Self {
        self.hue = hue;
        self
    }
    pub fn repeating(mut self) -> Self {
        self.repeating = true;
        self
    }

    pub(crate) fn prepare(&self, width: f32, height: f32, current: Color) -> GradientPaint {
        let (geometry, basis) = self.geometry(width, height);
        let mut stops = Vec::<(f32, Color, Option<f32>)>::new();
        let mut pending_hint = None;
        for item in self.stops.iter() {
            match item {
                GradientItem::Hint(v) => {
                    pending_hint = Some(v.resolve(basis) / basis.max(f32::MIN_POSITIVE))
                }
                GradientItem::Stop { color, position } => {
                    if let Some(previous) = stops.last_mut() {
                        previous.2 = pending_hint.take();
                    }
                    stops.push((
                        position
                            .map(|p| p.resolve(basis) / basis.max(f32::MIN_POSITIVE))
                            .unwrap_or(f32::NAN),
                        color.resolve(current),
                        None,
                    ));
                }
            }
        }
        if stops.len() < 2 {
            return GradientPaint {
                geometry,
                repeating: self.repeating,
                stops: Vec::new(),
                tile: None,
            };
        }
        if stops[0].0.is_nan() {
            stops[0].0 = 0.0;
        }
        let last = stops.len() - 1;
        if stops[last].0.is_nan() {
            stops[last].0 = 1.0;
        }
        let mut previous = stops[0].0;
        for stop in &mut stops {
            if !stop.0.is_nan() {
                stop.0 = stop.0.max(previous);
                previous = stop.0;
            }
            if let Some(hint) = &mut stop.2 {
                *hint = hint.max(previous);
                previous = *hint;
            }
        }
        let mut begin = 0;
        for end in 1..stops.len() {
            if stops[end].0.is_nan() {
                continue;
            }
            for i in begin + 1..end {
                stops[i].0 = stops[begin].0
                    + (stops[end].0 - stops[begin].0) * (i - begin) as f32 / (end - begin) as f32;
            }
            begin = end;
        }
        // Coincident repeating stops have a defined average, not the last color.
        // Integrate premultiplied sRGB with equal stop spacing in the zero-period case.
        let degenerate_radial = matches!(geometry,GradientGeometry::Radial {radii,..} if f32::from(radii.width)<=0.0 || f32::from(radii.height)<=0.0);
        if self.repeating && (stops[last].0 <= stops[0].0 || degenerate_radial) {
            let mut average = [0.0; 4];
            for pair in stops.windows(2) {
                let span = stops[last].0 - stops[0].0;
                let weight = if span > 0.0 {
                    (pair[1].0 - pair[0].0) / (2.0 * span)
                } else {
                    1.0 / (2.0 * (stops.len() - 1) as f32)
                };
                for stop in pair {
                    let [r, g, b, a] = stop
                        .1
                        .components()
                        .to_alpha_color::<color::Srgb>()
                        .components;
                    for (sum, v) in average.iter_mut().zip([
                        r.clamp(0.0, 1.0) * a,
                        g.clamp(0.0, 1.0) * a,
                        b.clamp(0.0, 1.0) * a,
                        a,
                    ]) {
                        *sum += v * weight;
                    }
                }
            }
            return GradientPaint {
                geometry,
                repeating: false,
                tile: None,
                stops: vec![
                    PaintStop {
                        offset: 0.0,
                        color: average,
                    },
                    PaintStop {
                        offset: 1.0,
                        color: average,
                    },
                ],
            };
        }
        let mut ramp = Vec::new();
        for pair in stops.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let interp =
                a.1.components()
                    .interpolate(b.1.components(), self.space, self.hue);
            let hint =
                a.2.filter(|_| b.0 > a.0)
                    .map(|h| ((h - a.0) / (b.0 - a.0)).clamp(0.0, 1.0));
            let evaluate = |t: f32| {
                let t = match hint {
                    Some(0.0) => {
                        if t > 0.0 {
                            1.0
                        } else {
                            0.0
                        }
                    }
                    Some(1.0) => {
                        if t < 1.0 {
                            0.0
                        } else {
                            1.0
                        }
                    }
                    Some(h) => t.powf(0.5f32.ln() / h.ln()),
                    None => t,
                };
                let [r, g, b, alpha] = crate::style::color::map_to_srgb(interp.eval(t));
                [
                    r.clamp(0.0, 1.0) * alpha,
                    g.clamp(0.0, 1.0) * alpha,
                    b.clamp(0.0, 1.0) * alpha,
                    alpha,
                ]
            };
            let start = PaintStop {
                offset: a.0,
                color: evaluate(0.0),
            };
            let end = PaintStop {
                offset: b.0,
                color: evaluate(1.0),
            };
            ramp.push(start);
            if b.0 > a.0 {
                if hint == Some(0.0) {
                    ramp.push(PaintStop {
                        offset: a.0,
                        color: end.color,
                    });
                } else if hint == Some(1.0) {
                    ramp.push(PaintStop {
                        offset: b.0,
                        color: start.color,
                    });
                } else {
                    approximate(
                        &evaluate,
                        (a.0, b.0),
                        (0.0, start.color),
                        (1.0, end.color),
                        &mut ramp,
                        0,
                    );
                }
            }
            ramp.push(end);
        }
        GradientPaint {
            geometry,
            repeating: self.repeating,
            stops: ramp,
            tile: None,
        }
    }

    fn geometry(&self, w: f32, h: f32) -> (GradientGeometry, f32) {
        match self.kind {
            GradientKind::Linear(direction) => {
                let (dx, dy) = match direction {
                    LinearDirection::Angle(a) => (a.to_radians().sin(), -a.to_radians().cos()),
                    // Corner directions are perpendicular to the adjacent-corner
                    // diagonal; this preserves CSS's rectangular "magic corners".
                    LinearDirection::Corner(x, y) => {
                        let (dx, dy) = (x as f32 * h, y as f32 * w);
                        let length = dx.hypot(dy).max(f32::MIN_POSITIVE);
                        (dx / length, dy / length)
                    }
                };
                let length = w * dx.abs() + h * dy.abs();
                (
                    GradientGeometry::Linear {
                        center: point(px(w * 0.5), px(h * 0.5)),
                        direction: [dx, dy],
                        length,
                    },
                    length,
                )
            }
            GradientKind::Conic { angle, center } => (
                GradientGeometry::Conic {
                    center: point(px(center.x.resolve(w)), px(center.y.resolve(h))),
                    angle,
                },
                1.0,
            ),
            GradientKind::Radial {
                circle,
                extent,
                center,
            } => {
                let (x, y) = (center.x.resolve(w), center.y.resolve(h));
                let (rx, ry) = match extent {
                    RadialSize::Explicit(a, b) => (a.resolve(w), b.resolve(h)),
                    RadialSize::Extent(extent) => {
                        let near = matches!(
                            extent,
                            RadialExtent::ClosestSide | RadialExtent::ClosestCorner
                        );
                        let choose = |a: f32, b: f32| if near { a.min(b) } else { a.max(b) };
                        let (mut rx, mut ry) = (
                            choose(x.abs(), (w - x).abs()),
                            choose(y.abs(), (h - y).abs()),
                        );
                        if circle {
                            let r = choose(rx, ry);
                            rx = r;
                            ry = r;
                        }
                        if matches!(
                            extent,
                            RadialExtent::ClosestCorner | RadialExtent::FarthestCorner
                        ) {
                            let distance = |cx: f32, cy: f32| {
                                if circle {
                                    (x - cx).hypot(y - cy)
                                } else {
                                    ((x - cx) / rx.max(f32::MIN_POSITIVE))
                                        .hypot((y - cy) / ry.max(f32::MIN_POSITIVE))
                                }
                            };
                            let r = choose(
                                choose(distance(0.0, 0.0), distance(w, 0.0)),
                                choose(distance(0.0, h), distance(w, h)),
                            );
                            if circle {
                                rx = r;
                                ry = r;
                            } else if rx > 0.0 && ry > 0.0 {
                                rx *= r;
                                ry *= r;
                            }
                        }
                        (rx, ry)
                    }
                };
                (
                    GradientGeometry::Radial {
                        center: point(px(x), px(y)),
                        radii: size(px(rx), px(ry)),
                    },
                    rx,
                )
            }
        }
    }
}

/// Subdivide toward half an 8-bit output step of interpolation error in premultiplied
/// sRGB. Only the small color ramp is prepared on CPU; geometry is evaluated per
/// pixel on GPU. Hard stops remain duplicate positions, never blurred into a LUT.
fn approximate(
    evaluate: &impl Fn(f32) -> [f32; 4],
    interval: (f32, f32),
    a: (f32, [f32; 4]),
    b: (f32, [f32; 4]),
    out: &mut Vec<PaintStop>,
    depth: u32,
) {
    let mut error = 0.0f32;
    for f in [0.25, 0.5, 0.75] {
        let actual = evaluate(a.0 + (b.0 - a.0) * f);
        for (i, v) in actual.iter().enumerate() {
            error = error.max((v - (a.1[i] + (b.1[i] - a.1[i]) * f)).abs());
        }
    }
    // Float mantissa precision bounds recursion for pathological color functions.
    if error <= 0.5 / 255.0 || depth >= f32::MANTISSA_DIGITS {
        return;
    }
    let t = (a.0 + b.0) * 0.5;
    let middle = (t, evaluate(t));
    approximate(evaluate, interval, a, middle, out, depth + 1);
    out.push(PaintStop {
        offset: interval.0 + (interval.1 - interval.0) * t,
        color: middle.1,
    });
    approximate(evaluate, interval, middle, b, out, depth + 1);
}

impl From<Gradient> for super::value::CssValue<BackgroundImages> {
    fn from(value: Gradient) -> Self {
        Self::Value(value.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn prepare(raw: &str) -> GradientPaint {
        raw.parse::<Gradient>()
            .unwrap()
            .prepare(200.0, 100.0, "black".parse().unwrap())
    }
    fn sample(g: &GradientPaint, t: f32) -> [f32; 4] {
        let hi = g.stops.partition_point(|s| s.offset <= t);
        let a = g.stops[hi.saturating_sub(1)];
        let b = g.stops[hi.min(g.stops.len() - 1)];
        let f = if b.offset > a.offset {
            ((t - a.offset) / (b.offset - a.offset)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        std::array::from_fn(|i| a.color[i] + (b.color[i] - a.color[i]) * f)
    }
    fn near(a: f32, b: f32) {
        assert!((a - b).abs() < 0.003, "{a} != {b}");
    }
    #[test]
    fn angles_and_corner_geometry_follow_css() {
        let g = prepare("linear-gradient(45deg,red,blue)");
        if let GradientGeometry::Linear {
            direction, length, ..
        } = g.geometry
        {
            near(direction[0], 0.5f32.sqrt());
            near(length, 300.0 * 0.5f32.sqrt());
        } else {
            panic!()
        }
        let g = prepare("linear-gradient(to top right,red,blue)");
        if let GradientGeometry::Linear { direction, .. } = g.geometry {
            near(direction[0] / -direction[1], 0.5);
        } else {
            panic!()
        }
    }
    #[test]
    fn alpha_is_premultiplied_and_hard_stops_are_not_smoothed() {
        let g = prepare("linear-gradient(in srgb,transparent,red)");
        near(sample(&g, 0.5)[0], 0.5);
        near(sample(&g, 0.5)[3], 0.5);
        let g = prepare("linear-gradient(in srgb,red 0% 50%,blue 50% 100%)");
        near(sample(&g, 0.499)[0], 1.0);
        near(sample(&g, 0.5)[2], 1.0);
    }
    #[test]
    fn hints_and_omitted_stop_positions_are_fixed_up() {
        let g = prepare("linear-gradient(in srgb,red,25%,blue)");
        let mid = sample(&g, 0.25);
        near(mid[0], 0.5);
        near(mid[2], 0.5);
        let g = prepare("linear-gradient(in srgb,red 20%,green,blue 80%)");
        near(sample(&g, 0.5)[1], 128.0 / 255.0);
        let g = prepare("linear-gradient(in srgb,red 80%,20%,blue 30%)");
        near(sample(&g, 0.79)[0], 1.0);
        near(sample(&g, 0.8)[2], 1.0);
    }
    #[test]
    fn coincident_repeating_stops_use_the_css_average() {
        let g = prepare("repeating-linear-gradient(in srgb,red 0px,white 0px,blue 0px)");
        assert!(!g.repeating);
        let c = sample(&g, 0.5);
        near(c[0], 0.75);
        near(c[1], 0.5);
        near(c[2], 0.75);
    }
    #[test]
    fn color_ramp_tracks_color4_interpolation_with_output_precision() {
        for space in [
            "srgb",
            "srgb-linear",
            "oklab",
            "oklch longer hue",
            "lab",
            "lch",
            "display-p3",
            "hsl increasing hue",
        ] {
            let source = format!(
                "linear-gradient(in {space},oklch(70% .18 20 / .3),color(display-p3 .1 .8 .6))"
            );
            let gradient = source.parse::<Gradient>().unwrap();
            let g = prepare(&source);
            let a = "oklch(70% .18 20 / .3)"
                .parse::<Color>()
                .unwrap()
                .components();
            let b = "color(display-p3 .1 .8 .6)"
                .parse::<Color>()
                .unwrap()
                .components();
            let interp = a.interpolate(b, gradient.space, gradient.hue);
            for i in 0..=1000 {
                let t = i as f32 / 1000.0;
                let [r, gc, b, a] = crate::style::color::map_to_srgb(interp.eval(t));
                let actual = sample(&g, t);
                let expected = [
                    r.clamp(0.0, 1.0) * a,
                    gc.clamp(0.0, 1.0) * a,
                    b.clamp(0.0, 1.0) * a,
                    a,
                ];
                for i in 0..4 {
                    assert!(
                        (actual[i] - expected[i]).abs() < 1.0 / 255.0,
                        "{space} t={t} {actual:?} != {expected:?}"
                    );
                }
            }
        }
    }
}
