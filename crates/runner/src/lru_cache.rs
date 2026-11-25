use std::{
    borrow::Borrow,
    collections::HashMap,
    hash::Hash,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

struct LruEntry<V> {
    value: V,
    last_used: AtomicU64,
    inserted_at: Instant,
}

/// Bounded map with lock-free LRU recency tracking and optional TTL expiry.
///
/// Reads (`get` and `iter`) work under an `RwLock::read()` guard because recency
/// timestamps use an `AtomicU64`. On capacity pressure, expired entries are purged
/// first and then the oldest batch (~12.5%) is evicted instead of clearing the map.
pub(crate) struct BoundedLruCache<K, V> {
    map: HashMap<K, LruEntry<V>>,
    max_entries: usize,
    ttl: Option<Duration>,
    clock: AtomicU64,
}

impl<K, V> BoundedLruCache<K, V> {
    pub(crate) fn new(max_entries: usize) -> Self {
        Self {
            map: HashMap::new(),
            max_entries: max_entries.max(1),
            ttl: None,
            clock: AtomicU64::new(1),
        }
    }

    pub(crate) fn with_ttl(max_entries: usize, ttl: Duration) -> Self {
        Self {
            map: HashMap::new(),
            max_entries: max_entries.max(1),
            ttl: Some(ttl),
            clock: AtomicU64::new(1),
        }
    }

    fn is_expired(&self, entry: &LruEntry<V>, now: Instant) -> bool {
        self.ttl
            .is_some_and(|ttl| now.saturating_duration_since(entry.inserted_at) >= ttl)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        let now = Instant::now();
        self.map
            .iter()
            .filter(move |(_, entry)| !self.is_expired(entry, now))
            .map(|(k, entry)| (k, &entry.value))
    }
}

impl<K: Eq + Hash, V> BoundedLruCache<K, V> {
    pub(crate) fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: ?Sized + Eq + Hash,
    {
        let entry = self.map.get(key)?;
        if self.is_expired(entry, Instant::now()) {
            return None;
        }
        let stamp = self.clock.fetch_add(1, Ordering::Relaxed);
        entry.last_used.store(stamp, Ordering::Relaxed);
        Some(&entry.value)
    }

    pub(crate) fn clear(&mut self) {
        self.map.clear();
        if self.map.capacity() > self.max_entries.saturating_mul(2) {
            self.map.shrink_to(self.max_entries.min(64));
        }
    }

    pub(crate) fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: ?Sized + Eq + Hash,
    {
        self.map.remove(key).map(|entry| entry.value)
    }

    pub(crate) fn retain<F>(&mut self, mut keep: F)
    where
        F: FnMut(&K) -> bool,
    {
        self.map.retain(|k, _| keep(k));
    }

    pub(crate) fn insert(&mut self, key: K, value: V) {
        let now = Instant::now();
        if self.ttl.is_some() && self.map.len() >= self.max_entries {
            let ttl = self.ttl;
            self.map.retain(|_, entry| {
                ttl.is_none_or(|limit| now.saturating_duration_since(entry.inserted_at) < limit)
            });
        }
        if !self.map.contains_key(&key) && self.map.len() >= self.max_entries {
            self.evict_oldest_batch();
        }
        let stamp = self.clock.fetch_add(1, Ordering::Relaxed);
        self.map.insert(
            key,
            LruEntry {
                value,
                last_used: AtomicU64::new(stamp),
                inserted_at: now,
            },
        );
    }

    fn evict_oldest_batch(&mut self) {
        let len = self.map.len();
        if len == 0 {
            return;
        }
        let evict_target = (self.max_entries / 8).max(1).min(len);
        let mut stamps: Vec<u64> = self
            .map
            .values()
            .map(|entry| entry.last_used.load(Ordering::Relaxed))
            .collect();
        let nth = evict_target - 1;
        let (_, &mut cutoff, _) = stamps.select_nth_unstable(nth);
        self.map
            .retain(|_, entry| entry.last_used.load(Ordering::Relaxed) > cutoff);
        // Handle tie where many entries shared `cutoff` so at least one slot opens.
        while self.map.len() >= self.max_entries {
            self.map.retain(|_, entry| {
                entry.last_used.load(Ordering::Relaxed) >= cutoff.saturating_add(1)
            });
            if self.map.len() >= self.max_entries {
                self.map.clear();
                break;
            }
        }
        if self.map.capacity() > self.max_entries.saturating_mul(2) {
            self.map.shrink_to(self.max_entries);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_lru_evicts_least_recently_used_and_preserves_hot_keys() {
        let mut cache = BoundedLruCache::new(4);
        cache.insert("k1", 1);
        cache.insert("k2", 2);
        cache.insert("k3", 3);
        cache.insert("k4", 4);
        assert_eq!(cache.get("k1"), Some(&1));
        cache.insert("k5", 5);
        assert_eq!(cache.get("k2"), None);
        assert_eq!(cache.get("k1"), Some(&1));
        assert_eq!(cache.get("k5"), Some(&5));
    }

    #[test]
    fn bounded_lru_with_ttl_hides_and_reclaims_expired_entries() {
        let mut cache = BoundedLruCache::with_ttl(4, Duration::from_millis(15));
        cache.insert("stale", 42);
        std::thread::sleep(Duration::from_millis(25));
        assert_eq!(cache.get("stale"), None);
        assert_eq!(cache.iter().count(), 0);
        cache.insert("fresh", 99);
        assert_eq!(cache.get("fresh"), Some(&99));
    }
}
