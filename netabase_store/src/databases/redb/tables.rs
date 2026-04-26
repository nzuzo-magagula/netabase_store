//! redb table handles over raw byte tables.
//!
//! Every table is `TableDefinition<&[u8], &[u8]>`: keys are the
//! order-preserving encodings (so redb's byte comparator *is* the typed
//! order — zero decode, zero alloc, no panic inside the b-tree), and values
//! are canonical rkyv bytes validated on access.

use crate::errors::{NetabaseError, StorageErrorKind};

use crate::traits::structural::database::tables::codec::{access_value, serialize_value};
use crate::traits::structural::database::tables::{
    StoreTable, TableConfig, TableKey, TableOwner, TableReadOps, TableValue, TableWriteOps,
};
use core::marker::PhantomData;
use core::ops::{Bound, RangeBounds};
use redb::{ReadableMultimapTable, ReadableTable};

/// Key/value byte table definitions.
pub(crate) type BytesTableDef = redb::TableDefinition<'static, &'static [u8], &'static [u8]>;
pub(crate) type BytesMultimapDef =
    redb::MultimapTableDefinition<'static, &'static [u8], &'static [u8]>;

pub struct RedbTableConfig {
    pub name: &'static str,
}

impl TableConfig for RedbTableConfig {
    #[allow(refining_impl_trait)]
    fn table_name(&self) -> &'static str {
        self.name
    }
}

pub(crate) fn encode_key<K: TableKey>(key: &K) -> Result<Vec<u8>, NetabaseError> {
    let mut buf = vec![0u8; K::MAX_ENCODED_LEN];
    let written = key.encode_into(&mut buf)?;
    buf.truncate(written);
    Ok(buf)
}

fn backend_read(err: impl core::error::Error + Send + Sync + 'static) -> NetabaseError {
    NetabaseError::backend(StorageErrorKind::Read, err)
}

fn backend_write(err: impl core::error::Error + Send + Sync + 'static) -> NetabaseError {
    NetabaseError::backend(StorageErrorKind::Write, err)
}

// ── Guard ─────────────────────────────────────────────────────────────────────

/// Zero-copy guard over a stored value.
///
/// Boxes the redb `AccessGuard` so its address is stable, then keeps a
/// validated pointer to the archived form inside the guard's bytes.
pub struct RedbGuard<'g, V: TableValue> {
    /// Owns the storage reference; never touched after construction.
    _guard: Box<redb::AccessGuard<'g, &'static [u8]>>,
    /// Points into `_guard`'s bytes; valid while `_guard` (boxed) lives.
    ptr: *const V::Archived,
}

impl<'g, V: TableValue> RedbGuard<'g, V> {
    pub(crate) fn new(guard: redb::AccessGuard<'g, &'static [u8]>) -> Result<Self, NetabaseError> {
        let boxed = Box::new(guard);
        let archived = access_value::<V>(boxed.value())? as *const V::Archived;
        Ok(Self {
            _guard: boxed,
            ptr: archived,
        })
    }
}

impl<'g, V: TableValue> core::ops::Deref for RedbGuard<'g, V> {
    type Target = V::Archived;
    fn deref(&self) -> &V::Archived {
        // SAFETY: ptr was derived from the boxed guard's bytes, validated at
        // construction; the box pins the guard for self's lifetime and is
        // never mutated.
        unsafe { &*self.ptr }
    }
}

// ── Iterators ────────────────────────────────────────────────────────────────

/// Range iterator over a plain byte table.
pub struct RedbIter<'g, K: TableKey, V: TableValue> {
    inner: Option<redb::Range<'g, &'static [u8], &'static [u8]>>,
    _marker: PhantomData<(K, V)>,
}

