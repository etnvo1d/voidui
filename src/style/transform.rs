//! CSS 2D transform lists. Percentages resolve against the border box after layout.
use super::{list::StyleList, math::MathLength};
use crate::render::Affine;
use std::str::FromStr;

#[derive(Clone, Debug, PartialEq)]
pub enum TransformLength {
    Pixels(f32),
    Percent(f32),
    Math(MathLength),
}
impl TransformLength {
    pub fn resolve(&self, basis: f32) -> f32 {
        match self {
            Self::Pixels(v) => *v,
            Self::Percent(v) => v * basis,
            Self::Math(v) => super::math::resolve_calc(v.handle(), basis),
        }
    }
}
impl From<f32> for TransformLength {
    fn from(v: f32) -> Self {
        assert!(v.is_finite());
        Self::Pixels(v)
    }
}
impl FromStr for TransformLength {
    type Err = String;
    fn from_str(raw: &str) -> Result<Self, String> {
        super::css::transform::length(raw)
    }
}
#[derive(Clone, Debug, PartialEq)]
pub enum TransformFunction {
    Translate(TransformLength, TransformLength),
    Scale(f32, f32),
    /// Angles in radians; the CSS parser accepts deg, rad, grad, and turn.
    Rotate(f32),
    Skew(f32, f32),
    SkewX(f32),
    SkewY(f32),
    Matrix(Affine),
}
impl TransformFunction {
    fn matrix(&self, width: f32, height: f32) -> Affine {
        match self {
            Self::Translate(x, y) => Affine::translate(x.resolve(width), y.resolve(height)),
            Self::Scale(x, y) => Affine::scale(*x, *y),
            Self::Rotate(a) => Affine::rotate(*a),
            Self::SkewX(x) => Affine([1., 0., x.tan(), 1., 0., 0.]),
            Self::SkewY(y) => Affine([1., y.tan(), 0., 1., 0., 0.]),
            Self::Skew(x, y) => Affine([1., y.tan(), x.tan(), 1., 0., 0.]),
            Self::Matrix(m) => *m,
        }
    }
}
/// `none` has an empty, allocation-free list. Identity functions still establish
/// containing blocks and stacking contexts, as required by CSS.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Transform(pub StyleList<TransformFunction>);
impl Transform {
    pub fn is_none(&self) -> bool {
        self.0.is_empty()
    }
    pub fn matrix(&self, width: f32, height: f32) -> Affine {
        self.0
            .iter()
            .fold(Affine::IDENTITY, |m, f| m.compose(f.matrix(width, height)))
    }
}
impl FromStr for Transform {
    type Err = String;
    fn from_str(raw: &str) -> Result<Self, String> {
        super::css::transform::parse(raw)
    }
}
impl<const N: usize> From<[TransformFunction; N]> for Transform {
    fn from(v: [TransformFunction; N]) -> Self {
        Self(v.into())
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct TransformOrigin {
    pub x: TransformLength,
    pub y: TransformLength,
}
impl Default for TransformOrigin {
    fn default() -> Self {
        Self {
            x: TransformLength::Percent(0.5),
            y: TransformLength::Percent(0.5),
        }
    }
}
impl FromStr for TransformOrigin {
    type Err = String;
    fn from_str(raw: &str) -> Result<Self, String> {
        super::css::transform::origin(raw)
    }
}

impl TransformLength {
    fn expression(&self) -> super::math::Expression {
        match self {
            Self::Pixels(v) => super::math::Expression::Pixels(*v as f64),
            Self::Percent(v) => super::math::Expression::Percent(*v as f64),
            Self::Math(v) => v.expression().clone(),
        }
    }
    pub(crate) fn interpolate(&self, other: &Self, t: f32) -> Self {
        let mix = |a: f32, b: f32| a + (b - a) * t;
        match (self, other) {
            (Self::Pixels(a), Self::Pixels(b)) => Self::Pixels(mix(*a, *b)),
            (Self::Percent(a), Self::Percent(b)) => Self::Percent(mix(*a, *b)),
            _ => Self::Math(MathLength::new(
                super::math::Expression::Sum(
                    Box::new(super::math::Expression::Scale(
                        Box::new(self.expression()),
                        (1. - t) as f64,
                    )),
                    Box::new(super::math::Expression::Scale(
                        Box::new(other.expression()),
                        t as f64,
                    )),
                ),
                false,
            )),
        }
    }
}
impl TransformFunction {
    fn identity(&self) -> Self {
        match self {
            Self::Translate(..) => {
                Self::Translate(TransformLength::Pixels(0.), TransformLength::Pixels(0.))
            }
            Self::Scale(..) => Self::Scale(1., 1.),
            Self::Rotate(..) => Self::Rotate(0.),
            Self::Skew(..) => Self::Skew(0., 0.),
            Self::SkewX(..) => Self::SkewX(0.),
            Self::SkewY(..) => Self::SkewY(0.),
            Self::Matrix(..) => Self::Matrix(Affine::IDENTITY),
        }
    }
}
impl Transform {
    /// CSS matches primitive functions first, then decomposes the unmatched
    /// suffix into translation, rotation, scale, and shear. Rotations in matching
    /// function lists retain full turns instead of taking a matrix's shortest arc.
    pub(crate) fn interpolate(&self, other: &Self, t: f32, size: [f32; 2]) -> Option<Self> {
        use TransformFunction::*;
        let mix = |a: f32, b: f32| a + (b - a) * t;
        let mut list = Vec::with_capacity(self.0.len().max(other.0.len()));
        for i in 0..self.0.len().max(other.0.len()) {
            let ai;
            let bi;
            let a = if let Some(a) = self.0.get(i) {
                a
            } else {
                ai = other.0[i].identity();
                &ai
            };
            let b = if let Some(b) = other.0.get(i) {
                b
            } else {
                bi = self.0[i].identity();
                &bi
            };
            list.push(match (a, b) {
                (Translate(x, y), Translate(u, v)) => {
                    Translate(x.interpolate(u, t), y.interpolate(v, t))
                }
                (Scale(x, y), Scale(u, v)) => Scale(mix(*x, *u), mix(*y, *v)),
                (Rotate(a), Rotate(b)) => Rotate(mix(*a, *b)),
                (SkewX(x), SkewX(y)) => SkewX(mix(*x, *y)),
                (SkewY(x), SkewY(y)) => SkewY(mix(*x, *y)),
                (Skew(x, y), Skew(u, v)) => Skew(mix(*x, *u), mix(*y, *v)),
                _ => {
                    let suffix = |functions: &[TransformFunction]| {
                        functions.iter().fold(Affine::IDENTITY, |m, f| {
                            m.compose(f.matrix(size[0], size[1]))
                        })
                    };
                    let a = suffix(self.0.get(i..).unwrap_or(&[]));
                    let b = suffix(other.0.get(i..).unwrap_or(&[]));
                    list.push(Matrix(interpolate_matrix(a, b, t)?));
                    break;
                }
            });
        }
        Some(Self(list.into()))
    }
}
fn interpolate_matrix(a: Affine, b: Affine, t: f32) -> Option<Affine> {
    fn decompose(m: Affine) -> Option<[f32; 9]> {
        m.inverse()?;
        let [a, b, c, d, x, y] = m.0;
        let mut sx = a.hypot(b);
        let mut sy = c.hypot(d);
        if a * d - b * c < 0. {
            if a < d {
                sx = -sx;
            } else {
                sy = -sy;
            }
        }
        let (a, b, c, d) = (a / sx, b / sx, c / sy, d / sy);
        let angle = b.atan2(a);
        let (sn, cs) = (-b, a);
        // Retain the residual 2x2 matrix, rather than reducing it to one shear.
        // This matches CSS matrix interpolation for skewed and reflected boxes.
        Some([
            x,
            y,
            angle,
            sx,
            sy,
            cs * a + sn * c,
            cs * b + sn * d,
            -sn * a + cs * c,
            -sn * b + cs * d,
        ])
    }
    let mut a = decompose(a)?;
    let mut b = decompose(b)?;
    if (a[3] < 0. && b[4] < 0.) || (a[4] < 0. && b[3] < 0.) {
        a[3] = -a[3];
        a[4] = -a[4];
        a[2] += if a[2] < 0. {
            std::f32::consts::PI
        } else {
            -std::f32::consts::PI
        };
    }
    if a[2] == 0. {
        a[2] = std::f32::consts::TAU;
    }
    if b[2] == 0. {
        b[2] = std::f32::consts::TAU;
    }
    if (a[2] - b[2]).abs() > std::f32::consts::PI {
        if a[2] > b[2] {
            a[2] -= std::f32::consts::TAU;
        } else {
            b[2] -= std::f32::consts::TAU;
        }
    }
    let v = std::array::from_fn::<_, 9, _>(|i| a[i] + (b[i] - a[i]) * t);
    Some(
        Affine::translate(v[0], v[1])
            .compose(Affine([v[5], v[6], v[7], v[8], 0., 0.]))
            .compose(Affine::rotate(v[2]))
            .compose(Affine::scale(v[3], v[4])),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn close(a: Affine, b: Affine) {
        for (a, b) in a.0.into_iter().zip(b.0) {
            assert!((a - b).abs() < 0.0001, "{a} != {b}");
        }
    }
    #[test]
    fn matrix_decomposition_preserves_endpoints_and_full_turn_functions() {
        let a = Affine([1.2, 0.4, 0.8, -2., 10., 20.]);
        let b = Affine([-1., 0.8, -0.4, 1.5, 30., -12.]);
        close(interpolate_matrix(a, b, 0.).unwrap(), a);
        close(interpolate_matrix(a, b, 1.).unwrap(), b);
        let a: Transform = "rotate(0deg)".parse().unwrap();
        let b: Transform = "rotate(360deg)".parse().unwrap();
        close(
            a.interpolate(&b, 0.5, [100., 100.])
                .unwrap()
                .matrix(100., 100.),
            Affine::rotate(std::f32::consts::PI),
        );
    }
}
