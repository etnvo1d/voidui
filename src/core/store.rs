//! Key-indexed state. Updating a value visits that key's readers, not every row.
use super::{
    data::Read,
    state::{self, State, Subscribers},
};
use std::{cell::RefCell, collections::HashMap, hash::Hash};

struct Data<K, V> {
    values: RefCell<HashMap<K, Read<V>>>,
    readers: RefCell<HashMap<K, Subscribers>>,
    structure: RefCell<Subscribers>,
}

/// A Copy handle to a component-owned map. Reads subscribe by key, including
/// missing keys. keys/len subscribe only to insertions and removals.
pub struct Store<K, V> {
    data: State<Data<K, V>>,
}
impl<K, V> Copy for Store<K, V> {}
impl<K, V> Clone for Store<K, V> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<K, V> PartialEq for Store<K, V> {
    fn eq(&self, other: &Self) -> bool {
        self.data.same_state(&other.data)
    }
}
impl<K, V> Eq for Store<K, V> {}

/// Create a keyed collection once per mount. Supply ordinary owned values;
/// values need not implement Clone. Call in a stable hook order, like state().
pub fn store<K, V, I>(init: impl FnOnce() -> I) -> Store<K, V>
where
    K: Eq + Hash + Clone + 'static,
    V: 'static,
    I: IntoIterator<Item = (K, V)>,
{
    Store {
        data: state::state(|| Data {
            values: RefCell::new(init().into_iter().map(|(k, v)| (k, Read::new(v))).collect()),
            readers: RefCell::default(),
            structure: RefCell::default(),
        }),
    }
}
impl<K: Eq + Hash + Clone + 'static, V: 'static> Store<K, V> {
    pub fn is_mounted(&self) -> bool {
        self.data.is_mounted()
    }
    /// Borrow one entry without copying it. An absent entry is also a dependency.
    pub fn with<R>(&self, key: &K, read: impl FnOnce(Option<&V>) -> R) -> R {
        self.read(key, |value| read(value.map(AsRef::as_ref)))
    }
    fn read<R>(&self, key: &K, read: impl FnOnce(Option<&Read<V>>) -> R) -> R {
        self.data.with_untracked(|data| {
            if state::is_rendering() {
                let mut readers = data.readers.borrow_mut();
                // Prune only before growth, so event reads allocate nothing and
                // repeated reads of mounted rows do not scan unrelated keys.
                if let Some(subscribers) = readers.get_mut(key) {
                    subscribers.track();
                } else {
                    if readers.len() == readers.capacity() {
                        readers.retain(|_, subscribers| subscribers.is_active());
                    }
                    readers.entry(key.clone()).or_default().track();
                }
            }
            read(data.values.borrow().get(key))
        })
    }
    /// Retain an immutable snapshot without cloning V. Later replacements do not
    /// change old snapshots; snapshots may outlive the store deliberately.
    pub fn get(&self, key: &K) -> Option<Read<V>> {
        self.read(key, |value| value.cloned())
    }
    pub fn contains_key(&self, key: &K) -> bool {
        self.with(key, |value| value.is_some())
    }
    pub fn len(&self) -> usize {
        self.data.with_untracked(|data| {
            data.structure.borrow_mut().track();
            data.values.borrow().len()
        })
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Collect current keys. Ordering follows HashMap; sort explicitly if needed.
    pub fn keys(&self) -> Vec<K> {
        self.data.with_untracked(|data| {
            data.structure.borrow_mut().track();
            data.values.borrow().keys().cloned().collect()
        })
    }
    pub fn insert(&self, key: K, value: V) -> Option<Read<V>> {
        if !self.is_mounted() {
            return None;
        }
        state::assert_not_rendering();
        self.data.with_untracked(|data| {
            let old = data
                .values
                .borrow_mut()
                .insert(key.clone(), Read::new(value));
            Self::notify(data, &key, old.is_none());
            old
        })
    }
    pub fn remove(&self, key: &K) -> Option<Read<V>> {
        if !self.is_mounted() {
            return None;
        }
        state::assert_not_rendering();
        self.data.with_untracked(|data| {
            let old = data.values.borrow_mut().remove(key);
            if old.is_some() {
                Self::notify(data, key, true);
            }
            old
        })
    }
    /// Edit one entry. If an older snapshot is retained elsewhere, clone only
    /// this value before editing; otherwise mutate in place without allocating.
    /// Return false without calling the closure when absent or unmounted.
    pub fn update(&self, key: &K, update: impl FnOnce(&mut V)) -> bool
    where
        V: Clone,
    {
        if !self.is_mounted() {
            return false;
        }
        state::assert_not_rendering();
        self.data.with_untracked(|data| {
            struct Notify<'a, K: Eq + Hash + Clone + 'static, V: 'static> {
                data: &'a Data<K, V>,
                key: &'a K,
                changed: std::cell::Cell<bool>,
            }
            impl<K: Eq + Hash + Clone + 'static, V: 'static> Drop for Notify<'_, K, V> {
                fn drop(&mut self) {
                    if self.changed.get() {
                        Store::notify(self.data, self.key, false);
                    }
                }
            }
            // Declared before the borrow so even a panicking edit releases the
            // value first, then notifies readers of any partial change.
            let notify = Notify {
                data,
                key,
                changed: std::cell::Cell::new(false),
            };
            let mut values = data.values.borrow_mut();
            let Some(value) = values.get_mut(key) else {
                return false;
            };
            notify.changed.set(true);
            update(value.make_mut());
            true
        })
    }
    /// Replace a changed entry only. Comparing values is opt-in; ordinary insert
    /// never performs an implicit deep equality check on a large object.
    pub fn set_if_changed(&self, key: K, value: V) -> bool
    where
        V: PartialEq,
    {
        if !self.is_mounted() {
            return false;
        }
        state::assert_not_rendering();
        if self.data.with_untracked(|data| {
            data.values
                .borrow()
                .get(&key)
                .is_some_and(|old| **old == value)
        }) {
            return false;
        }
        self.insert(key, value);
        true
    }
    fn notify(data: &Data<K, V>, key: &K, structure: bool) {
        let mut readers = data.readers.borrow_mut();
        if let Some(subscribers) = readers.get_mut(key) {
            subscribers.notify();
            if !subscribers.is_active() {
                readers.remove(key);
            }
        }
        drop(readers);
        if structure {
            data.structure.borrow_mut().notify();
        }
    }
}

