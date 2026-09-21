#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba8 {
    pub fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub fn from_rgb8(r: u8, g: u8, b: u8) -> Self {
        Self::new(r, g, b, 255)
    }

    pub fn from_rgb_f32(r: f32, g: f32, b: f32) -> Self {
        let clamp = |v: f32| v.clamp(0.0, 1.0);
        Self::new(
            (clamp(r) * 255.0) as u8,
            (clamp(g) * 255.0) as u8,
            (clamp(b) * 255.0) as u8,
            255,
        )
    }

    pub fn from_rgba_f32(r: f32, g: f32, b: f32, a: f32) -> Self {
        let clamp = |v: f32| v.clamp(0.0, 1.0);
        Self::new(
            (clamp(r) * 255.0) as u8,
            (clamp(g) * 255.0) as u8,
            (clamp(b) * 255.0) as u8,
            (clamp(a) * 255.0) as u8,
        )
    }

    pub fn from_hex_rgb(hex: u32) -> Self {
        Self::new(
            ((hex >> 16) & 0xFF) as u8,
            ((hex >> 8) & 0xFF) as u8,
            (hex & 0xFF) as u8,
            255,
        )
    }

    pub fn from_hex_rgba(hex: u32) -> Self {
        Self::new(
            ((hex >> 24) & 0xFF) as u8,
            ((hex >> 16) & 0xFF) as u8,
            ((hex >> 8) & 0xFF) as u8,
            (hex & 0xFF) as u8,
        )
    }
}

/// CSS colors retain floating-point components and their original color space.
/// Conversion to the window's sRGB output happens only at the paint boundary.
#[derive(Debug, Clone, Copy)]
pub enum Color {
    /// Resolve against this element's computed foreground color.
    CurrentColor,
    Rgba8(Rgba8),
    Components(color::DynamicColor),
}

pub use color::{ColorSpaceTag as ColorSpace, HueDirection};

impl PartialEq for Color {
    fn eq(&self, other: &Self) -> bool {
        match (*self, *other) {
            (Self::CurrentColor, Self::CurrentColor) => true,
            (Self::CurrentColor, _) | (_, Self::CurrentColor) => false,
            (Self::Rgba8(a), Self::Rgba8(b)) => a == b,
            (a, b) => {
                let (a, b) = (a.components(), b.components());
                if a.cs == b.cs || !a.flags.missing().is_empty() || !b.flags.missing().is_empty() {
                    a.cs == b.cs
                        && a.flags.missing() == b.flags.missing()
                        && a.components == b.components
                } else {
                    a.to_alpha_color::<color::Srgb>().components
                        == b.to_alpha_color::<color::Srgb>().components
                }
            }
        }
    }
}

impl std::str::FromStr for Color {
    type Err = String;
    fn from_str(source: &str) -> Result<Self, Self::Err> {
        if source.trim().eq_ignore_ascii_case("currentcolor") {
            return Ok(Self::CurrentColor);
        }
        color::parse_color(source.trim())
            .map(Self::Components)
            .map_err(|error| format!("invalid CSS color: {error}"))
    }
}

impl From<Rgba8> for Color {
    fn from(value: Rgba8) -> Self {
        Self::Rgba8(value)
    }
}

/// Use the same sRGB conversion for inline styles and ordinary widget colors.
impl From<Rgba8> for voidui_gpui_wgpu::Hsla {
    fn from(value: Rgba8) -> Self {
        Color::from(value).into()
    }
}

/// Preserve all four channels when passing UI colors to the rendering backend.
impl From<Color> for voidui_gpui_wgpu::Hsla {
    fn from(value: Color) -> Self {
        match value {
            Color::CurrentColor => {
                panic!("currentColor must be resolved against an element before rendering")
            }
            Color::Rgba8(color) => voidui_gpui_wgpu::Rgba {
                r: f32::from(color.r) / f32::from(u8::MAX),
                g: f32::from(color.g) / f32::from(u8::MAX),
                b: f32::from(color.b) / f32::from(u8::MAX),
                a: f32::from(color.a) / f32::from(u8::MAX),
            }
            .into(),
            Color::Components(color) => {
                let [r, g, b, a] = map_to_srgb(color);
                // Native surfaces currently use sRGB. Keep wide-gamut values in
                // styles/interpolation; map only when writing an SDR primitive.
                voidui_gpui_wgpu::Rgba {
                    r: r.clamp(0.0, 1.0),
                    g: g.clamp(0.0, 1.0),
                    b: b.clamp(0.0, 1.0),
                    a: a.clamp(0.0, 1.0),
                }
                .into()
            }
        }
    }
}

