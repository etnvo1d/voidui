//! Authored inputs and aligned packing for the recovered background compositor.
use crate::{Affine, GlassMaterial, GlassTone};

/// Four blur controls before native difference-weight packing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlassBlurRamp {
    pub opacities: [f32; 4],
    pub distances: [f32; 4],
}
/// Background color sampled through the edge's larger optical footprint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlassBleed {
    pub amount: f32,
    pub height: f32,
    pub blur: f32,
    pub distances: [f32; 2],
    pub opacity: f32,
    /// Darken follows luminance^4; lighten follows (1-luminance)^4.
    pub darken: bool,
    pub tone: GlassTone,
}
/// A background-aware shadow, separate from CSS box-shadow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlassOpticalShadow {
    pub amount: f32,
    pub height: f32,
    pub blur: f32,
    pub radius: f32,
    /// Visible shadow translation. Packing negates it for SDF lookup.
    pub offset: [f32; 2],
    pub distance_offset: f32,
    pub opacity: f32,
    pub vibrancy: f32,
    pub face_opacity: f32,
    pub tone: GlassTone,
}
/// Apply a white holding tone across the specified distance interval.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlassHoldingTone {
    /// Desired holding strength before headroom suppression.
    pub opacity: f32,
    pub white: f32,
    pub distances: [f32; 2],
}
/// Complete FP32 background composition controls, in logical pixels.
/// Presets use observed material inputs; display-dependent native policy remains
/// the application's responsibility. The renderer does not query private APIs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlassBackground {
    pub outer_amount: f32,
    pub outer_height: f32,
    pub refraction_distances: [f32; 2],
    pub refraction_opacity: f32,
    pub complex_refraction: bool,
    pub blur_ramp: GlassBlurRamp,
    pub face_opacity: f32,
    pub bleed: Option<GlassBleed>,
    pub shadow: Option<GlassOpticalShadow>,
    pub holding: Option<GlassHoldingTone>,
    /// Working transfer exponent used by native host clamp packing (2.2 in the
    /// traced SDR CARenderer). This is not a full display color-space transform.
    pub working_gamma: f32,
    /// Available output headroom and authored maximum for SDR shadow adaptation.
    pub headroom: f32,
    pub max_headroom: f32,
    pub sdr_shadow_opacity: f32,
    /// Zero disables clamping. Hue preservation scales all channels together.
    pub clamp: f32,
    pub preserve_hue: bool,
    /// Encoded output scale; an SDR surface cannot present values above its range.
    pub edr_scale: f32,
}
impl GlassBackground {
    pub const fn regular() -> Self {
        Self {
            outer_amount: 46.,
            outer_height: 28.75,
            refraction_distances: [-1., -0.5],
            refraction_opacity: 0.3,
            complex_refraction: true,
            blur_ramp: GlassBlurRamp {
                opacities: [1., 0.5, 0.5, 1.],
                distances: [-115., -1., 0., 0.],
            },
            face_opacity: 1.,
            bleed: Some(GlassBleed {
                amount: 80.5,
                height: 80.5,
                blur: 160.,
                distances: [1., 0.],
                opacity: 0.5,
                darken: true,
                tone: GlassTone {
                    white: 1.,
                    black: 0.9,
                    saturation: 1.2,
                    fill: [0.; 4],
                },
            }),
            shadow: Some(GlassOpticalShadow {
                amount: 75.,
                height: 92.,
                blur: 40.,
                radius: 24.,
                offset: [0., 8.],
                distance_offset: 0.,
                opacity: 0.25,
                vibrancy: 1.,
                face_opacity: 0.12,
                tone: GlassTone {
                    white: 1.,
                    black: 0.,
                    saturation: 1.8,
                    fill: [0., 0., 0., 0.12],
                },
            }),
            holding: Some(GlassHoldingTone {
                opacity: 1.,
                white: 0.97,
                distances: [-1., -0.5],
            }),
            working_gamma: 2.2,
            headroom: 1.,
            max_headroom: 9999.,
            sdr_shadow_opacity: 0.24,
            clamp: 1.06961,
            preserve_hue: false,
            edr_scale: 1.,
        }
    }
    pub const fn regular_dark() -> Self {
        Self {
            bleed: Some(GlassBleed {
                amount: 80.5,
                height: 80.5,
                blur: 160.,
                distances: [1., 0.],
                opacity: 0.8,
                darken: false,
                tone: GlassTone {
                    white: 0.5,
                    black: 0.,
                    saturation: 1.,
                    fill: [0.; 4],
                },
            }),
            shadow: Some(GlassOpticalShadow {
                amount: 75.,
                height: 92.,
                blur: 40.,
                radius: 24.,
                offset: [0., 8.],
                distance_offset: 0.,
                opacity: 0.25,
                vibrancy: 1.,
                face_opacity: 0.,
                tone: GlassTone {
                    white: 0.5,
                    black: 0.,
                    saturation: 1.,
                    fill: [0.; 4],
                },
            }),
            clamp: 1.,
            ..Self::regular()
        }
    }
    pub const fn clear() -> Self {
        Self {
            refraction_opacity: 0.,
            bleed: None,
            shadow: None,
            clamp: 1.375824,
            ..Self::regular()
        }
    }
    pub fn is_valid(self) -> bool {
        let finite = |values: &[f32]| values.iter().all(|v| v.is_finite());
        let unit = |v: f32| v.is_finite() && (0. ..=1.).contains(&v);
        finite(&[
            self.outer_amount,
            self.outer_height,
            self.refraction_distances[0],
            self.refraction_distances[1],
            self.clamp,
            self.edr_scale,
            self.working_gamma,
            self.headroom,
            self.max_headroom,
        ]) && self.outer_height > 0.
            && self.clamp >= 0.
            && self.edr_scale >= 0.
            && unit(self.refraction_opacity)
            && unit(self.face_opacity)
            && finite(&self.blur_ramp.distances)
            && self.blur_ramp.distances.windows(2).all(|w| w[0] <= w[1])
            && self.blur_ramp.opacities.into_iter().all(unit)
            && self.bleed.is_none_or(|b| {
                finite(&[b.amount, b.height, b.blur, b.distances[0], b.distances[1]])
                    && b.height > 0.
                    && b.blur >= 0.
                    && unit(b.opacity)
                    && b.tone.is_valid()
            })
            && self.shadow.is_none_or(|s| {
                finite(&[
                    s.amount,
                    s.height,
                    s.blur,
                    s.radius,
                    s.offset[0],
                    s.offset[1],
                    s.distance_offset,
                ]) && s.height > 0.
                    && s.radius > 0.
                    && s.blur >= 0.
                    && unit(s.opacity)
                    && unit(s.vibrancy)
                    && unit(s.face_opacity)
                    && s.tone.is_valid()
            })
            && self.holding.is_none_or(|h| {
                unit(h.opacity)
                    && finite(&[h.white, h.distances[0], h.distances[1]])
                    && h.white >= 0.
                    && h.distances[0] <= h.distances[1]
            })
    }
    pub(crate) fn scaled(mut self, scale: f32) -> Self {
        self.outer_amount *= scale;
        self.outer_height *= scale;
        self.refraction_distances = self.refraction_distances.map(|v| v * scale);
        self.blur_ramp.distances = self.blur_ramp.distances.map(|v| v * scale);
        if let Some(b) = &mut self.bleed {
            b.amount *= scale;
            b.height *= scale;
            b.blur *= scale;
            b.distances = b.distances.map(|v| v * scale);
        }
        if let Some(s) = &mut self.shadow {
            s.amount *= scale;
            s.height *= scale;
            s.blur *= scale;
            s.radius *= scale;
            s.offset = s.offset.map(|v| v * scale);
            s.distance_offset *= scale;
        }
        if let Some(h) = &mut self.holding {
            h.distances = h.distances.map(|v| v * scale);
        }
        self
    }
    pub(crate) fn paint_padding(self) -> f32 {
        self.shadow.filter(|s| s.opacity > 0.).map_or(0., |s| {
            2. * s.radius + s.offset[0].abs().max(s.offset[1].abs()) + s.distance_offset.abs()
        })
    }
    pub(crate) fn max_blur(self, base: f32) -> f32 {
        base.max(self.bleed.filter(|b| b.opacity > 0.).map_or(0., |b| b.blur))
            .max(
                self.shadow
                    .filter(|s| s.opacity > 0. && s.vibrancy > 0.)
                    .map_or(0., |s| s.blur),
            )
    }
    pub(crate) fn max_displacement(self, inner: f32) -> f32 {
        inner
            .max(self.outer_amount.abs())
            .max(
                self.bleed
                    .filter(|b| b.opacity > 0.)
                    .map_or(0., |b| b.amount.abs()),
            )
            .max(
                self.shadow
                    .filter(|s| s.opacity > 0.)
                    .map_or(0., |s| s.amount.abs()),
            )
    }
}

