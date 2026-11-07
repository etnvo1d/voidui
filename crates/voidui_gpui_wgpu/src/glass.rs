//! Portable backdrop material. Lengths are logical pixels at the painter API.
use crate::Quad;

/// Resolution of the backdrop before the blur pyramid is built. Lower resolutions
/// save filtering work but discard fine detail even when the blur radius is zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlassBackdropResolution {
    Full,
    Half,
    Quarter,
}
impl GlassBackdropResolution {
    pub(crate) const fn divisor(self) -> u32 {
        match self {
            Self::Full => 1,
            Self::Half => 2,
            Self::Quarter => 4,
        }
    }
}

/// An experimental distance-field glass material, independent of native window effects.
/// Core SDF/refraction formulas follow recovered native AIR. Full AppKit color
/// and compositing parity is still under validation.
/// Presets record observed macOS 26.5 material inputs; adaptive AppKit policy
/// is not reproduced. Set `opacity` to zero to disable the pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlassMaterial {
    /// Use the complete recovered background compositor; None preserves backdrop alpha.
    pub background: Option<crate::GlassBackground>,
    /// Base backdrop resolution, independent of the optical blur radius.
    pub backdrop_resolution: GlassBackdropResolution,
    /// Blur-pyramid radius in logical pixels. With `background: None`, zero
    /// samples the full-resolution source regardless of backdrop resolution.
    pub blur: f32,
    /// Maximum inward background displacement, in logical pixels.
    pub refraction: f32,
    /// Width of the curved edge, in logical pixels.
    pub thickness: f32,
    /// Background saturation multiplier.
    pub saturation: f32,
    /// Straight-alpha sRGB tint.
    pub tint: [f32; 4],
    /// Edge reflection intensity, from zero to one.
    pub highlight: f32,
    /// Optional native YCbCr face transfer, applied before an authored tint.
    pub tone: Option<GlassTone>,
    pub key_light: GlassLight,
    pub fill_light: GlassLight,
    pub curvature: f32,
    /// Continuous corner contour, matching the native glass shape mode.
    pub continuous: bool,
    /// Mix the contour gradient toward an oval before merging shapes.
    pub ovalization: f32,
    /// Material coverage, from zero to one.
    pub opacity: f32,
}
impl Default for GlassMaterial {
    fn default() -> Self {
        Self::regular()
    }
}
impl GlassMaterial {
    pub const fn regular() -> Self {
        Self {
            background: Some(crate::GlassBackground::regular()),
            backdrop_resolution: GlassBackdropResolution::Quarter,
            blur: 4.,
            refraction: 60.,
            thickness: 20.,
            saturation: 1.,
            tint: [0.; 4],
            tone: Some(GlassTone {
                white: 1.03,
                black: 0.5,
                saturation: 1.,
                fill: [1., 1., 1., 0.4],
            }),
            key_light: GlassLight {
                height: 1.,
                angle: -std::f32::consts::FRAC_PI_4,
                spread: 2.7925268,
                amount: 0.5,
            },
            fill_light: GlassLight {
                height: 1.,
                angle: 3. * std::f32::consts::FRAC_PI_4,
                spread: 2.7925268,
                amount: 0.5,
            },
            curvature: 0.7,
            highlight: 1.,
            opacity: 1.,
            continuous: true,
            ovalization: 0.5,
        }
    }
    pub const fn clear() -> Self {
        Self {
            background: Some(crate::GlassBackground::clear()),
            backdrop_resolution: GlassBackdropResolution::Half,
            blur: 1.,
            tone: Some(GlassTone {
                white: 1.15,
                black: 0.075,
                saturation: 1.06,
                fill: [0.; 4],
            }),
            ..Self::regular()
        }
    }
    /// Full-resolution, unblurred glass with a neutral face and a refractive rim.
    /// The zero-blur/identity-face policy follows the observed iOS 26 text loupe.
    /// This material does not magnify or relocate text like UIKit's portal layer.
    /// Geometry remains adjustable; use CSS box-shadow for an authored shadow.
    pub const fn unblurred() -> Self {
        Self {
            background: Some(crate::GlassBackground::unblurred()),
            backdrop_resolution: GlassBackdropResolution::Full,
            blur: 0.,
            tone: None,
            // Observed 115x85-point system loupe: rim displacement and height.
            refraction: 25.5,
            thickness: 8.075,
            ovalization: 0.,
            curvature: 0.8,
            key_light: GlassLight {
                spread: std::f32::consts::FRAC_PI_2,
                ..Self::clear().key_light
            },
            fill_light: GlassLight {
                spread: std::f32::consts::FRAC_PI_2,
                ..Self::clear().fill_light
            },
            ..Self::clear()
        }
    }
    /// An unfiltered source can bind the existing sharp capture directly. Do not
    /// infer this from face blur alone: bleed and shadow may still need mips.
    pub(crate) fn samples_sharp_backdrop(self) -> bool {
        self.background.map_or(self.blur == 0., |background| {
            self.backdrop_resolution == GlassBackdropResolution::Full
                && background.max_blur(self.blur) == 0.
        })
    }
    /// Observed active dark material endpoints on macOS 26.5.
    pub const fn regular_dark() -> Self {
        Self {
            background: Some(crate::GlassBackground::regular_dark()),
            tone: Some(GlassTone {
                white: 0.6,
                black: 0.2,
                saturation: 1.,
                fill: [0., 0., 0., 0.4],
            }),
            ..Self::regular()
        }
    }
    /// Validate authored values before recording GPU work.
    pub fn is_valid(self) -> bool {
        self.background.is_none_or(crate::GlassBackground::is_valid)
            && self.key_light.is_valid()
            && self.fill_light.is_valid()
            && self.tone.is_none_or(GlassTone::is_valid)
            && [self.blur, self.refraction, self.thickness, self.saturation]
                .into_iter()
                .all(|v| v.is_finite() && v >= 0.)
            && self.thickness > 0.
            && self
                .tint
                .into_iter()
                .chain([
                    self.highlight,
                    self.opacity,
                    self.ovalization,
                    self.curvature,
                ])
                .all(|v| v.is_finite() && (0. ..=1.).contains(&v))
    }
    pub(crate) fn scaled(self, scale: f32) -> Self {
        Self {
            background: self.background.map(|p| p.scaled(scale)),
            blur: self.blur * scale,
            refraction: self.refraction * scale,
            thickness: self.thickness * scale,
            key_light: GlassLight {
                height: self.key_light.height * scale,
                ..self.key_light
            },
            fill_light: GlassLight {
                height: self.fill_light.height * scale,
                ..self.fill_light
            },
            ..self
        }
    }
}