/// Single selection with per-key subscriptions. Changing selection only notifies
/// the old/new selected rows plus components explicitly reading the selected key.
pub struct Selection<K> {
    selected: State<Option<K>>,
    members: Store<K, ()>,
}
impl<K> Copy for Selection<K> {}
impl<K> Clone for Selection<K> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<K> PartialEq for Selection<K> {
    fn eq(&self, other: &Self) -> bool {
        self.selected.same_state(&other.selected)
    }
}
impl<K> Eq for Selection<K> {}
pub fn selection<K: Eq + Hash + Clone + 'static>() -> Selection<K> {
    Selection {
        selected: state::state(|| None),
        members: store(std::iter::empty),
    }
}
impl<K: Eq + Hash + Clone + 'static> Selection<K> {
    pub fn is_selected(&self, key: &K) -> bool {
        self.members.contains_key(key)
    }
    pub fn get(&self) -> Option<K> {
        self.selected.get()
    }
    pub fn select(&self, key: K) {
        self.set(Some(key));
    }
    pub fn clear(&self) {
        self.set(None);
    }
    fn set(&self, next: Option<K>) {
        if !self.selected.is_mounted() {
            return;
        }
        state::assert_not_rendering();
        let old = self.selected.with_untracked(Clone::clone);
        if old == next {
            return;
        }
        if let Some(old) = old {
            self.members.remove(&old);
        }
        if let Some(key) = &next {
            self.members.insert(key.clone(), ());
        }
        self.selected.set(next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{component, core::widget_tree::WidgetTree, div};
    use std::{cell::Cell, rc::Rc};

    #[test]
    fn inactive_missing_keys_are_pruned_and_event_reads_do_not_subscribe() {
        let key = Rc::new(Cell::new(0));
        let slot = Rc::new(RefCell::new(None));
        let input = key.clone();
        let output = slot.clone();
        let mut tree = WidgetTree::new();
        tree.build_root(component(move || {
            let values = store(|| std::iter::empty::<(usize, ())>());
            *output.borrow_mut() = Some(values);
            div().width(usize::from(values.contains_key(&input.get())) as f32)
        }));
        let values = slot.borrow().unwrap();
        for i in 0..10_000 {
            values.get(&i);
        }
        let keys = || {
            values
                .data
                .with_untracked(|data| data.readers.borrow().len())
        };
        assert_eq!(keys(), 1, "event reads must not add missing-key channels");
        for i in 1..10_000 {
            // Notify the previous dependency, then render a different missing key.
            values.insert(key.get(), ());
            values.remove(&key.get());
            key.set(i);
            tree.flush_updates();
        }
        assert!(keys() < 32, "inactive key channels must not accumulate");
    }
}
