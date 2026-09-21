//! CSS box shadows in logical pixels. The first shadow in a list is painted on top.
use super::{
    color::{Color, ColorSpace, HueDirection},
    list::StyleList,
};

pub type BoxShadows = StyleList<BoxShadow>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxShadow {
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur: f32,
    pub spread: f32,
    pub color: Color,
    pub inset: bool,
}
impl Default for BoxShadow {
    fn default() -> Self {
        Self {
            offset_x: 0.0,
            offset_y: 0.0,
            blur: 0.0,
            spread: 0.0,
            color: Color::CurrentColor,
            inset: false,
        }
    }
}
impl BoxShadow {
    /// CSS blur is twice the standard deviation used by the Gaussian shader.
    pub fn new(x: f32, y: f32, blur: f32, spread: f32, color: impl Into<Color>) -> Self {
        assert!(
            [x, y, blur, spread].iter().all(|v| v.is_finite()) && blur >= 0.0,
            "shadow lengths must be finite and blur must be nonnegative"
        );
        Self {
            offset_x: x,
            offset_y: y,
            blur,
            spread,
            color: color.into(),
            inset: false,
        }
    }
    pub fn inset(mut self) -> Self {
        self.inset = true;
        self
    }
}

/// Shadow lists interpolate pairwise, padding the shorter list with transparent
/// zero-sized shadows. A mismatched inset flag makes the entire value discrete.
pub(crate) fn interpolate(a: &[BoxShadow], b: &[BoxShadow], t: f32) -> Option<BoxShadows> {
    let mut result = Vec::with_capacity(a.len().max(b.len()));
    for i in 0..a.len().max(b.len()) {
        let transparent = |inset| BoxShadow {
            inset,
            color: Color::new(ColorSpace::Srgb, [0.0; 4]),
            ..BoxShadow::default()
        };
        let a = a.get(i).copied().unwrap_or_else(|| transparent(b[i].inset));
        let b = b.get(i).copied().unwrap_or_else(|| transparent(a.inset));
        if a.inset != b.inset {
            return None;
        }
        let mix = |a: f32, b: f32| a + (b - a) * t;
        result.push(BoxShadow {
            offset_x: mix(a.offset_x, b.offset_x),
            offset_y: mix(a.offset_y, b.offset_y),
            blur: mix(a.blur, b.blur).max(0.0),
            spread: mix(a.spread, b.spread),
            inset: a.inset,
            color: a
                .color
                .interpolate(b.color, t, ColorSpace::Oklab, HueDirection::Shorter),
        });
    }
    Some(result.into())
}
impl From<BoxShadow> for BoxShadows {
    fn from(value: BoxShadow) -> Self {
        vec![value].into()
    }
}

impl From<BoxShadow> for super::value::CssValue<BoxShadows> {
    fn from(value: BoxShadow) -> Self {
        Self::Value(value.into())
    }
}