impl<'g, K: TableKey, V: TableValue> Iterator for RedbIter<'g, K, V> {
    type Item = Result<(K, RedbGuard<'g, V>), NetabaseError>;

    fn next(&mut self) -> Option<Self::Item> {
        let entry = self.inner.as_mut()?.next()?;
        Some(entry.map_err(backend_read).and_then(|(k, v)| {
            let key = K::decode_exact(k.value())?;
            Ok((key, RedbGuard::new(v)?))
        }))
    }
}

/// Iterator over a multimap: either the values of one key, or a flattened
/// range over (key, value-set) pairs.
pub enum RedbMultimapIter<'g, K: TableKey, V: TableValue> {
    Single {
        key: K,
        values: redb::MultimapValue<'g, &'static [u8]>,
    },
    Range {
        current: Option<(K, redb::MultimapValue<'g, &'static [u8]>)>,
        rest: redb::MultimapRange<'g, &'static [u8], &'static [u8]>,
    },
    Empty(PhantomData<V>),
}

impl<'g, K: TableKey, V: TableValue> Iterator for RedbMultimapIter<'g, K, V> {
    type Item = Result<(K, RedbGuard<'g, V>), NetabaseError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Empty(_) => None,
            Self::Single { key, values } => {
                let value = values.next()?;
                Some(
                    value
                        .map_err(backend_read)
                        .and_then(|v| Ok((key.clone(), RedbGuard::new(v)?))),
                )
            }
            Self::Range { current, rest } => loop {
                if let Some((key, values)) = current.as_mut() {
                    match values.next() {
                        Some(Ok(v)) => {
                            let key = key.clone();
                            return Some(RedbGuard::new(v).map(|g| (key, g)));
                        }
                        Some(Err(e)) => return Some(Err(backend_read(e))),
                        None => *current = None,
                    }
                }
                match rest.next()? {
                    Ok((k, values)) => match K::decode_exact(k.value()) {
                        Ok(key) => *current = Some((key, values)),
                        Err(e) => return Some(Err(e.into())),
                    },
                    Err(e) => return Some(Err(backend_read(e))),
                }
            },
        }
    }
}

/// Convert a typed key range into encoded byte bounds.
fn byte_bounds<K: TableKey>(
    range: impl RangeBounds<K>,
) -> Result<(Bound<Vec<u8>>, Bound<Vec<u8>>), NetabaseError> {
    let map = |b: Bound<&K>| -> Result<Bound<Vec<u8>>, NetabaseError> {
        Ok(match b {
            Bound::Included(k) => Bound::Included(encode_key(k)?),
            Bound::Excluded(k) => Bound::Excluded(encode_key(k)?),
            Bound::Unbounded => Bound::Unbounded,
        })
    };
    Ok((map(range.start_bound())?, map(range.end_bound())?))
}

fn as_slice_bounds(
    bounds: &(Bound<Vec<u8>>, Bound<Vec<u8>>),
) -> (Bound<&[u8]>, Bound<&[u8]>) {
    fn map(b: &Bound<Vec<u8>>) -> Bound<&[u8]> {
        match b {
            Bound::Included(v) => Bound::Included(v.as_slice()),
            Bound::Excluded(v) => Bound::Excluded(v.as_slice()),
            Bound::Unbounded => Bound::Unbounded,
        }
    }
    (map(&bounds.0), map(&bounds.1))
}

// ── Plain tables ─────────────────────────────────────────────────────────────

/// Read-only handle (read transactions).
pub struct RedbReadTable<O, K, V> {
    pub(crate) table: redb::ReadOnlyTable<&'static [u8], &'static [u8]>,
    pub(crate) _marker: PhantomData<(O, K, V)>,
}

impl<O: TableOwner, K: TableKey, V: TableValue> TableReadOps<K, V> for RedbReadTable<O, K, V> {
    type Guard<'g>
        = RedbGuard<'g, V>
    where
        Self: 'g;
    type Iter<'g>
        = RedbIter<'g, K, V>
    where
        Self: 'g;

    fn get<'g>(&'g self, key: &K) -> Result<Option<Self::Guard<'g>>, NetabaseError> {
        let key_bytes = encode_key(key)?;
        match self
            .table
            .get(key_bytes.as_slice())
            .map_err(backend_read)?
        {
            Some(guard) => Ok(Some(RedbGuard::new(guard)?)),
            None => Ok(None),
        }
    }

    fn get_all<'g>(&'g self, key: &K) -> Result<Self::Iter<'g>, NetabaseError> {
        // Plain table: zero or one value — a degenerate inclusive range.
        self.range(key.clone()..=key.clone())
    }

    fn range<'g>(&'g self, range: impl RangeBounds<K>) -> Result<Self::Iter<'g>, NetabaseError> {
        let bounds = byte_bounds(range)?;
        let inner = self
            .table
            .range::<&[u8]>(as_slice_bounds(&bounds))
            .map_err(backend_read)?;
        Ok(RedbIter {
            inner: Some(inner),
            _marker: PhantomData,
        })
    }
}