/// Device-scaled glass geometry. Use `Painter::paint_glass` for logical pixels.
#[derive(Clone, Debug)]
pub struct Glass {
    pub quad: Quad,
    pub material: GlassMaterial,
    /// Device-scaled native SDF smooth-union width. Zero keeps shapes separate.
    pub smoothing: f32,
    /// Evaluate one union field, including the zero-smoothing hard-union limit.
    pub merge: bool,
}
impl From<Glass> for crate::Primitive {
    fn from(value: Glass) -> Self {
        Self::Glass(value)
    }
}

/// Luminance/chroma material transfer before optical composition.
/// Values describe endpoints in YCbCr, not three independent RGB contrasts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlassTone {
    pub white: f32,
    pub black: f32,
    pub saturation: f32,
    /// Straight-alpha sRGB fill, converted to premultiplied form during packing.
    pub fill: [f32; 4],
}
impl GlassTone {
    /// Recover the native `ColorMatrix::set_ycc_composite` construction.
    /// The rounded conversion constants are intentional: replacing the inverse
    /// with a numerically exact matrix does not reproduce QuartzCore's values.
    pub fn matrix(self) -> [[f32; 4]; 3] {
        type Matrix = [[f32; 5]; 4];
        const TO_YCC: Matrix = [
            [0.2126, 0.7152, 0.0722, 0., 0.],
            [-0.1146, -0.3854, 0.5, 0., 0.5],
            [0.5, -0.4542, -0.0458, 0., 0.5],
            [0., 0., 0., 1., 0.],
        ];
        const TO_RGB: Matrix = [
            [1., 0., 1.5748, 0., -0.7874],
            [1., -0.187324, -0.468124, 0., 0.327724],
            [1., 1.8556, 0., 0., -0.9278],
            [0., 0., 0., 1., 0.],
        ];
        let multiply = |a: Matrix, b: Matrix| {
            let mut result = [[0.; 5]; 4];
            for row in 0..4 {
                for column in 0..5 {
                    result[row][column] = (0..4).map(|i| a[row][i] * b[i][column]).sum::<f32>()
                        + if column == 4 { a[row][4] } else { 0. };
                }
            }
            result
        };
        let luminance = [
            [self.white - self.black, 0., 0., 0., self.black],
            [0., 1., 0., 0., 0.],
            [0., 0., 1., 0., 0.],
            [0., 0., 0., 1., 0.],
        ];
        let s = self.saturation;
        let chroma = [
            [1., 0., 0., 0., 0.],
            [0., s, 0., 0., 0.5 - 0.5 * s],
            [0., 0., s, 0., 0.5 - 0.5 * s],
            [0., 0., 0., 1., 0.],
        ];
        let matrix = multiply(TO_RGB, multiply(chroma, multiply(luminance, TO_YCC)));
        let alpha = self.fill[3];
        std::array::from_fn(|row| {
            [
                matrix[row][0] * (1. - alpha),
                matrix[row][1] * (1. - alpha),
                matrix[row][2] * (1. - alpha),
                matrix[row][4] * (1. - alpha) + self.fill[row] * alpha,
            ]
        })
    }
    pub(crate) fn is_valid(self) -> bool {
        [self.white, self.black, self.saturation]
            .into_iter()
            .all(f32::is_finite)
            && self
                .matrix()
                .into_iter()
                .flatten()
                .all(|v| v.is_finite() && v.abs() <= 65504.)
            && self
                .fill
                .into_iter()
                .all(|v| v.is_finite() && (0. ..=1.).contains(&v))
    }
}

