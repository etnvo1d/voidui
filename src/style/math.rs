//! Owned layout expressions and the bridge to Taffy's opaque calculation handles.
use std::{
    cell::RefCell,
    collections::HashMap,
    rc::{Rc, Weak},
};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Expression {
    Pixels(f64),
    Percent(f64),
    Sum(Box<Self>, Box<Self>),
    Scale(Box<Self>, f64),
    Min(Vec<Self>),
    Max(Vec<Self>),
}

impl Expression {
    pub(crate) fn bounds(&self) -> Option<(f64, f64)> {
        // Bound intermediate arithmetic for every finite layout basis, including
        // negative free space. Reject overflow before it can introduce NaN into layout.
        let (low, high) = match self {
            Self::Pixels(n) => (*n, *n),
            Self::Percent(n) => (-n.abs() * f32::MAX as f64, n.abs() * f32::MAX as f64),
            Self::Sum(a, b) => {
                let (al, ah) = a.bounds()?;
                let (bl, bh) = b.bounds()?;
                (al + bl, ah + bh)
            }
            Self::Scale(a, n) => {
                let (low, high) = a.bounds()?;
                if *n >= 0.0 {
                    (low * n, high * n)
                } else {
                    (high * n, low * n)
                }
            }
            Self::Min(values) | Self::Max(values) => {
                let mut low = f64::INFINITY;
                let mut high = f64::NEG_INFINITY;
                for value in values {
                    let (a, b) = value.bounds()?;
                    low = low.min(a);
                    high = high.max(b);
                }
                (low, high)
            }
        };
        (low.is_finite() && high.is_finite()).then_some((low, high))
    }

    pub(crate) fn evaluate(&self, basis: f64) -> f64 {
        match self {
            Self::Pixels(n) => *n,
            Self::Percent(n) => n * basis,
            Self::Sum(a, b) => a.evaluate(basis) + b.evaluate(basis),
            Self::Scale(a, n) => a.evaluate(basis) * n,
            Self::Min(values) => values
                .iter()
                .map(|v| v.evaluate(basis))
                .fold(f64::INFINITY, f64::min),
            Self::Max(values) => values
                .iter()
                .map(|v| v.evaluate(basis))
                .fold(f64::NEG_INFINITY, f64::max),
        }
    }

    pub(crate) fn has_percentage(&self) -> bool {
        match self {
            Self::Pixels(_) => false,
            Self::Percent(_) => true,
            Self::Sum(a, b) => a.has_percentage() || b.has_percentage(),
            Self::Scale(a, _) => a.has_percentage(),
            Self::Min(v) | Self::Max(v) => v.iter().any(Self::has_percentage),
        }
    }
}

#[derive(Default)]
struct Registry {
    next: usize,
    values: HashMap<usize, Weak<Registered>>,
}

thread_local! {
    // Styles and Taffy lengths are thread-local. Weak entries never keep old
    // stylesheets alive, and integer handles are never dereferenced as pointers.
    static REGISTRY: RefCell<Registry> = RefCell::default();
}

#[derive(Debug)]
struct Registered {
    handle: usize,
    expression: Expression,
    nonnegative: bool,
}

impl Drop for Registered {
    fn drop(&mut self) {
        let _ = REGISTRY.try_with(|registry| {
            registry.borrow_mut().values.remove(&self.handle);
        });
    }
}

/// A stylesheet-owned calculation. Cloning keeps its layout handle valid across
/// cascade, inheritance, and stylesheet replacement. Raw Taffy length copies do
/// not own expressions: retain the originating Style or ComputedStyle with them.
#[derive(Debug, Clone)]
pub struct MathLength(Rc<Registered>);

impl PartialEq for MathLength {
    fn eq(&self, other: &Self) -> bool {
        self.0.nonnegative == other.0.nonnegative && self.0.expression == other.0.expression
    }
}

impl MathLength {
    pub(crate) fn retain(handle: *const ()) -> Self {
        Self(
            REGISTRY
                .with(|registry| {
                    registry
                        .borrow()
                        .values
                        .get(&(handle as usize))
                        .and_then(Weak::upgrade)
                })
                .expect("CSS math handle has no live owner on this thread"),
        )
    }

    pub(crate) fn new(expression: Expression, nonnegative: bool) -> Self {
        REGISTRY.with(|registry| {
            let mut registry = registry.borrow_mut();
            // Taffy reserves three tag bits. Never reuse an expired handle.
            registry.next = registry
                .next
                .checked_add(8)
                .expect("CSS math handle space exhausted");
            let value = Rc::new(Registered {
                handle: registry.next,
                expression,
                nonnegative,
            });
            registry.values.insert(value.handle, Rc::downgrade(&value));
            Self(value)
        })
    }