/// Matches GlassBackgroundParams in WGSL; vec4-only fields avoid host ABI padding.
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub(crate) struct GlassBackgroundGpu(pub [[f32; 4]; 21]);
impl GlassBackgroundGpu {
    pub fn new(
        m: GlassMaterial,
        transform: Affine,
        viewport: (u32, u32),
        source_scale: f32,
    ) -> Self {
        let mut p = m.background.unwrap_or(GlassBackground::clear());
        // GlassBackgroundFilter::render adjusts shadow fill and holding against
        // headroom before building matrices. A source-level CAFilter snapshot
        // is not the final GPU parameter block.
        let headroom_fraction = if p.max_headroom > f32::from_bits(0x3f800001) {
            ((p.headroom.min(p.max_headroom) - 1.).max(0.)) / (p.max_headroom - 1.)
        } else {
            1.
        };
        if let Some(shadow) = &mut p.shadow {
            let extra = (1. - headroom_fraction) * p.sdr_shadow_opacity;
            let old_alpha = shadow.tone.fill[3];
            let new_alpha = old_alpha + extra;
            // Native packing retains premultiplied fill RGB while increasing
            // alpha; keep that invariant through the public straight-alpha type.
            if new_alpha > 0. {
                for channel in &mut shadow.tone.fill[..3] {
                    *channel *= old_alpha / new_alpha;
                }
            }
            shadow.tone.fill[3] = new_alpha;
            shadow.face_opacity += extra;
        }
        if let Some(holding) = &mut p.holding {
            holding.opacity *= 1. - headroom_fraction;
        }
        p.clamp = p.clamp.powf(1. / p.working_gamma);
        let [a, b, c, d, _, _] = transform.0;
        let device_scale = (a.abs() + c.abs()).max(b.abs() + d.abs());
        let blur_scale = device_scale * source_scale;
        let matrix = |tone: Option<GlassTone>| {
            tone.map_or(
                [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
                |t| t.matrix().map(|row| row.map(crate::glass::round_half)),
            )
        };
        let face = matrix(m.tone);
        let bleed = matrix(p.bleed.map(|b| b.tone));
        let shadow = matrix(p.shadow.map(|s| s.tone));
        let op = p.blur_ramp.opacities;
        let mut rows = [[0.; 4]; 21];
        rows[0] = [
            a / viewport.0 as f32,
            c / viewport.0 as f32,
            b / viewport.1 as f32,
            d / viewport.1 as f32,
        ];
        rows[1] = [
            -m.refraction,
            1. / m.thickness,
            p.outer_amount,
            1. / p.outer_height,
        ];
        rows[2] = [
            p.refraction_distances[0],
            p.refraction_distances[1],
            p.refraction_opacity,
            f32::from(p.complex_refraction),
        ];
        rows[3] = [
            m.blur * blur_scale,
            p.bleed.map_or(0., |b| b.blur * blur_scale),
            p.shadow.map_or(0., |s| s.blur * blur_scale),
            p.face_opacity,
        ];
        rows[4] = [op[0], op[0] - op[1], op[1] - op[2], op[2] - op[3]];
        rows[5] = p.blur_ramp.distances;
        rows[6..9].copy_from_slice(&face);
        rows[9..12].copy_from_slice(&bleed);
        rows[12..15].copy_from_slice(&shadow);
        if let Some(b) = p.bleed {
            rows[15] = [b.amount, 1. / b.height, b.distances[0], b.distances[1]];
            rows[16] = [
                b.opacity,
                if b.darken { 1. } else { -1. },
                if b.darken { 0. } else { 1. },
                1.,
            ];
        }
        if let Some(s) = p.shadow {
            rows[17] = [s.amount, 1. / s.height, s.opacity, s.vibrancy];
            rows[18] = [-s.offset[0], -s.offset[1], 1. / s.radius, s.distance_offset];
        }
        if let Some(h) = p.holding {
            rows[19] = [h.opacity, h.white, h.distances[0], h.distances[1]];
        }
        rows[20] = [
            p.clamp,
            f32::from(p.preserve_hue),
            p.edr_scale,
            p.shadow.map_or(0., |s| s.face_opacity),
        ];
        Self(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uniform_layout_and_native_difference_weights() {
        assert_eq!(std::mem::size_of::<GlassBackgroundGpu>(), 336);
        let m = GlassMaterial::regular().scaled(2.);
        let gpu = GlassBackgroundGpu::new(m, Affine::IDENTITY, (640, 480), 0.25).0;
        assert_eq!(gpu[1], [-120., 1. / 40., 92., 1. / 57.5]);
        assert_eq!(gpu[4], [1., 0.5, 0., -0.5]);
        assert_eq!(gpu[5], [-230., -2., 0., 0.]);
        assert_eq!(gpu[18], [0., -16., 1. / 48., 0.]);
        assert_eq!(gpu[3][0], 2.);
        assert_eq!(gpu[3][1], 80.);
    }
    #[test]
    fn host_transfer_matches_traced_half_uniforms() {
        let reference: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/glass-traced-uniforms.json"))
                .unwrap();
        let values = &reference["values"];
        let scalar = |name: &str| values[name].as_f64().unwrap() as f32;
        let mut m = GlassMaterial::regular();
        let mut p = m.background.unwrap();
        // Authored values read from the same live 140x140 pair, before QuartzCore
        // transforms them. No GPU-buffer values are fed into the constructor.
        p.outer_amount = 28.;
        p.outer_height = 17.5;
        p.blur_ramp.distances[0] = -70.;
        p.bleed.as_mut().unwrap().opacity = 0.3958333;
        p.shadow.as_mut().unwrap().height = 56.;
        p.shadow.as_mut().unwrap().opacity = 0.2946429;
        p.shadow.as_mut().unwrap().vibrancy = 0.7916667;
        p.sdr_shadow_opacity = 0.2114286;
        m.background = Some(p);
        let packed = GlassBackgroundGpu::new(m, Affine::IDENTITY, (640, 420), 0.25).0;
        assert!((packed[20][3] - scalar("shadow_face_opacity")).abs() < 1e-6);
        assert_eq!(
            crate::glass::round_half(packed[20][0]),
            scalar("clamp_limit")
        );
        assert_eq!(packed[19][0], scalar("holding_tone_opacity"));
        for row in 0..3 {
            for column in 0..4 {
                let native = values[format!("shadow_cm{row}")][column].as_f64().unwrap() as f32;
                assert_eq!(packed[12 + row][column], native);
            }
        }
    }
    #[test]
    fn presets_and_invalid_optics() {
        assert!(GlassBackground::regular().is_valid());
        assert!(GlassBackground::regular_dark().is_valid());
        assert!(GlassBackground::clear().is_valid());
        let mut b = GlassBackground::regular();
        b.outer_height = 0.;
        assert!(!b.is_valid());
        let mut b = GlassBackground::regular();
        b.blur_ramp.distances = [0., 1., -1., 2.];
        assert!(!b.is_valid());
        let mut b = GlassBackground::regular();
        b.edr_scale = f32::INFINITY;
        assert!(!b.is_valid());
    }
}
