//! Memory-backend table handles: validated owned guards over cloned bytes.

use super::MemoryMap;
use crate::errors::NetabaseError;

use crate::traits::structural::database::tables::codec::{access_value, serialize_value};
use crate::traits::structural::database::tables::{
    StoreTable, TableConfig, TableKey, TableOwner, TableReadOps, TableValue, TableWriteOps,
};
use core::cell::RefCell;
use core::marker::PhantomData;
use core::ops::{Bound, RangeBounds};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemoryTableMode {
    Standard,
    Multimap,
}

pub struct MemoryTableConfig {
    pub name: &'static str,
}

impl TableConfig for MemoryTableConfig {
    #[allow(refining_impl_trait)]
    fn table_name(&self) -> &'static str {
        self.name
    }
}

/// Owned guard: the value bytes are cloned out of the map and validated
/// once; `Deref` hands out the archived view.
pub struct MemoryGuard<V: TableValue> {
    bytes: Box<[u8]>,
    _marker: PhantomData<V>,
}

impl<V: TableValue> MemoryGuard<V> {
    fn new(bytes: &[u8]) -> Result<Self, NetabaseError> {
        // Validate now so Deref can be infallible.
        access_value::<V>(bytes)?;
        Ok(Self {
            bytes: bytes.into(),
            _marker: PhantomData,
        })
    }
}

impl<V: TableValue> core::ops::Deref for MemoryGuard<V> {
    type Target = V::Archived;
    fn deref(&self) -> &V::Archived {
        // Validated at construction; cannot fail here.
        access_value::<V>(&self.bytes).expect("MemoryGuard bytes were validated at construction")
    }
}

/// Owned iterator: rows are collected (cloned) up front, so iteration cannot
/// observe later mutation and needs no map borrow.
pub struct MemoryIter<K, V: TableValue> {
    rows: std::vec::IntoIter<(Vec<u8>, Vec<u8>)>,
    _marker: PhantomData<(K, V)>,
}

impl<K: TableKey, V: TableValue> Iterator for MemoryIter<K, V> {
    type Item = Result<(K, MemoryGuard<V>), NetabaseError>;

    fn next(&mut self) -> Option<Self::Item> {
        let (key_bytes, value_bytes) = self.rows.next()?;
        let item = (|| {
            let key = K::decode_exact(&key_bytes)?;
            Ok::<_, NetabaseError>((key, MemoryGuard::new(&value_bytes)?))
        })();
        Some(item)
    }
}

fn encode_key<K: TableKey>(key: &K) -> Result<Vec<u8>, NetabaseError> {
    let mut buf = vec![0u8; K::MAX_ENCODED_LEN];
    let written = key.encode_into(&mut buf)?;
    buf.truncate(written);
    Ok(buf)
}

/// Shared read logic over any borrowed view of the map.
fn map_get<K: TableKey, V: TableValue>(
    map: &MemoryMap,
    name: &'static str,
    key: &K,
) -> Result<Option<MemoryGuard<V>>, NetabaseError> {
    let key_bytes = encode_key(key)?;
    match map.get(&(name, key_bytes)).and_then(|vals| vals.first()) {
        Some(bytes) => Ok(Some(MemoryGuard::new(bytes)?)),
        None => Ok(None),
    }
}

fn map_get_all<K: TableKey, V: TableValue>(
    map: &MemoryMap,
    name: &'static str,
    key: &K,
) -> Result<MemoryIter<K, V>, NetabaseError> {
    let key_bytes = encode_key(key)?;
    let rows = match map.get(&(name, key_bytes.clone())) {
        Some(vals) => vals
            .iter()
            .map(|v| (key_bytes.clone(), v.clone()))
            .collect::<Vec<_>>(),
        None => Vec::new(),
    };
    Ok(MemoryIter {
        rows: rows.into_iter(),
        _marker: PhantomData,
    })
}

fn map_range<K: TableKey, V: TableValue>(
    map: &MemoryMap,
    name: &'static str,
    range: impl RangeBounds<K>,
) -> Result<MemoryIter<K, V>, NetabaseError> {
    let start: Bound<(&'static str, Vec<u8>)> = match range.start_bound() {
        Bound::Included(k) => Bound::Included((name, encode_key(k)?)),
        Bound::Excluded(k) => Bound::Excluded((name, encode_key(k)?)),
        Bound::Unbounded => Bound::Included((name, Vec::new())),
    };
    let end: Bound<(&'static str, Vec<u8>)> = match range.end_bound() {
        Bound::Included(k) => Bound::Included((name, encode_key(k)?)),
        Bound::Excluded(k) => Bound::Excluded((name, encode_key(k)?)),
        // All keys of this table: rely on the tuple's first component.
        Bound::Unbounded => Bound::Excluded((name_successor(name), Vec::new())),
    };
    let mut rows = Vec::new();
    for ((_, key_bytes), vals) in map.range((start, end)) {
        for v in vals {
            rows.push((key_bytes.clone(), v.clone()));
        }
    }
    Ok(MemoryIter {
        rows: rows.into_iter(),
        _marker: PhantomData,
    })
}

