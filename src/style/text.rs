use crate::{
    core::layout::Direction,
    style::{
        color::{Color, Rgba8},
        value::CssValue,
    },
};
use voidui_gpui_wgpu::{Font, TextAlign};

/// Font size: em and percentages resolve against the parent's computed size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FontSize {
    Pixels(f32),
    Em(f32),
    /// A fraction: 1.5 means 150%.
    Percent(f32),
}

impl FontSize {
    pub fn resolve(self, parent_size: f32) -> f32 {
        match self {
            Self::Pixels(value) => value,
            Self::Em(value) | Self::Percent(value) => value * parent_size,
        }
    }
}

/// Unitless line height remains a multiplier when inherited; percentages/em
/// become pixels on the declaring element before passing down the tree.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum LineHeight {
    #[default]
    Normal,
    Pixels(f32),
    Relative(f32),
    Percent(f32),
    Em(f32),
}

impl LineHeight {
    pub fn resolve(self, font_size: f32) -> f32 {
        match self {
            // CSS lets the user agent choose normal line spacing. Keep the UI's
            // existing 1.2 policy centralized instead of baking it into widgets.
            Self::Normal => 1.2 * font_size,
            Self::Pixels(value) => value,
            Self::Relative(value) | Self::Percent(value) | Self::Em(value) => value * font_size,
        }
    }

    pub(crate) fn computed(self, font_size: f32) -> Self {
        match self {
            Self::Percent(_) | Self::Em(_) => Self::Pixels(self.resolve(font_size)),
            value => value,
        }
    }
}

/// Logical text alignment retains start/end through inheritance, then resolves
/// against each element's own direction immediately before painting.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum TextAlignment {
    #[default]
    Start,
    End,
    Left,
    Center,
    Right,
}

impl TextAlignment {
    pub fn resolve(self, direction: Direction) -> TextAlign {
        match self {
            Self::Start if direction == Direction::Rtl => TextAlign::Right,
            Self::End if direction == Direction::Ltr => TextAlign::Right,
            Self::Start | Self::End | Self::Left => TextAlign::Left,
            Self::Center => TextAlign::Center,
            Self::Right => TextAlign::Right,
        }
    }
}

impl From<TextAlign> for CssValue<TextAlignment> {
    fn from(value: TextAlign) -> Self {
        Self::Value(match value {
            TextAlign::Left => TextAlignment::Left,
            TextAlign::Center => TextAlignment::Center,
            TextAlign::Right => TextAlignment::Right,
        })
    }
}

macro_rules! numeric_text_values {
    ($($ty:ty),*) => { $(
        impl From<$ty> for FontSize { fn from(value: $ty) -> Self { Self::Pixels(value as f32) } }
        impl From<$ty> for LineHeight { fn from(value: $ty) -> Self { Self::Pixels(value as f32) } }
        impl From<$ty> for CssValue<FontSize> { fn from(value: $ty) -> Self { Self::Value(FontSize::from(value)) } }
        impl From<$ty> for CssValue<LineHeight> { fn from(value: $ty) -> Self { Self::Value(LineHeight::from(value)) } }
    )* };
}
numeric_text_values!(i32, u32, f32, f64);

/// Computed inherited properties. Relative font sizes have already become pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    pub color: Color,
    /// The auto policy follows currentColor without changing text shaping.
    pub caret_color: Color,
    pub caret_animation: bool,
    pub font: Font,
    pub font_size: f32,
    pub line_height: LineHeight,
    pub align: TextAlignment,
    pub direction: Direction,
    pub wrap: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            color: Rgba8::from_rgb8(0, 0, 0).into(),
            caret_color: Color::CurrentColor,
            caret_animation: true,
            font: Font::default(),
            font_size: 16.0,
            line_height: LineHeight::Normal,
            align: TextAlignment::Start,
            direction: Direction::Ltr,
            wrap: true,
        }
    }
}

macro_rules! numeric_font_weight {
    ($($ty:ty),*) => { $(
        impl From<$ty> for CssValue<voidui_gpui_wgpu::FontWeight> {
            fn from(value: $ty) -> Self { Self::Value(voidui_gpui_wgpu::FontWeight(value as f32)) }
        }
    )* };
}
numeric_font_weight!(i32, u32, f32, f64);
