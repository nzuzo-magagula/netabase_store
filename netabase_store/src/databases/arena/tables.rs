//! Arena table handles: inline (heap-free) guards over slab records.
//!
//! Reads copy a record's value bytes into a fixed inline buffer and validate
//! them, so a guard owns its bytes — no borrow of the slab escapes, which lets
//! the same guard type serve both the immutable read-tx slab and the
//! `RefCell`-guarded write-tx work slab.

use super::region;
use super::region::{KEY_MAX, VAL_MAX};
use super::region_slab::{WorkSlab, encode_key_inline, serialize_val_inline};
use crate::errors::NetabaseError;
use crate::traits::structural::database::tables::codec::access_value;
use crate::traits::structural::database::tables::{
    StoreTable, TableConfig, TableKey, TableOwner, TableReadOps, TableValue, TableWriteOps,
};
use core::cell::{Ref, RefCell};
use core::marker::PhantomData;
use core::ops::{Bound, RangeBounds};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArenaTableMode {
    Standard,
    Multimap,
}

pub struct ArenaTableConfig {
    pub name: &'static str,
}

impl TableConfig for ArenaTableConfig {
    #[allow(refining_impl_trait)]
    fn table_name(&self) -> &'static str {
        self.name
    }
}

/// An owned, validated view of one stored value (inline bytes, no heap).
pub struct ArenaGuard<V: TableValue> {
    bytes: [u8; VAL_MAX],
    len: usize,
    _marker: PhantomData<V>,
}

impl<V: TableValue> ArenaGuard<V> {
    fn new(value_bytes: &[u8]) -> Result<Self, NetabaseError> {
        // Validate now so Deref is infallible.
        access_value::<V>(value_bytes)?;
        let mut bytes = [0u8; VAL_MAX];
        bytes[..value_bytes.len()].copy_from_slice(value_bytes);
        Ok(Self {
            bytes,
            len: value_bytes.len(),
            _marker: PhantomData,
        })
    }
}

impl<V: TableValue> core::ops::Deref for ArenaGuard<V> {
    type Target = V::Archived;
    fn deref(&self) -> &V::Archived {
        access_value::<V>(&self.bytes[..self.len])
            .expect("ArenaGuard bytes were validated at construction")
    }
}

// ── Range bound specification (inline, no heap) ─────────────────────────────

struct BoundSpec {
    key: [u8; KEY_MAX],
    len: usize,
    kind: BoundKind,
}

#[derive(Clone, Copy, PartialEq)]
enum BoundKind {
    Unbounded,
    Included,
    Excluded,
}

fn bound_spec<K: TableKey>(b: Bound<&K>) -> Result<BoundSpec, NetabaseError> {
    Ok(match b {
        Bound::Unbounded => BoundSpec {
            key: [0u8; KEY_MAX],
            len: 0,
            kind: BoundKind::Unbounded,
        },
        Bound::Included(k) => {
            let (key, len) = encode_key_inline(k)?;
            BoundSpec {
                key,
                len,
                kind: BoundKind::Included,
            }
        }
        Bound::Excluded(k) => {
            let (key, len) = encode_key_inline(k)?;
            BoundSpec {
                key,
                len,
                kind: BoundKind::Excluded,
            }
        }
    })
}

impl BoundSpec {
    fn enc(&self) -> &[u8] {
        &self.key[..self.len]
    }
    fn lo_ok(&self, key: &[u8]) -> bool {
        match self.kind {
            BoundKind::Unbounded => true,
            BoundKind::Included => key >= self.enc(),
            BoundKind::Excluded => key > self.enc(),
        }
    }
    /// Returns whether `key` is below the high bound; the boolean is "continue".
    fn hi_ok(&self, key: &[u8]) -> bool {
        match self.kind {
            BoundKind::Unbounded => true,
            BoundKind::Included => key <= self.enc(),
            BoundKind::Excluded => key < self.enc(),
        }
    }
}

/// Decode one record at index `i` into `(K, guard)`.
fn read_record<K: TableKey, V: TableValue>(
    slab: &[u8],
    i: usize,
) -> Result<(K, ArenaGuard<V>), NetabaseError> {
    let key = K::decode_exact(region::rec_key(slab, i))?;
    let guard = ArenaGuard::new(region::rec_val(slab, i))?;
    Ok((key, guard))
}

/// Shared iteration cursor over a slab: walks records of one table within a
/// key range. The slab bytes are supplied per `next` so the same logic serves
/// both borrow shapes.
struct Cursor {
    name: &'static str,
    idx: usize,
    lo: BoundSpec,
    hi: BoundSpec,
    /// When `Some`, restrict to exactly this key (the get_all case).
    exact: Option<([u8; KEY_MAX], usize)>,
}

