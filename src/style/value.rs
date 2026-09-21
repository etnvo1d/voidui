//! Typed CSS values and numeric conveniences for the fluent style API.
use crate::core::layout::{LengthPercentage, LengthPercentageAuto};
use taffy::Dimension;

/// Explicit CSS defaulting for supported inherited properties.
/// `Unset` (also the default) follows the property's normal inheritance rule.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum CssValue<T> {
    Value(T),
    Inherit,
    Initial,
    #[default]
    Unset,
}

impl<T> From<T> for CssValue<T> {
    fn from(value: T) -> Self {
        Self::Value(value)
    }
}

impl<T: Clone> CssValue<T> {
    /// Resolve defaulting before converting relative units into computed values.
    pub fn resolve(&self, parent: &T, initial: &T, inherited: bool) -> T {
        match self {
            Self::Value(value) => value.clone(),
            Self::Inherit => parent.clone(),
            Self::Initial => initial.clone(),
            Self::Unset => {
                if inherited {
                    parent.clone()
                } else {
                    initial.clone()
                }
            }
        }
    }
}

/// Accept logical-pixel numbers and the property's own Taffy length type.
/// The target type keeps `auto()` invalid for padding and valid for margins.
pub trait IntoCssLength<T> {
    fn into_css_length(self) -> T;
}

impl<T> IntoCssLength<T> for T {
    fn into_css_length(self) -> T {
        self
    }
}

macro_rules! numeric_lengths {
    ($($ty:ty),* $(,)?) => { $(
        impl IntoCssLength<Dimension> for $ty {
            fn into_css_length(self) -> Dimension { Dimension::length(self as f32) }
        }
        impl IntoCssLength<LengthPercentage> for $ty {
            fn into_css_length(self) -> LengthPercentage { LengthPercentage::length(self as f32) }
        }
        impl IntoCssLength<LengthPercentageAuto> for $ty {
            fn into_css_length(self) -> LengthPercentageAuto { LengthPercentageAuto::length(self as f32) }
        }
    )* };
}
numeric_lengths!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize, f32, f64);

/// A percentage input for setters: `pct(50.0)` means 50%.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Percent(pub f32);

pub const fn pct(percentage: f32) -> Percent {
    Percent(percentage)
}

#[derive(Debug, Clone, Copy)]
pub struct Auto;
/// Auto sizing or margins. Intentionally cannot be converted to padding.
pub const AUTO: Auto = Auto;

impl IntoCssLength<Dimension> for Percent {
    fn into_css_length(self) -> Dimension {
        Dimension::percent(self.0 / 100.0)
    }
}
impl IntoCssLength<LengthPercentage> for Percent {
    fn into_css_length(self) -> LengthPercentage {
        LengthPercentage::percent(self.0 / 100.0)
    }
}
impl IntoCssLength<LengthPercentageAuto> for Percent {
    fn into_css_length(self) -> LengthPercentageAuto {
        LengthPercentageAuto::percent(self.0 / 100.0)
    }
}
impl IntoCssLength<Dimension> for Auto {
    fn into_css_length(self) -> Dimension {
        Dimension::auto()
    }
}
impl IntoCssLength<LengthPercentageAuto> for Auto {
    fn into_css_length(self) -> LengthPercentageAuto {
        LengthPercentageAuto::auto()
    }
}

/// Share validation across individual edges and their shorthand setters.
pub(crate) trait ValidateCssLength: Copy {
    fn numeric(self) -> Option<f32>;
    fn calc_handle(self) -> Option<*const ()>;
    fn validate(self, property: &str, signed: bool) -> Self {
        if let Some(value) = self.numeric() {
            assert!(
                value.is_finite() && (signed || value >= 0.0),
                "{property} must be finite{}",
                if signed { "" } else { " and nonnegative" }
            );
        }
        self
    }
}
impl ValidateCssLength for LengthPercentage {
    fn numeric(self) -> Option<f32> {
        use crate::core::layout::ExpandedLengthPercentage::*;
        match self.expand() {
            Length(v) | Percent(v) => Some(v),
            Calc(_) => None,
        }
    }
    fn calc_handle(self) -> Option<*const ()> {
        match self.expand() {
            crate::core::layout::ExpandedLengthPercentage::Calc(handle) => Some(handle),
            _ => None,
        }
    }
}
impl ValidateCssLength for LengthPercentageAuto {
    fn numeric(self) -> Option<f32> {
        match self.expand() {
            crate::core::layout::ExpandedLengthPercentageAuto::Length(v)
            | crate::core::layout::ExpandedLengthPercentageAuto::Percent(v) => Some(v),
            _ => None,
        }
    }
    fn calc_handle(self) -> Option<*const ()> {
        match self.expand() {
            crate::core::layout::ExpandedLengthPercentageAuto::Calc(handle) => Some(handle),
            _ => None,
        }
    }
}
impl ValidateCssLength for Dimension {
    fn numeric(self) -> Option<f32> {
        if self.is_auto() || self.is_sizing_keyword() || self.calc_handle().is_some() {
            None
        } else {
            Some(self.value())
        }
    }
    fn calc_handle(self) -> Option<*const ()> {
        match self.expand() {
            crate::core::layout::ExpandedDimension::Calc(handle) => Some(handle),
            _ => None,
        }
    }
}

impl From<i32> for CssValue<f32> {
    fn from(value: i32) -> Self {
        Self::Value(value as f32)
    }
}
impl From<u32> for CssValue<f32> {
    fn from(value: u32) -> Self {
        Self::Value(value as f32)
    }
}
impl From<f64> for CssValue<f32> {
    fn from(value: f64) -> Self {
        Self::Value(value as f32)
    }
}
