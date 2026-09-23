//! Byte- and entry-bounded retention for resources that active views may also own.
use std::{
    collections::{BTreeMap, HashMap},
    hash::Hash,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheBudget {
    pub max_bytes: usize,
    pub max_entries: usize,
}
impl Default for CacheBudget {
    fn default() -> Self {
        Self {
            max_bytes: 16 * 1024 * 1024,
            max_entries: 1024,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub entries: usize,
    pub retained_bytes: usize,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
}
/// LRU ownership with caller-supplied resource costs. Eviction drops only the
/// cache's reference: active views remain valid. Include keys and native payloads
/// in the supplied cost when they dominate memory; allocator overhead is excluded.
pub struct BudgetCache<K, V> {
    budget: CacheBudget,
    entries: HashMap<K, (V, usize, u64)>,
    order: BTreeMap<u64, K>,
    clock: u64,
    stats: CacheStats,
}
impl<K: Clone + Eq + Hash, V> BudgetCache<K, V> {
    pub fn new(budget: CacheBudget) -> Self {
        Self {
            budget,
            entries: HashMap::new(),
            order: BTreeMap::new(),
            clock: 0,
            stats: CacheStats::default(),
        }
    }
    pub fn get(&mut self, key: &K) -> Option<&V> {
        let Some((value, _, age)) = self.entries.get_mut(key) else {
            self.stats.misses += 1;
            return None;
        };
        self.order.remove(age);
        self.clock += 1;
        *age = self.clock;
        self.order.insert(*age, key.clone());
        self.stats.hits += 1;
        Some(value)
    }
    pub fn insert(&mut self, key: K, value: V, bytes: usize) {
        self.remove(&key);
        // An oversized resource can be owned by its view without flushing every
        // other cached resource or violating the retention budget.
        if bytes > self.budget.max_bytes || self.budget.max_entries == 0 {
            return;
        }
        self.clock += 1;
        self.order.insert(self.clock, key.clone());
        self.entries.insert(key, (value, bytes, self.clock));
        self.stats.retained_bytes += bytes;
        self.trim();
    }
    pub fn remove(&mut self, key: &K) -> Option<V> {
        let (value, bytes, age) = self.entries.remove(key)?;
        self.order.remove(&age);
        self.stats.retained_bytes -= bytes;
        Some(value)
    }
    pub fn set_budget(&mut self, budget: CacheBudget) {
        self.budget = budget;
        self.trim();
    }
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            entries: self.entries.len(),
            ..self.stats
        }
    }
    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
        self.stats.retained_bytes = 0;
    }
    fn trim(&mut self) {
        while self.entries.len() > self.budget.max_entries
            || self.stats.retained_bytes > self.budget.max_bytes
        {
            let Some((_, key)) = self.order.pop_first() else {
                break;
            };
            self.remove(&key);
            self.stats.evictions += 1;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn evicts_by_bytes_and_recency_without_clearing_working_set() {
        let mut cache = BudgetCache::new(CacheBudget {
            max_bytes: 10,
            max_entries: 2,
        });
        cache.insert(1, "a", 4);
        cache.insert(2, "b", 4);
        assert_eq!(cache.get(&1), Some(&"a"));
        cache.insert(3, "c", 4);
        assert!(cache.get(&2).is_none());
        assert!(cache.get(&1).is_some());
        cache.insert(4, "oversized", 11);
        assert_eq!(cache.stats().entries, 2);
        cache.set_budget(CacheBudget {
            max_bytes: 0,
            max_entries: 0,
        });
        assert_eq!(cache.stats().retained_bytes, 0);
    }
}