    pub(crate) fn expression(&self) -> &Expression {
        &self.0.expression
    }

    pub(crate) fn handle(&self) -> *const () {
        std::ptr::without_provenance(self.0.handle)
    }
}

/// Resolve a VoidUI-owned Taffy calculation against the current percentage basis.
/// Custom layout trees using stylesheet lengths should forward their
/// `LayoutPartialTree::resolve_calc_value` method to this function.
pub fn resolve_calc(handle: *const (), basis: f32) -> f32 {
    let value = MathLength::retain(handle).0;
    let result = value.expression.evaluate(basis.into());
    // CSS clamps restricted properties after evaluating the entire expression;
    // intermediate negative values (including margins) must remain meaningful.
    let minimum = if value.nonnegative {
        0.0
    } else {
        -(f32::MAX as f64)
    };
    result.clamp(minimum, f32::MAX as f64) as f32
}

macro_rules! retain_lengths {
    (layout_setters { $($plain:tt)* } optional_setters { $($optional:tt)* }
     length_setters { $($(#[$doc:meta])* $prop:ident => $name:ident($ty:ty), $signed:literal => $($field:ident).+;)* }) => {
        pub(crate) fn retain_layout(layout: &crate::core::layout::LayoutStyle) -> Vec<(super::properties::LayoutProperty, MathLength)> {
            use super::value::ValidateCssLength;
            let mut values = Vec::new();
            $(if let Some(handle) = layout.$($field).+.calc_handle() {
                values.push((super::properties::LayoutProperty::$prop, MathLength::retain(handle)));
            })*
            values
        }
    };
}
super::properties::layout_properties!(retain_lengths);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_follow_owners_without_leaking_registry_entries() {
        let value = MathLength::new(Expression::Percent(0.5), true);
        let handle = value.handle();
        let copy = value.clone();
        drop(value);
        assert_eq!(resolve_calc(handle, 200.0), 100.0);
        drop(copy);
        REGISTRY
            .with(|registry| assert!(!registry.borrow().values.contains_key(&(handle as usize))));
    }

    #[test]
    fn copied_lengths_and_temporary_keywords_preserve_their_owners() {
        use crate::style::{
            css::properties::parse_property,
            properties::{LayoutKeyword, LayoutProperty},
            style::Style,
        };
        let mut style = Style::default();
        for declaration in parse_property("width", "calc(50% + 10px)").unwrap() {
            declaration.apply(&mut style);
        }
        style.set_layout_keyword(LayoutProperty::Width, LayoutKeyword::Initial);
        style.clear_layout_keywords();
        let width = style.layout.size.width;
        let copy = crate::div().width(width);
        let preset = crate::div().layout_style(style.layout.clone());
        let handle = style.math_lengths[0].1.handle();
        drop(style);
        assert_eq!(resolve_calc(handle, 200.0), 110.0);
        let copy = copy.width(width);
        drop(preset);
        assert_eq!(resolve_calc(handle, 400.0), 210.0);
        let _literal = copy.width(20.0);
        REGISTRY
            .with(|registry| assert!(!registry.borrow().values.contains_key(&(handle as usize))));
    }

    #[test]
    fn inherited_computed_style_keeps_expression_after_source_styles_are_dropped() {
        use crate::{
            core::layout::ExpandedDimension,
            style::{computed::ComputedStyle, css::properties::parse_property, style::Style},
        };
        let mut parent = Style::default();
        for declaration in parse_property("width", "calc(100% - 20px)").unwrap() {
            declaration.apply(&mut parent);
        }
        let computed_parent =
            ComputedStyle::resolve_in_layer(&parent, &ComputedStyle::default(), false);
        let mut child = Style::default();
        for declaration in parse_property("width", "inherit").unwrap() {
            declaration.apply(&mut child);
        }
        let computed_child = ComputedStyle::resolve_in_layer(&child, &computed_parent, false);
        let ExpandedDimension::Calc(handle) = computed_child.layout.size.width.expand() else {
            panic!("expected an inherited expression");
        };
        drop(parent);
        drop(computed_parent);
        drop(child);
        assert_eq!(resolve_calc(handle, 200.0), 180.0);
        let copy = computed_child.clone();
        drop(computed_child);
        assert_eq!(resolve_calc(handle, 300.0), 280.0);
        drop(copy);
        REGISTRY
            .with(|registry| assert!(!registry.borrow().values.contains_key(&(handle as usize))));
    }
}