impl Cursor {
    fn for_key(name: &'static str, key: &[u8]) -> Self {
        let mut buf = [0u8; KEY_MAX];
        buf[..key.len()].copy_from_slice(key);
        Self {
            name,
            idx: 0,
            lo: BoundSpec {
                key: [0u8; KEY_MAX],
                len: 0,
                kind: BoundKind::Unbounded,
            },
            hi: BoundSpec {
                key: [0u8; KEY_MAX],
                len: 0,
                kind: BoundKind::Unbounded,
            },
            exact: Some((buf, key.len())),
        }
    }

    fn for_range(name: &'static str, lo: BoundSpec, hi: BoundSpec) -> Self {
        Self {
            name,
            idx: 0,
            lo,
            hi,
            exact: None,
        }
    }

    /// Position `idx` at the first candidate record. Call once before iterating.
    fn seed(&mut self, slab: &[u8]) {
        let name = self.name.as_bytes();
        self.idx = match &self.exact {
            Some((k, n)) => region::lower_bound(slab, name, &k[..*n]),
            None => match self.lo.kind {
                BoundKind::Unbounded => region::lower_bound_name(slab, name),
                _ => region::lower_bound(slab, name, self.lo.enc()),
            },
        };
    }

    /// Index of the next matching record, advancing `idx` past it.
    fn next_idx(&mut self, slab: &[u8]) -> Option<usize> {
        let name = self.name.as_bytes();
        loop {
            if self.idx >= region::count(slab) {
                return None;
            }
            if region::rec_name(slab, self.idx) != name {
                return None;
            }
            let key = region::rec_key(slab, self.idx);
            if let Some((k, n)) = &self.exact {
                if key != &k[..*n] {
                    return None;
                }
                let i = self.idx;
                self.idx += 1;
                return Some(i);
            }
            // Range case.
            if !self.hi.hi_ok(key) {
                return None; // sorted → nothing further qualifies
            }
            if self.lo.lo_ok(key) {
                let i = self.idx;
                self.idx += 1;
                return Some(i);
            }
            self.idx += 1;
        }
    }
}

// ── Read-table iterator (immutable base slab) ───────────────────────────────

pub struct ArenaReadIter<'g, K, V> {
    slab: &'g [u8],
    cursor: Cursor,
    _marker: PhantomData<(K, V)>,
}

impl<'g, K: TableKey, V: TableValue> Iterator for ArenaReadIter<'g, K, V> {
    type Item = Result<(K, ArenaGuard<V>), NetabaseError>;
    fn next(&mut self) -> Option<Self::Item> {
        let i = self.cursor.next_idx(self.slab)?;
        Some(read_record::<K, V>(self.slab, i))
    }
}

// ── Write-table iterator (RefCell-guarded work slab) ────────────────────────

pub struct ArenaWriteIter<'g, K, V> {
    slab: Ref<'g, WorkSlab<'g>>,
    cursor: Cursor,
    _marker: PhantomData<(K, V)>,
}

impl<'g, K: TableKey, V: TableValue> Iterator for ArenaWriteIter<'g, K, V> {
    type Item = Result<(K, ArenaGuard<V>), NetabaseError>;
    fn next(&mut self) -> Option<Self::Item> {
        let i = self.cursor.next_idx(self.slab.0)?;
        Some(read_record::<K, V>(self.slab.0, i))
    }
}

// ── Read-only table over the base slab ──────────────────────────────────────

pub struct ArenaReadTable<'txn, O, K, V> {
    pub(crate) slab: &'txn [u8],
    pub(crate) name: &'static str,
    pub(crate) _marker: PhantomData<(O, K, V)>,
}

impl<'txn, O: TableOwner, K: TableKey, V: TableValue> TableReadOps<K, V>
    for ArenaReadTable<'txn, O, K, V>
{
    type Guard<'g>
        = ArenaGuard<V>
    where
        Self: 'g;
    type Iter<'g>
        = ArenaReadIter<'g, K, V>
    where
        Self: 'g;

    fn get<'g>(&'g self, key: &K) -> Result<Option<Self::Guard<'g>>, NetabaseError> {
        let (kb, kn) = encode_key_inline(key)?;
        match region::find_first(self.slab, self.name.as_bytes(), &kb[..kn]) {
            Some(i) => Ok(Some(ArenaGuard::new(region::rec_val(self.slab, i))?)),
            None => Ok(None),
        }
    }

    fn get_all<'g>(&'g self, key: &K) -> Result<Self::Iter<'g>, NetabaseError> {
        let (kb, kn) = encode_key_inline(key)?;
        let mut cursor = Cursor::for_key(self.name, &kb[..kn]);
        cursor.seed(self.slab);
        Ok(ArenaReadIter {
            slab: self.slab,
            cursor,
            _marker: PhantomData,
        })
    }

    fn range<'g>(&'g self, range: impl RangeBounds<K>) -> Result<Self::Iter<'g>, NetabaseError> {
        let lo = bound_spec(range.start_bound())?;
        let hi = bound_spec(range.end_bound())?;
        let mut cursor = Cursor::for_range(self.name, lo, hi);
        cursor.seed(self.slab);
        Ok(ArenaReadIter {
            slab: self.slab,
            cursor,
            _marker: PhantomData,
        })
    }
}

