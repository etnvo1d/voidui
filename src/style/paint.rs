use crate::style::color::{Color, Rgba8};

/// Computed non-inherited paint properties. Keep currentColor unresolved here:
/// even when explicitly inherited, it uses the receiving element's own color.
#[derive(Debug, Clone, PartialEq)]
pub struct PaintStyle {
    pub background: Color,
    pub background_image: super::gradient::BackgroundImages,
    pub box_shadow: super::shadow::BoxShadows,
    pub border_color: Color,
    pub border_radius: f32,
}

impl Default for PaintStyle {
    fn default() -> Self {
        Self {
            background_image: Default::default(),
            box_shadow: Default::default(),
            background: Rgba8::new(0, 0, 0, 0).into(),
            border_color: Color::CurrentColor,
            border_radius: 0.0,
        }
    }
}
