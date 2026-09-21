use std::{
    borrow::{Borrow, Cow},
    ops::Deref,
    sync::Arc,
};

/// Cheaply cloned immutable text. Static strings do not allocate.
#[derive(Clone, Debug)]
pub struct SharedString(Storage);
#[derive(Clone, Debug)]
enum Storage {
    Static(&'static str),
    Shared(Arc<str>),
}
impl SharedString {
    pub fn new(value: impl AsRef<str>) -> Self {
        Self(Storage::Shared(Arc::from(value.as_ref())))
    }
    pub const fn new_static(value: &'static str) -> Self {
        Self(Storage::Static(value))
    }
    pub fn as_str(&self) -> &str {
        match &self.0 {
            Storage::Static(s) => s,
            Storage::Shared(s) => s,
        }
    }
}
impl Default for SharedString {
    fn default() -> Self {
        Self::new_static("")
    }
}
impl Deref for SharedString {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}
impl AsRef<str> for SharedString {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
impl Borrow<str> for SharedString {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}
impl From<&str> for SharedString {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}
impl From<String> for SharedString {
    fn from(s: String) -> Self {
        Self(Storage::Shared(s.into()))
    }
}
impl From<Arc<str>> for SharedString {
    fn from(s: Arc<str>) -> Self {
        Self(Storage::Shared(s))
    }
}
impl From<Cow<'_, str>> for SharedString {
    fn from(s: Cow<'_, str>) -> Self {
        Self::new(s)
    }
}
impl std::fmt::Display for SharedString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.as_str().fmt(f)
    }
}

// Equality, ordering and hashing must depend on text, not the storage variant:
// Borrow<str> hash-map lookups rely on this invariant.
impl PartialEq for SharedString {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}
impl Eq for SharedString {}
impl std::hash::Hash for SharedString {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.as_str().hash(h);
    }
}
impl PartialOrd for SharedString {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for SharedString {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl From<&SharedString> for SharedString {
    fn from(s: &SharedString) -> Self {
        s.clone()
    }
}
