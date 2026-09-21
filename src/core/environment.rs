//! Typed dependencies retained along the logical component ancestry.
use super::state::Subscribers;
use std::{
    any::{Any, TypeId, type_name},
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
};

#[derive(Default)]
struct Entry {
    value: Option<Rc<dyn Any>>,
    readers: Subscribers,
    published: bool,
}

/// A scope belongs to one mounted component, including components with no DOM node.
/// Missing lookups subscribe too, so adding a nearer provider reconnects readers.
pub(crate) struct ContextScope {
    parent: Option<Rc<Self>>,
    // Components that never provide or resolve context keep no map allocation.
    #[allow(clippy::box_collection)]
    entries: RefCell<Option<Box<HashMap<TypeId, Entry>>>>,
}
impl ContextScope {
    pub(crate) fn new(parent: Option<Rc<Self>>) -> Rc<Self> {
        Rc::new(Self {
            parent,
            entries: Default::default(),
        })
    }
    pub(crate) fn begin(&self) {
        if let Some(entries) = self.entries.borrow_mut().as_mut() {
            for entry in entries.values_mut() {
                entry.published = false;
            }
        }
    }
    pub(crate) fn finish(&self) {
        let mut storage = self.entries.borrow_mut();
        let Some(entries) = storage.as_mut() else {
            return;
        };
        entries.retain(|_, entry| {
            if !entry.published && entry.value.take().is_some() {
                entry.readers.notify_context();
            }
            entry.value.is_some() || entry.readers.is_active()
        });
    }
    fn provide<T: Clone + PartialEq + 'static>(&self, value: T) {
        let mut entries = self.entries.borrow_mut();
        let entry = entries
            .get_or_insert_with(Default::default)
            .entry(TypeId::of::<T>())
            .or_default();
        assert!(
            !entry.published,
            "context {} was provided twice in one component",
            type_name::<T>()
        );
        entry.published = true;
        if entry.value.as_ref().and_then(|old| old.downcast_ref::<T>()) == Some(&value) {
            return;
        }
        entry.value = Some(Rc::new(value));
        entry.readers.notify_context();
    }
    fn lookup<T: Clone + 'static>(&self) -> Option<T> {
        let mut ancestor = self.parent.clone();
        while let Some(scope) = ancestor {
            let value = {
                let mut entries = scope.entries.borrow_mut();
                let entry = entries
                    .get_or_insert_with(Default::default)
                    .entry(TypeId::of::<T>())
                    .or_default();
                entry.readers.track();
                entry.value.clone()
            };
            if let Some(value) = value {
                return Some(
                    value
                        .downcast_ref::<T>()
                        .expect("context type identity")
                        .clone(),
                );
            }
            ancestor = scope.parent.clone();
        }
        None
    }
}

/// Publish a dependency to descendants of the current component. Call once per
/// type on each render where it should be available. Equal values preserve memo
/// skips; changing or removing a provider reconnects its subscribed descendants.
/// Prefer domain types containing State handles over undifferentiated booleans.
pub fn provide_context<T: Clone + PartialEq + 'static>(value: T) {
    super::state::context_scope().provide(value);
}

/// Resolve the nearest ancestor provider, or return None. Call during rendering
/// and capture the returned handle in callbacks. Lookup tracks provider changes;
/// reading a State inside the returned value tracks that State separately.
pub fn try_context<T: Clone + 'static>() -> Option<T> {
    super::state::context_scope().lookup()
}

/// Resolve a required ancestor dependency. Panics with its type name if missing.
pub fn use_context<T: Clone + 'static>() -> T {
    try_context().unwrap_or_else(|| {
        panic!(
            "missing context {}; add an ancestor provider",
            type_name::<T>()
        )
    })
}