/// A `&'static str` strictly greater than every key beginning with `name`
/// for tuple comparison purposes. Table names are compile-time constants and
/// never empty, so appending `\u{10FFFF}` is a safe upper fence via a leaked
/// one-time allocation per (name) — acceptable for the test-oriented backend.
fn name_successor(name: &'static str) -> &'static str {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static FENCES: OnceLock<Mutex<HashMap<&'static str, &'static str>>> = OnceLock::new();
    let fences = FENCES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut fences = fences.lock().unwrap_or_else(|p| p.into_inner());
    fences.entry(name).or_insert_with(|| {
        let mut s = String::from(name);
        s.push('\u{10FFFF}');
        Box::leak(s.into_boxed_str())
    })
}

// ── Read-only table (over a transaction snapshot) ───────────────────────────

pub struct MemoryReadTable<'txn, O, K, V> {
    pub(crate) map: &'txn MemoryMap,
    pub(crate) name: &'static str,
    pub(crate) _marker: PhantomData<(O, K, V)>,
}

impl<'txn, O: TableOwner, K: TableKey, V: TableValue> TableReadOps<K, V>
    for MemoryReadTable<'txn, O, K, V>
{
    type Guard<'g>
        = MemoryGuard<V>
    where
        Self: 'g;
    type Iter<'g>
        = MemoryIter<K, V>
    where
        Self: 'g;

    fn get<'g>(&'g self, key: &K) -> Result<Option<Self::Guard<'g>>, NetabaseError> {
        map_get(self.map, self.name, key)
    }

    fn get_all<'g>(&'g self, key: &K) -> Result<Self::Iter<'g>, NetabaseError> {
        map_get_all(self.map, self.name, key)
    }

    fn range<'g>(&'g self, range: impl RangeBounds<K>) -> Result<Self::Iter<'g>, NetabaseError> {
        map_range(self.map, self.name, range)
    }
}

impl<'txn, O: TableOwner, K: TableKey, V: TableValue> StoreTable<O, K, V>
    for MemoryReadTable<'txn, O, K, V>
{
    type TableConfig = MemoryTableConfig;
}

// ── Write table (over the transaction's working copy) ───────────────────────

pub struct MemoryWriteTable<'txn, O, K, V> {
    pub(crate) work: &'txn RefCell<MemoryMap>,
    pub(crate) name: &'static str,
    pub(crate) mode: MemoryTableMode,
    pub(crate) _marker: PhantomData<(O, K, V)>,
}

impl<'txn, O: TableOwner, K: TableKey, V: TableValue> TableReadOps<K, V>
    for MemoryWriteTable<'txn, O, K, V>
{
    type Guard<'g>
        = MemoryGuard<V>
    where
        Self: 'g;
    type Iter<'g>
        = MemoryIter<K, V>
    where
        Self: 'g;

    fn get<'g>(&'g self, key: &K) -> Result<Option<Self::Guard<'g>>, NetabaseError> {
        map_get(&self.work.borrow(), self.name, key)
    }

    fn get_all<'g>(&'g self, key: &K) -> Result<Self::Iter<'g>, NetabaseError> {
        map_get_all(&self.work.borrow(), self.name, key)
    }

    fn range<'g>(&'g self, range: impl RangeBounds<K>) -> Result<Self::Iter<'g>, NetabaseError> {
        map_range(&self.work.borrow(), self.name, range)
    }
}

impl<'txn, O: TableOwner, K: TableKey, V: TableValue> TableWriteOps<K, V>
    for MemoryWriteTable<'txn, O, K, V>
{
    fn insert(&mut self, key: &K, value: &V) -> Result<(), NetabaseError> {
        let key_bytes = encode_key(key)?;
        let value_bytes = serialize_value(value)?;
        let mut work = self.work.borrow_mut();
        let slot = work.entry((self.name, key_bytes)).or_default();
        match self.mode {
            MemoryTableMode::Standard => {
                slot.clear();
                slot.push(value_bytes);
            }
            MemoryTableMode::Multimap => {
                // Set semantics: a duplicate pair is a no-op.
                if !slot.contains(&value_bytes) {
                    slot.push(value_bytes);
                    // Keep multimap values deterministically ordered.
                    slot.sort_unstable();
                }
            }
        }
        Ok(())
    }

    fn remove(&mut self, key: &K) -> Result<bool, NetabaseError> {
        let key_bytes = encode_key(key)?;
        Ok(self
            .work
            .borrow_mut()
            .remove(&(self.name, key_bytes))
            .is_some())
    }

    fn remove_value(&mut self, key: &K, value: &V) -> Result<bool, NetabaseError> {
        if self.mode != MemoryTableMode::Multimap {
            return Err(NetabaseError::Unsupported(crate::errors::OpKind::Multimap));
        }
        let key_bytes = encode_key(key)?;
        let value_bytes = serialize_value(value)?;
        let mut work = self.work.borrow_mut();
        let Some(slot) = work.get_mut(&(self.name, key_bytes.clone())) else {
            return Ok(false);
        };
        let before = slot.len();
        slot.retain(|v| *v != value_bytes);
        let removed = slot.len() != before;
        if slot.is_empty() {
            work.remove(&(self.name, key_bytes));
        }
        Ok(removed)
    }
}

impl<'txn, O: TableOwner, K: TableKey, V: TableValue> StoreTable<O, K, V>
    for MemoryWriteTable<'txn, O, K, V>
{
    type TableConfig = MemoryTableConfig;
}
