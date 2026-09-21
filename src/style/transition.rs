//! CSS transition timing, independent of the event loop and GPU.
#![doc = include_str!("../../docs/effects.md")]
use super::list::StyleList;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepPosition {
    Start,
    End,
    JumpNone,
    JumpBoth,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Easing {
    Linear,
    CubicBezier(f64, f64, f64, f64),
    Steps(u32, StepPosition),
    /// CSS Easing Level 2 piecewise-linear input/output stops.
    PiecewiseLinear(StyleList<(f64, f64)>),
}
impl Default for Easing {
    fn default() -> Self {
        Self::EASE
    }
}
impl Easing {
    pub const EASE: Self = Self::CubicBezier(0.25, 0.1, 0.25, 1.0);
    pub const EASE_IN: Self = Self::CubicBezier(0.42, 0.0, 1.0, 1.0);
    pub const EASE_OUT: Self = Self::CubicBezier(0.0, 0.0, 0.58, 1.0);
    pub const EASE_IN_OUT: Self = Self::CubicBezier(0.42, 0.0, 0.58, 1.0);

    pub fn is_valid(&self) -> bool {
        match self {
            Self::Linear => true,
            Self::CubicBezier(x1, y1, x2, y2) => {
                (0.0..=1.0).contains(x1)
                    && (0.0..=1.0).contains(x2)
                    && y1.is_finite()
                    && y2.is_finite()
            }
            Self::Steps(n, position) => *n > 0 && (*n > 1 || *position != StepPosition::JumpNone),
            Self::PiecewiseLinear(points) => {
                points.len() >= 2
                    && points.iter().all(|(x, y)| x.is_finite() && y.is_finite())
                    && points.windows(2).all(|w| w[0].0 <= w[1].0)
            }
        }
    }
    /// Evaluate CSS easing. `before` selects the left side of a step boundary,
    /// as required while a transition is waiting in its positive delay.
    pub fn evaluate(&self, x: f64, before: bool) -> f64 {
        match self {
            Self::Linear => x,
            Self::CubicBezier(x1, y1, x2, y2) => {
                let (x1, y1, x2, y2) = (*x1, *y1, *x2, *y2);
                if x <= 0.0 {
                    return x * if x1 > 0.0 {
                        y1 / x1
                    } else if x2 > 0.0 {
                        y2 / x2
                    } else {
                        0.0
                    };
                }
                if x >= 1.0 {
                    return 1.0
                        + (x - 1.0)
                            * if x2 < 1.0 {
                                (1.0 - y2) / (1.0 - x2)
                            } else if x1 < 1.0 {
                                (1.0 - y1) / (1.0 - x1)
                            } else {
                                0.0
                            };
                }
                let cubic = |t: f64, a: f64, b: f64| {
                    3.0 * (1.0 - t).powi(2) * t * a + 3.0 * (1.0 - t) * t * t * b + t * t * t
                };
                let (mut lo, mut hi) = (0.0, 1.0);
                // Monotonic x permits bounded bisection, including flat tangents.
                // 40 iterations give substantially more precision than f32 styles.
                for _ in 0..40 {
                    let t = (lo + hi) * 0.5;
                    if cubic(t, x1, x2) < x {
                        lo = t;
                    } else {
                        hi = t;
                    }
                }
                cubic((lo + hi) * 0.5, y1, y2)
            }
            Self::Steps(n, position) => {
                let (n, position) = (*n, *position);
                let scaled = x * f64::from(n);
                let mut step = scaled.floor();
                if matches!(position, StepPosition::Start | StepPosition::JumpBoth) {
                    step += 1.0;
                }
                if before && scaled == scaled.floor() {
                    step -= 1.0;
                }
                let jumps = f64::from(n)
                    + match position {
                        StepPosition::JumpBoth => 1.0,
                        StepPosition::JumpNone => -1.0,
                        _ => 0.0,
                    };
                if x >= 0.0 {
                    step = step.max(0.0);
                }
                if x <= 1.0 {
                    step = step.min(jumps);
                }
                step / jumps
            }
            Self::PiecewiseLinear(points) => {
                let hi = points
                    .partition_point(|(position, _)| *position < x || (*position == x && !before));
                let (a, b) = if hi == 0 {
                    (points[0], points[1])
                } else if hi == points.len() {
                    (points[hi - 2], points[hi - 1])
                } else {
                    (points[hi - 1], points[hi])
                };
                if a.0 == b.0 {
                    if before { a.1 } else { b.1 }
                } else {
                    a.1 + (b.1 - a.1) * (x - a.0) / (b.0 - a.0)
                }
            }
        }
    }
}

/// A named longhand, shorthand, `all`, or an unknown CSS property. Unknown names
/// are retained for CSS list matching but cannot start an animation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionProperty(pub String);
impl From<&str> for TransitionProperty {
    fn from(value: &str) -> Self {
        Self(value.to_ascii_lowercase())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Transition {
    pub property: TransitionProperty,
    pub duration: Duration,
    /// Seconds; negative delay starts the animation partway through its duration.
    pub delay: f64,
    pub easing: Easing,
}
impl Transition {
    pub fn new(property: impl Into<TransitionProperty>, duration: Duration) -> Self {
        Self {
            property: property.into(),
            duration,
            delay: 0.0,
            easing: Easing::default(),
        }
    }
    pub fn delay(mut self, seconds: f64) -> Self {
        assert!(seconds.is_finite(), "transition delay must be finite");
        self.delay = seconds;
        self
    }
    pub fn easing(mut self, easing: Easing) -> Self {
        assert!(easing.is_valid(), "invalid transition easing");
        self.easing = easing;
        self
    }
}

/// Resolved transition longhands. Empty lists use the CSS initial values:
/// all / 0s / 0s / ease. `none` is represented by a named property.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TransitionStyle {
    pub properties: StyleList<TransitionProperty>,
    pub durations: StyleList<f64>,
    pub delays: StyleList<f64>,
    pub easing: StyleList<Easing>,
}
impl TransitionStyle {
    pub(crate) fn enabled(&self) -> bool {
        !self.properties.iter().any(|p| p.0 == "none")
            && (self.durations.iter().any(|v| *v > 0.0) || self.delays.iter().any(|v| *v > 0.0))
    }
    pub(crate) fn timing(&self, name: &str) -> Option<(f64, f64, Easing)> {
        let matches = |property: &str| {
            property == "all"
                || property
                    .bytes()
                    .eq(name.bytes().map(|b| if b == b'_' { b'-' } else { b }))
                || match property {
                    "background" => name == "background-color",
                    "border" => name.starts_with("border-") || name.starts_with("border_"),
                    "border-width" => {
                        (name.starts_with("border-") || name.starts_with("border_"))
                            && (name.ends_with("-width") || name.ends_with("_width"))
                    }
                    "margin" => name.starts_with("margin_"),
                    "padding" => name.starts_with("padding_"),
                    "gap" => matches!(name, "row_gap" | "column_gap"),
                    "inset" => matches!(name, "top" | "right" | "bottom" | "left"),
                    _ => false,
                }
        };
        let index = if self.properties.is_empty() {
            0
        } else {
            self.properties.iter().rposition(|p| matches(&p.0))?
        };
        let duration = self
            .durations
            .get(index % self.durations.len().max(1))
            .copied()
            .unwrap_or(0.0);
        let delay = self
            .delays
            .get(index % self.delays.len().max(1))
            .copied()
            .unwrap_or(0.0);
        let easing = self
            .easing
            .get(index % self.easing.len().max(1))
            .cloned()
            .unwrap_or_default();
        Some((duration, delay, easing))
    }
}
impl From<Transition> for StyleList<Transition> {
    fn from(value: Transition) -> Self {
        vec![value].into()
    }
}