/// Parameters of one native-style directional rim lobe.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlassLight {
    pub height: f32,
    /// Radians, with direction `(sin(angle), -cos(angle))` in local coordinates.
    pub angle: f32,
    pub spread: f32,
    /// Native contrast mapping is `1 / amount - 2`. Must be in (0, 1].
    pub amount: f32,
}
impl GlassLight {
    fn is_valid(self) -> bool {
        self.height.is_finite()
            && self.height > 0.
            && self.angle.is_finite()
            && self.spread.is_finite()
            && (0. ..=std::f32::consts::PI).contains(&self.spread)
            && self.amount.is_finite()
            && self.amount > 0.
            && self.amount <= 1.
    }
}

/// Round a finite coefficient exactly once when packing native half matrices.
/// This keeps half conversion work out of every shaded fragment on f32 backends.
pub(crate) fn round_half(value: f32) -> f32 {
    let bits = value.to_bits();
    let sign = bits & 0x8000_0000;
    let magnitude = bits & 0x7fff_ffff;
    if magnitude >= 0x7f80_0000 {
        return value;
    }
    if magnitude < 0x3880_0000 {
        let rounded = (value.abs() * 16_777_216.).round_ties_even() / 16_777_216.;
        return f32::from_bits(rounded.to_bits() | sign);
    }
    let rounded = (magnitude + 4095 + ((magnitude >> 13) & 1)) & 0xffff_e000;
    f32::from_bits(
        sign | if rounded >= 0x4780_0000 {
            0x7f80_0000
        } else {
            rounded
        },
    )
}