impl<'txn, O: TableOwner, K: TableKey, V: TableValue> StoreTable<O, K, V>
    for ArenaReadTable<'txn, O, K, V>
{
    type TableConfig = ArenaTableConfig;
}

// ── Read/write table over the work slab ─────────────────────────────────────

pub struct ArenaWriteTable<'txn, 'buf, O, K, V> {
    pub(crate) work: &'txn RefCell<WorkSlab<'buf>>,
    pub(crate) name: &'static str,
    pub(crate) mode: ArenaTableMode,
    pub(crate) _marker: PhantomData<(O, K, V)>,
}

impl<'txn, 'buf, O: TableOwner, K: TableKey, V: TableValue> TableReadOps<K, V>
    for ArenaWriteTable<'txn, 'buf, O, K, V>
{
    type Guard<'g>
        = ArenaGuard<V>
    where
        Self: 'g;
    type Iter<'g>
        = ArenaWriteIter<'g, K, V>
    where
        Self: 'g;

    fn get<'g>(&'g self, key: &K) -> Result<Option<Self::Guard<'g>>, NetabaseError> {
        let (kb, kn) = encode_key_inline(key)?;
        let slab = self.work.borrow();
        match region::find_first(slab.0, self.name.as_bytes(), &kb[..kn]) {
            Some(i) => Ok(Some(ArenaGuard::new(region::rec_val(slab.0, i))?)),
            None => Ok(None),
        }
    }

    fn get_all<'g>(&'g self, key: &K) -> Result<Self::Iter<'g>, NetabaseError> {
        let (kb, kn) = encode_key_inline(key)?;
        let mut cursor = Cursor::for_key(self.name, &kb[..kn]);
        let slab = self.work.borrow();
        cursor.seed(slab.0);
        Ok(ArenaWriteIter {
            slab,
            cursor,
            _marker: PhantomData,
        })
    }

    fn range<'g>(&'g self, range: impl RangeBounds<K>) -> Result<Self::Iter<'g>, NetabaseError> {
        let lo = bound_spec(range.start_bound())?;
        let hi = bound_spec(range.end_bound())?;
        let mut cursor = Cursor::for_range(self.name, lo, hi);
        let slab = self.work.borrow();
        cursor.seed(slab.0);
        Ok(ArenaWriteIter {
            slab,
            cursor,
            _marker: PhantomData,
        })
    }
}

impl<'txn, 'buf, O: TableOwner, K: TableKey, V: TableValue> TableWriteOps<K, V>
    for ArenaWriteTable<'txn, 'buf, O, K, V>
{
    fn insert(&mut self, key: &K, value: &V) -> Result<(), NetabaseError> {
        let (kb, kn) = encode_key_inline(key)?;
        let (vb, vn) = serialize_val_inline(value)?;
        let mut slab = self.work.borrow_mut();
        let name = self.name.as_bytes();
        match self.mode {
            ArenaTableMode::Standard => {
                region::put_unique(slab.0, name, &kb[..kn], &vb[..vn], self.name)
            }
            ArenaTableMode::Multimap => {
                region::put_multi(slab.0, name, &kb[..kn], &vb[..vn], self.name)
            }
        }
    }

    fn remove(&mut self, key: &K) -> Result<bool, NetabaseError> {
        let (kb, kn) = encode_key_inline(key)?;
        let mut slab = self.work.borrow_mut();
        Ok(region::remove_key(slab.0, self.name.as_bytes(), &kb[..kn]))
    }

    fn remove_value(&mut self, key: &K, value: &V) -> Result<bool, NetabaseError> {
        if self.mode != ArenaTableMode::Multimap {
            return Err(region::unsupported(crate::errors::OpKind::Multimap));
        }
        let (kb, kn) = encode_key_inline(key)?;
        let (vb, vn) = serialize_val_inline(value)?;
        let mut slab = self.work.borrow_mut();
        Ok(region::remove_pair(
            slab.0,
            self.name.as_bytes(),
            &kb[..kn],
            &vb[..vn],
        ))
    }
}

impl<'txn, 'buf, O: TableOwner, K: TableKey, V: TableValue> StoreTable<O, K, V>
    for ArenaWriteTable<'txn, 'buf, O, K, V>
{
    type TableConfig = ArenaTableConfig;
}
