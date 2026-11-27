//! Fixed-capacity FIFO deduplication sets and maps for long-lived menu bar sessions.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    hash::Hash,
};

#[derive(Debug)]
pub struct BoundedSet<T> {
    capacity: usize,
    set: HashSet<T>,
    order: VecDeque<T>,
}

impl<T: Eq + Hash + Clone> BoundedSet<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        let cap = capacity.max(1);
        Self {
            capacity: cap,
            set: HashSet::with_capacity(cap),
            order: VecDeque::with_capacity(cap),
        }
    }

    /// Inserts `value`. Returns `false` if already present, `true` if newly inserted.
    pub fn insert(&mut self, value: T) -> bool {
        if self.set.contains(&value) {
            return false;
        }
        if self.order.len() >= self.capacity
            && let Some(oldest) = self.order.pop_front()
        {
            self.set.remove(&oldest);
        }
        self.order.push_back(value.clone());
        self.set.insert(value);
        true
    }

    #[cfg(test)]
    pub fn contains(&self, value: &T) -> bool {
        self.set.contains(value)
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.set.len()
    }
}

#[derive(Debug)]
pub struct BoundedMap<K, V> {
    capacity: usize,
    map: HashMap<K, V>,
    order: VecDeque<K>,
}

impl<K: Eq + Hash + Clone, V> BoundedMap<K, V> {
    pub fn with_capacity(capacity: usize) -> Self {
        let cap = capacity.max(1);
        Self {
            capacity: cap,
            map: HashMap::with_capacity(cap),
            order: VecDeque::with_capacity(cap),
        }
    }

    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: std::borrow::Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.map.get(key)
    }

    pub fn insert(&mut self, key: K, value: V) {
        if let Some(existing) = self.map.get_mut(&key) {
            *existing = value;
            return;
        }
        if self.order.len() >= self.capacity
            && let Some(oldest) = self.order.pop_front()
        {
            self.map.remove(&oldest);
        }
        self.order.push_back(key.clone());
        self.map.insert(key, value);
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.map.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_set_evicts_oldest_entries_at_capacity() {
        let mut set = BoundedSet::with_capacity(3);
        assert!(set.insert("r1".to_owned()));
        assert!(!set.insert("r1".to_owned()));
        assert!(set.insert("r2".to_owned()));
        assert!(set.insert("r3".to_owned()));
        assert_eq!(set.len(), 3);

        assert!(set.insert("r4".to_owned()));
        assert_eq!(set.len(), 3);
        assert!(!set.contains(&"r1".to_owned()));
        assert!(set.contains(&"r2".to_owned()));
        assert!(set.contains(&"r4".to_owned()));
    }

    #[test]
    fn bounded_map_updates_in_place_and_evicts_oldest() {
        let mut map = BoundedMap::with_capacity(2);
        map.insert("a".to_owned(), 1);
        map.insert("b".to_owned(), 2);
        map.insert("a".to_owned(), 10);
        assert_eq!(map.len(), 2);
        assert_eq!(map.get(&"a".to_owned()), Some(&10));

        map.insert("c".to_owned(), 3);
        assert_eq!(map.len(), 2);
        assert_eq!(map.get(&"a".to_owned()), None);
        assert_eq!(map.get(&"b".to_owned()), Some(&2));
        assert_eq!(map.get(&"c".to_owned()), Some(&3));
    }
}
