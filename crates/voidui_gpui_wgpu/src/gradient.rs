//! Variable-length GPU color ramps. One quad covers each gradient, irrespective
//! of its stop count; fragments use binary search rather than a fixed-size loop.
use crate::*;

pub(crate) const GRADIENT_HEADER_WORDS: usize = 5;

#[derive(Debug, Clone, Copy)]
pub enum GradientGeometry {
    Linear {
        center: Point<Pixels>,
        direction: [f32; 2],
        length: f32,
    },
    Radial {
        center: Point<Pixels>,
        radii: Size<Pixels>,
    },
    /// CSS degrees: zero points up and positive angles turn clockwise.
    Conic { center: Point<Pixels>, angle: f32 },
}
#[derive(Debug, Clone, Copy)]
pub struct GradientStop {
    pub offset: f32,
    /// Premultiplied, encoded sRGB. The shader unpremultiplies before writing
    /// to the presentation target's compositing space.
    pub color: [f32; 4],
}
#[derive(Debug, Clone)]
pub struct GradientPaint {
    pub geometry: GradientGeometry,
    pub repeating: bool,
    pub stops: Vec<GradientStop>,
    /// Optional repeated background image tile, relative to the painted quad.
    pub tile: Option<Bounds<Pixels>>,
}
impl GradientPaint {
    /// Record layout, in 16-byte words (shared by storage and WebGL transports):
    ///   0: kind, repeat, stop count, reserved
    ///   1: center x/y, parameter x/y
    ///   2: gradient length, reserved...
    ///   3: repeated tile origin x/y and size x/y (zero size disables tiling)
    ///   4: average premultiplied sRGB (subpixel repeating patterns)
    ///   5 + 2*i: stop offset, reserved...
    ///   6 + 2*i: premultiplied color
    pub(crate) fn encode(&self, origin: Point<Pixels>, scale: f32, out: &mut Vec<[u32; 4]>) -> u32 {
        let index = u32::try_from(out.len()).expect("gradient data exceeds u32 indices");
        let count = u32::try_from(self.stops.len()).expect("too many gradient stops");
        let origin = origin + self.tile.map(|t| t.origin).unwrap_or_default();
        let (kind, center, parameters, length) = match self.geometry {
            GradientGeometry::Linear {
                center,
                direction,
                length,
            } => (0, center, direction, length * scale),
            GradientGeometry::Radial { center, radii } => (
                1,
                center,
                [
                    f32::from(radii.width) * scale,
                    f32::from(radii.height) * scale,
                ],
                0.0,
            ),
            GradientGeometry::Conic { center, angle } => {
                (2, center, [angle.to_radians(), 0.0], 0.0)
            }
        };
        out.push([kind, u32::from(self.repeating), count, 0]);
        out.push([
            (f32::from(center.x + origin.x) * scale).to_bits(),
            (f32::from(center.y + origin.y) * scale).to_bits(),
            parameters[0].to_bits(),
            parameters[1].to_bits(),
        ]);
        out.push([length.to_bits(), 0, 0, 0]);
        let tile = self
            .tile
            .map(|tile| {
                [
                    f32::from(origin.x) * scale,
                    f32::from(origin.y) * scale,
                    f32::from(tile.size.width) * scale,
                    f32::from(tile.size.height) * scale,
                ]
            })
            .unwrap_or([0.0; 4]);
        out.push(tile.map(f32::to_bits));
        let mut average = [0.0; 4];
        if let (Some(first), Some(last)) = (self.stops.first(), self.stops.last()) {
            let span = last.offset - first.offset;
            if span > 0.0 {
                for pair in self.stops.windows(2) {
                    let weight = (pair[1].offset - pair[0].offset) / (2.0 * span);
                    for (i, value) in average.iter_mut().enumerate() {
                        *value += (pair[0].color[i] + pair[1].color[i]) * weight;
                    }
                }
            } else {
                average = last.color;
            }
        }
        out.push(average.map(f32::to_bits));
        debug_assert_eq!(out.len() - index as usize, GRADIENT_HEADER_WORDS);
        for stop in &self.stops {
            out.push([stop.offset.to_bits(), 0, 0, 0]);
            out.push(stop.color.map(f32::to_bits));
        }
        index
    }
}