impl<O: TableOwner, K: TableKey, V: TableValue> StoreTable<O, K, V> for RedbReadTable<O, K, V> {
    type TableConfig = RedbTableConfig;
}

/// Read-write handle (write transactions).
pub struct RedbWriteTable<'txn, O, K, V> {
    pub(crate) table: redb::Table<'txn, &'static [u8], &'static [u8]>,
    pub(crate) _marker: PhantomData<(O, K, V)>,
}

impl<'txn, O: TableOwner, K: TableKey, V: TableValue> TableReadOps<K, V>
    for RedbWriteTable<'txn, O, K, V>
{
    type Guard<'g>
        = RedbGuard<'g, V>
    where
        Self: 'g;
    type Iter<'g>
        = RedbIter<'g, K, V>
    where
        Self: 'g;

    fn get<'g>(&'g self, key: &K) -> Result<Option<Self::Guard<'g>>, NetabaseError> {
        let key_bytes = encode_key(key)?;
        match self
            .table
            .get(key_bytes.as_slice())
            .map_err(backend_read)?
        {
            Some(guard) => Ok(Some(RedbGuard::new(guard)?)),
            None => Ok(None),
        }
    }

    fn get_all<'g>(&'g self, key: &K) -> Result<Self::Iter<'g>, NetabaseError> {
        self.range(key.clone()..=key.clone())
    }

    fn range<'g>(&'g self, range: impl RangeBounds<K>) -> Result<Self::Iter<'g>, NetabaseError> {
        let bounds = byte_bounds(range)?;
        let inner = self
            .table
            .range::<&[u8]>(as_slice_bounds(&bounds))
            .map_err(backend_read)?;
        Ok(RedbIter {
            inner: Some(inner),
            _marker: PhantomData,
        })
    }
}

impl<'txn, O: TableOwner, K: TableKey, V: TableValue> TableWriteOps<K, V>
    for RedbWriteTable<'txn, O, K, V>
{
    fn insert(&mut self, key: &K, value: &V) -> Result<(), NetabaseError> {
        let key_bytes = encode_key(key)?;
        let value_bytes = serialize_value(value)?;
        self.table
            .insert(key_bytes.as_slice(), value_bytes.as_slice())
            .map_err(backend_write)?;
        Ok(())
    }

    fn remove(&mut self, key: &K) -> Result<bool, NetabaseError> {
        let key_bytes = encode_key(key)?;
        Ok(self
            .table
            .remove(key_bytes.as_slice())
            .map_err(backend_write)?
            .is_some())
    }
}

impl<'txn, O: TableOwner, K: TableKey, V: TableValue> StoreTable<O, K, V>
    for RedbWriteTable<'txn, O, K, V>
{
    type TableConfig = RedbTableConfig;
}

// ── Multimap tables ──────────────────────────────────────────────────────────

pub struct RedbReadMultimapTable<O, K, V> {
    pub(crate) table: redb::ReadOnlyMultimapTable<&'static [u8], &'static [u8]>,
    pub(crate) _marker: PhantomData<(O, K, V)>,
}

