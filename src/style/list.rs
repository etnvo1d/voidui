//! Immutable style lists: empty values allocate nothing, clones share their storage.
use std::{ops::Deref, sync::Arc};

#[derive(Debug, Clone, PartialEq)]
pub struct StyleList<T>(Option<Arc<[T]>>);
impl<T> Default for StyleList<T> {
    fn default() -> Self {
        Self(None)
    }
}
impl<T> Deref for StyleList<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        self.0.as_deref().unwrap_or(&[])
    }
}
impl<T> From<Vec<T>> for StyleList<T> {
    fn from(values: Vec<T>) -> Self {
        Self((!values.is_empty()).then(|| values.into()))
    }
}
impl<T, const N: usize> From<[T; N]> for StyleList<T> {
    fn from(values: [T; N]) -> Self {
        Vec::from(values).into()
    }
}

impl<T> StyleList<T> {
    /// Identity check for caches of immutable CSS data; no element scan is needed.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}
impl<T> From<Vec<T>> for super::value::CssValue<StyleList<T>> {
    fn from(value: Vec<T>) -> Self {
        Self::Value(value.into())
    }
}
impl<T, const N: usize> From<[T; N]> for super::value::CssValue<StyleList<T>> {
    fn from(value: [T; N]) -> Self {
        Self::Value(value.into())
    }
}
