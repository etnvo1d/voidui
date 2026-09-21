//! Immutable inputs with explicit, inexpensive snapshot ownership.
use std::{fmt, ops::Deref, sync::Arc};

/// An immutable, owned snapshot. Cloning shares its storage without cloning T.
/// Component parameters declared as Read<T> also accept plain T at the call site.
/// Equality compares snapshot identity, not potentially expensive model contents.
pub struct Read<T: ?Sized>(Arc<T>);
impl<T> Read<T> {
    pub fn new(value: T) -> Self {
        Self(Arc::new(value))
    }
    pub(crate) fn make_mut(&mut self) -> &mut T
    where
        T: Clone,
    {
        Arc::make_mut(&mut self.0)
    }
}
impl<T: ?Sized> Clone for Read<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T: ?Sized> Deref for Read<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}
impl<T: ?Sized> AsRef<T> for Read<T> {
    fn as_ref(&self) -> &T {
        self
    }
}
impl<T: ?Sized> PartialEq for Read<T> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl<T: ?Sized> Eq for Read<T> {}
impl<T: ?Sized + fmt::Debug> fmt::Debug for Read<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl<T: ?Sized + fmt::Display> fmt::Display for Read<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl<T> From<T> for Read<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}
impl<T: ?Sized> From<Arc<T>> for Read<T> {
    fn from(value: Arc<T>) -> Self {
        Self(value)
    }
}
impl<T: ?Sized> From<&Read<T>> for Read<T> {
    fn from(value: &Read<T>) -> Self {
        value.clone()
    }
}
impl From<&str> for Read<String> {
    fn from(value: &str) -> Self {
        Self::new(value.to_owned())
    }
}

/// An immutable list whose items can independently outlive the list.
/// Build from ordinary values once; iteration yields inexpensive owned snapshots.
/// Each item has its own allocation, so retaining one item does not retain siblings.
pub struct List<T>(Arc<[Read<T>]>);
impl<T> List<T> {
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn get(&self, index: usize) -> Option<Read<T>> {
        self.0.get(index).cloned()
    }
    pub fn iter(&self) -> impl ExactSizeIterator<Item = Read<T>> + DoubleEndedIterator + '_ {
        self.0.iter().cloned()
    }
}
impl<T> Clone for List<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T> Default for List<T> {
    fn default() -> Self {
        Self(Arc::from([]))
    }
}
impl<T> PartialEq for List<T> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl<T> Eq for List<T> {}
impl<T: fmt::Debug> fmt::Debug for List<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl<T> FromIterator<T> for List<T> {
    fn from_iter<I: IntoIterator<Item = T>>(values: I) -> Self {
        Self(values.into_iter().map(Read::new).collect())
    }
}
impl<T> From<Vec<T>> for List<T> {
    fn from(values: Vec<T>) -> Self {
        values.into_iter().collect()
    }
}
impl<T, const N: usize> From<[T; N]> for List<T> {
    fn from(values: [T; N]) -> Self {
        values.into_iter().collect()
    }
}
impl<T> From<&List<T>> for List<T> {
    fn from(values: &List<T>) -> Self {
        values.clone()
    }
}

/// Clone named captures into a callback without introducing temporary variables.
/// Only the listed values are cloned, once when the callback is constructed.
///
/// ```
/// use voidui::{capture, component, div, state};
/// let view = component(|| {
///     let count = state(|| 0);
///     div().on_click(capture!(count => move || count.update(|n| *n += 1)))
/// });
/// ```
#[macro_export]
macro_rules! capture {
    ($($name:ident),+ $(,)? => $body:expr) => {{
        $(let $name = ::core::clone::Clone::clone(&$name);)+
        $body
    }};
}