impl<O: TableOwner, K: TableKey, V: TableValue> TableReadOps<K, V>
    for RedbReadMultimapTable<O, K, V>
{
    type Guard<'g>
        = RedbGuard<'g, V>
    where
        Self: 'g;
    type Iter<'g>
        = RedbMultimapIter<'g, K, V>
    where
        Self: 'g;

    fn get<'g>(&'g self, key: &K) -> Result<Option<Self::Guard<'g>>, NetabaseError> {
        let key_bytes = encode_key(key)?;
        let mut values = self
            .table
            .get(key_bytes.as_slice())
            .map_err(backend_read)?;
        match values.next() {
            Some(v) => Ok(Some(RedbGuard::new(v.map_err(backend_read)?)?)),
            None => Ok(None),
        }
    }

    fn get_all<'g>(&'g self, key: &K) -> Result<Self::Iter<'g>, NetabaseError> {
        let key_bytes = encode_key(key)?;
        let values = self
            .table
            .get(key_bytes.as_slice())
            .map_err(backend_read)?;
        Ok(RedbMultimapIter::Single {
            key: key.clone(),
            values,
        })
    }

    fn range<'g>(&'g self, range: impl RangeBounds<K>) -> Result<Self::Iter<'g>, NetabaseError> {
        let bounds = byte_bounds(range)?;
        let rest = self
            .table
            .range::<&[u8]>(as_slice_bounds(&bounds))
            .map_err(backend_read)?;
        Ok(RedbMultimapIter::Range {
            current: None,
            rest,
        })
    }
}

impl<O: TableOwner, K: TableKey, V: TableValue> StoreTable<O, K, V>
    for RedbReadMultimapTable<O, K, V>
{
    type TableConfig = RedbTableConfig;
}

pub struct RedbWriteMultimapTable<'txn, O, K, V> {
    pub(crate) table: redb::MultimapTable<'txn, &'static [u8], &'static [u8]>,
    pub(crate) _marker: PhantomData<(O, K, V)>,
}

impl<'txn, O: TableOwner, K: TableKey, V: TableValue> TableReadOps<K, V>
    for RedbWriteMultimapTable<'txn, O, K, V>
{
    type Guard<'g>
        = RedbGuard<'g, V>
    where
        Self: 'g;
    type Iter<'g>
        = RedbMultimapIter<'g, K, V>
    where
        Self: 'g;

    fn get<'g>(&'g self, key: &K) -> Result<Option<Self::Guard<'g>>, NetabaseError> {
        let key_bytes = encode_key(key)?;
        let mut values = self
            .table
            .get(key_bytes.as_slice())
            .map_err(backend_read)?;
        match values.next() {
            Some(v) => Ok(Some(RedbGuard::new(v.map_err(backend_read)?)?)),
            None => Ok(None),
        }
    }

    fn get_all<'g>(&'g self, key: &K) -> Result<Self::Iter<'g>, NetabaseError> {
        let key_bytes = encode_key(key)?;
        let values = self
            .table
            .get(key_bytes.as_slice())
            .map_err(backend_read)?;
        Ok(RedbMultimapIter::Single {
            key: key.clone(),
            values,
        })
    }

    fn range<'g>(&'g self, range: impl RangeBounds<K>) -> Result<Self::Iter<'g>, NetabaseError> {
        let bounds = byte_bounds(range)?;
        let rest = self
            .table
            .range::<&[u8]>(as_slice_bounds(&bounds))
            .map_err(backend_read)?;
        Ok(RedbMultimapIter::Range {
            current: None,
            rest,
        })
    }
}

impl<'txn, O: TableOwner, K: TableKey, V: TableValue> TableWriteOps<K, V>
    for RedbWriteMultimapTable<'txn, O, K, V>
{
    fn insert(&mut self, key: &K, value: &V) -> Result<(), NetabaseError> {
        let key_bytes = encode_key(key)?;
        let value_bytes = serialize_value(value)?;
        self.table
            .insert(key_bytes.as_slice(), value_bytes.as_slice())
            .map_err(backend_write)?;
        Ok(())
    }

    fn remove(&mut self, key: &K) -> Result<bool, NetabaseError> {
        let key_bytes = encode_key(key)?;
        let removed = self
            .table
            .remove_all(key_bytes.as_slice())
            .map_err(backend_write)?;
        Ok(removed.count() > 0)
    }

    fn remove_value(&mut self, key: &K, value: &V) -> Result<bool, NetabaseError> {
        let key_bytes = encode_key(key)?;
        let value_bytes = serialize_value(value)?;
        self.table
            .remove(key_bytes.as_slice(), value_bytes.as_slice())
            .map_err(backend_write)
    }
}

impl<'txn, O: TableOwner, K: TableKey, V: TableValue> StoreTable<O, K, V>
    for RedbWriteMultimapTable<'txn, O, K, V>
{
    type TableConfig = RedbTableConfig;
}