impl Color {
    /// Construct a color in any of the CSS Color 4 predefined spaces.
    pub fn new(space: ColorSpace, components: [f32; 4]) -> Self {
        assert!(
            components.iter().all(|v| v.is_finite()),
            "color components must be finite"
        );
        Self::Components(color::DynamicColor {
            cs: space,
            flags: Default::default(),
            components: [
                components[0],
                components[1],
                components[2],
                components[3].clamp(0.0, 1.0),
            ],
        })
    }

    /// Read floating-point components without quantizing or losing missing-channel flags.
    /// Resolve currentColor against an element before calling this method.
    pub fn components(self) -> color::DynamicColor {
        match self {
            Self::CurrentColor => panic!("resolve currentColor before reading components"),
            Self::Rgba8(c) => color::DynamicColor::from_alpha_color(
                color::AlphaColor::<color::Srgb>::from_rgba8(c.r, c.g, c.b, c.a),
            ),
            Self::Components(c) => c,
        }
    }

    /// CSS premultiplied interpolation, including missing components and polar hue paths.
    pub fn interpolate(self, other: Self, t: f32, space: ColorSpace, hue: HueDirection) -> Self {
        Self::Components(
            self.components()
                .interpolate(other.components(), space, hue)
                .eval(t),
        )
    }

    pub fn resolve(self, current: Color) -> Color {
        match self {
            Self::CurrentColor => current,
            color => color,
        }
    }
}

impl From<Rgba8> for crate::style::value::CssValue<Color> {
    fn from(value: Rgba8) -> Self {
        Self::Value(value.into())
    }
}

/// CSS Color 4 binary-search gamut mapping with local MINDE. In-gamut colors
/// take the conversion-only fast path. Chroma reduction is only needed for colors
/// outside the sRGB presentation gamut; authored values remain untouched.
/// https://www.w3.org/TR/css-color-4/#binsearch
pub fn map_to_srgb(origin: color::DynamicColor) -> [f32; 4] {
    let rgb = origin.to_alpha_color::<color::Srgb>();
    let in_gamut = |components: [f32; 4]| components[..3].iter().all(|v| (0.0..=1.0).contains(v));
    if in_gamut(rgb.components) {
        return rgb.components;
    }
    let mut lch = origin.to_alpha_color::<color::Oklch>();
    let alpha = lch.components[3].clamp(0.0, 1.0);
    if lch.components[0] >= 1.0 {
        return [1.0, 1.0, 1.0, alpha];
    }
    if lch.components[0] <= 0.0 {
        return [0.0, 0.0, 0.0, alpha];
    }
    let clip = |v: color::AlphaColor<color::Srgb>| {
        color::AlphaColor::<color::Srgb>::new(v.components.map(|v| v.clamp(0.0, 1.0)))
    };
    let delta = |a: color::AlphaColor<color::Srgb>, b: color::AlphaColor<color::Oklch>| {
        let a = a.convert::<color::Oklab>().components;
        let b = b.convert::<color::Oklab>().components;
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
    };
    // CSS defines one just-noticeable Oklab difference and the search precision.
    const JND: f32 = 0.02;
    const EPSILON: f32 = 0.0001;
    let mut clipped = clip(rgb);
    if delta(clipped, lch) < JND {
        return clipped.components;
    }
    let (mut low, mut high) = (0.0, lch.components[1]);
    let mut low_in_gamut = true;
    while high - low > EPSILON {
        let chroma = (low + high) * 0.5;
        if chroma == low || chroma == high {
            break;
        }
        lch.components[1] = chroma;
        let rgb = lch.convert::<color::Srgb>();
        if low_in_gamut && in_gamut(rgb.components) {
            low = chroma;
            continue;
        }
        clipped = clip(rgb);
        let error = delta(clipped, lch);
        if error < JND {
            if JND - error < EPSILON {
                return clipped.components;
            }
            low_in_gamut = false;
            low = chroma;
        } else {
            high = chroma;
        }
    }
    clipped.components
}
