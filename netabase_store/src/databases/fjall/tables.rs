//! fjall table handles over keyspaces of raw bytes.
//!
//! Keys are the order-preserving encodings, so fjall's lexicographic LSM
//! iteration *is* the typed order — `prefix`/`range` scans need no decode.
//! Values are canonical rkyv bytes, validated on access.
//!
//! fjall has no native multimap, so a multimap table composes the key as
//! `enc(K) ++ value_bytes` with an empty stored value: `get_all(K)` is a
//! prefix scan over `enc(K)`, decoding `K` from the prefix and the value from
//! the suffix.

use crate::errors::{CodecErrorKind, NetabaseError, StorageErrorKind};
use crate::traits::structural::database::tables::codec::{access_value, serialize_value};
use crate::traits::structural::database::tables::{
    StoreTable, TableConfig, TableKey, TableOwner, TableReadOps, TableValue, TableWriteOps,
};
use core::cell::RefCell;
use core::marker::PhantomData;
use core::ops::{Bound, RangeBounds};
use fjall::{Readable, SingleWriterTxKeyspace, SingleWriterWriteTx, Slice, Snapshot};

pub struct FjallTableConfig {
    pub name: &'static str,
}

impl TableConfig for FjallTableConfig {
    #[allow(refining_impl_trait)]
    fn table_name(&self) -> &'static str {
        self.name
    }
}

fn backend_read(err: impl core::error::Error + Send + Sync + 'static) -> NetabaseError {
    NetabaseError::backend(StorageErrorKind::Read, err)
}

fn corruption() -> NetabaseError {
    NetabaseError::Codec(CodecErrorKind::Corruption)
}

pub(crate) fn encode_key<K: TableKey>(key: &K) -> Result<Vec<u8>, NetabaseError> {
    let mut buf = vec![0u8; K::MAX_ENCODED_LEN];
    let n = key.encode_into(&mut buf)?;
    buf.truncate(n);
    Ok(buf)
}

/// Composed multimap entry key: `enc(K) ++ serialize(V)`.
fn compose_key<K: TableKey, V: TableValue>(key: &K, value: &V) -> Result<Vec<u8>, NetabaseError> {
    let mut buf = encode_key(key)?;
    buf.extend_from_slice(&serialize_value(value)?);
    Ok(buf)
}

// ── Guard ─────────────────────────────────────────────────────────────────────

/// Validated owned view of a stored value.
pub struct FjallGuard<V: TableValue> {
    bytes: Box<[u8]>,
    _marker: PhantomData<V>,
}

impl<V: TableValue> FjallGuard<V> {
    fn new(bytes: &[u8]) -> Result<Self, NetabaseError> {
        access_value::<V>(bytes)?;
        Ok(Self {
            bytes: bytes.into(),
            _marker: PhantomData,
        })
    }
}

impl<V: TableValue> core::ops::Deref for FjallGuard<V> {
    type Target = V::Archived;
    fn deref(&self) -> &V::Archived {
        access_value::<V>(&self.bytes).expect("FjallGuard bytes validated at construction")
    }
}

// ── Iterators ────────────────────────────────────────────────────────────────

/// Plain-table iterator: keys are `enc(K)`, values are the stored bytes.
pub struct FjallIter<K, V: TableValue> {
    inner: fjall::Iter,
    _marker: PhantomData<(K, V)>,
}

impl<K: TableKey, V: TableValue> Iterator for FjallIter<K, V> {
    type Item = Result<(K, FjallGuard<V>), NetabaseError>;

    fn next(&mut self) -> Option<Self::Item> {
        let guard = self.inner.next()?;
        Some((|| {
            let (k, v) = guard.into_inner().map_err(backend_read)?;
            let key = K::decode_exact(k.as_ref())?;
            Ok((key, FjallGuard::new(v.as_ref())?))
        })())
    }
}

/// Multimap iterator: each key is `enc(K) ++ value_bytes`; decode `K` from the
/// prefix and the value from the suffix.
pub struct FjallMultiIter<K, V: TableValue> {
    inner: fjall::Iter,
    _marker: PhantomData<(K, V)>,
}

impl<K: TableKey, V: TableValue> Iterator for FjallMultiIter<K, V> {
    type Item = Result<(K, FjallGuard<V>), NetabaseError>;

    fn next(&mut self) -> Option<Self::Item> {
        let guard = self.inner.next()?;
        Some((|| {
            let k = guard.key().map_err(backend_read)?;
            let composed = k.as_ref();
            let (key, consumed) = K::decode(composed)?;
            let value_bytes = composed.get(consumed..).ok_or_else(corruption)?;
            Ok((key, FjallGuard::new(value_bytes)?))
        })())
    }
}

/// Encode a range's bounds into byte bounds over `enc(K)`.
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

// ── Read tables (over a Snapshot) ───────────────────────────────────────────

pub struct FjallReadTable<'txn, O, K, V> {
    pub(crate) snapshot: &'txn Snapshot,
    pub(crate) keyspace: SingleWriterTxKeyspace,
    pub(crate) _marker: PhantomData<(O, K, V)>,
}

impl<'txn, O: TableOwner, K: TableKey, V: TableValue> TableReadOps<K, V>
    for FjallReadTable<'txn, O, K, V>
{
    type Guard<'g>
        = FjallGuard<V>
    where
        Self: 'g;
    type Iter<'g>
        = FjallIter<K, V>
    where
        Self: 'g;

    fn get<'g>(&'g self, key: &K) -> Result<Option<Self::Guard<'g>>, NetabaseError> {
        let kb = encode_key(key)?;
        match self.snapshot.get(&self.keyspace, &kb).map_err(backend_read)? {
            Some(v) => Ok(Some(FjallGuard::new(v.as_ref())?)),
            None => Ok(None),
        }
    }

    fn get_all<'g>(&'g self, key: &K) -> Result<Self::Iter<'g>, NetabaseError> {
        // Plain table: at most one value — a degenerate inclusive range.
        self.range(key.clone()..=key.clone())
    }

    fn range<'g>(&'g self, range: impl RangeBounds<K>) -> Result<Self::Iter<'g>, NetabaseError> {
        let (lo, hi) = byte_bounds(range)?;
        Ok(FjallIter {
            inner: self.snapshot.range(&self.keyspace, (lo, hi)),
            _marker: PhantomData,
        })
    }
}

impl<'txn, O: TableOwner, K: TableKey, V: TableValue> StoreTable<O, K, V>
    for FjallReadTable<'txn, O, K, V>
{
    type TableConfig = FjallTableConfig;
}

pub struct FjallReadMultimapTable<'txn, O, K, V> {
    pub(crate) snapshot: &'txn Snapshot,
    pub(crate) keyspace: SingleWriterTxKeyspace,
    pub(crate) _marker: PhantomData<(O, K, V)>,
}

impl<'txn, O: TableOwner, K: TableKey, V: TableValue> TableReadOps<K, V>
    for FjallReadMultimapTable<'txn, O, K, V>
{
    type Guard<'g>
        = FjallGuard<V>
    where
        Self: 'g;
    type Iter<'g>
        = FjallMultiIter<K, V>
    where
        Self: 'g;

    fn get<'g>(&'g self, key: &K) -> Result<Option<Self::Guard<'g>>, NetabaseError> {
        match self.get_all(key)?.next() {
            Some(r) => r.map(|(_, g)| Some(g)),
            None => Ok(None),
        }
    }

    fn get_all<'g>(&'g self, key: &K) -> Result<Self::Iter<'g>, NetabaseError> {
        let kb = encode_key(key)?;
        Ok(FjallMultiIter {
            inner: self.snapshot.prefix(&self.keyspace, kb),
            _marker: PhantomData,
        })
    }

    fn range<'g>(&'g self, range: impl RangeBounds<K>) -> Result<Self::Iter<'g>, NetabaseError> {
        let (lo, hi) = byte_bounds(range)?;
        Ok(FjallMultiIter {
            inner: self.snapshot.range(&self.keyspace, (lo, hi)),
            _marker: PhantomData,
        })
    }
}

impl<'txn, O: TableOwner, K: TableKey, V: TableValue> StoreTable<O, K, V>
    for FjallReadMultimapTable<'txn, O, K, V>
{
    type TableConfig = FjallTableConfig;
}

// ── Write tables (over a RefCell<WriteTransaction>) ─────────────────────────

pub struct FjallWriteTable<'txn, 'db, O, K, V> {
    pub(crate) tx: &'txn RefCell<SingleWriterWriteTx<'db>>,
    pub(crate) keyspace: SingleWriterTxKeyspace,
    pub(crate) _marker: PhantomData<(O, K, V)>,
}

impl<'txn, 'db, O: TableOwner, K: TableKey, V: TableValue> TableReadOps<K, V>
    for FjallWriteTable<'txn, 'db, O, K, V>
{
    type Guard<'g>
        = FjallGuard<V>
    where
        Self: 'g;
    type Iter<'g>
        = FjallIter<K, V>
    where
        Self: 'g;

    fn get<'g>(&'g self, key: &K) -> Result<Option<Self::Guard<'g>>, NetabaseError> {
        let kb = encode_key(key)?;
        match self.tx.borrow().get(&self.keyspace, &kb).map_err(backend_read)? {
            Some(v) => Ok(Some(FjallGuard::new(v.as_ref())?)),
            None => Ok(None),
        }
    }

    fn get_all<'g>(&'g self, key: &K) -> Result<Self::Iter<'g>, NetabaseError> {
        self.range(key.clone()..=key.clone())
    }

    fn range<'g>(&'g self, range: impl RangeBounds<K>) -> Result<Self::Iter<'g>, NetabaseError> {
        let (lo, hi) = byte_bounds(range)?;
        Ok(FjallIter {
            inner: self.tx.borrow().range(&self.keyspace, (lo, hi)),
            _marker: PhantomData,
        })
    }
}

impl<'txn, 'db, O: TableOwner, K: TableKey, V: TableValue> TableWriteOps<K, V>
    for FjallWriteTable<'txn, 'db, O, K, V>
{
    fn insert(&mut self, key: &K, value: &V) -> Result<(), NetabaseError> {
        let kb = encode_key(key)?;
        let vb = serialize_value(value)?;
        self.tx.borrow_mut().insert(&self.keyspace, kb, vb);
        Ok(())
    }

    fn remove(&mut self, key: &K) -> Result<bool, NetabaseError> {
        let kb = encode_key(key)?;
        let existed = self
            .tx
            .borrow()
            .get(&self.keyspace, &kb)
            .map_err(backend_read)?
            .is_some();
        self.tx.borrow_mut().remove(&self.keyspace, kb);
        Ok(existed)
    }
}

impl<'txn, 'db, O: TableOwner, K: TableKey, V: TableValue> StoreTable<O, K, V>
    for FjallWriteTable<'txn, 'db, O, K, V>
{
    type TableConfig = FjallTableConfig;
}

pub struct FjallWriteMultimapTable<'txn, 'db, O, K, V> {
    pub(crate) tx: &'txn RefCell<SingleWriterWriteTx<'db>>,
    pub(crate) keyspace: SingleWriterTxKeyspace,
    pub(crate) _marker: PhantomData<(O, K, V)>,
}

impl<'txn, 'db, O: TableOwner, K: TableKey, V: TableValue> TableReadOps<K, V>
    for FjallWriteMultimapTable<'txn, 'db, O, K, V>
{
    type Guard<'g>
        = FjallGuard<V>
    where
        Self: 'g;
    type Iter<'g>
        = FjallMultiIter<K, V>
    where
        Self: 'g;

    fn get<'g>(&'g self, key: &K) -> Result<Option<Self::Guard<'g>>, NetabaseError> {
        match self.get_all(key)?.next() {
            Some(r) => r.map(|(_, g)| Some(g)),
            None => Ok(None),
        }
    }

    fn get_all<'g>(&'g self, key: &K) -> Result<Self::Iter<'g>, NetabaseError> {
        let kb = encode_key(key)?;
        Ok(FjallMultiIter {
            inner: self.tx.borrow().prefix(&self.keyspace, kb),
            _marker: PhantomData,
        })
    }

    fn range<'g>(&'g self, range: impl RangeBounds<K>) -> Result<Self::Iter<'g>, NetabaseError> {
        let (lo, hi) = byte_bounds(range)?;
        Ok(FjallMultiIter {
            inner: self.tx.borrow().range(&self.keyspace, (lo, hi)),
            _marker: PhantomData,
        })
    }
}

impl<'txn, 'db, O: TableOwner, K: TableKey, V: TableValue> TableWriteOps<K, V>
    for FjallWriteMultimapTable<'txn, 'db, O, K, V>
{
    fn insert(&mut self, key: &K, value: &V) -> Result<(), NetabaseError> {
        // Composed key carries (K, V); the stored value is empty.
        let composed = compose_key(key, value)?;
        self.tx
            .borrow_mut()
            .insert(&self.keyspace, composed, Slice::from(&[][..]));
        Ok(())
    }

    fn remove(&mut self, key: &K) -> Result<bool, NetabaseError> {
        // Remove every (K, *) pair: collect composed keys via prefix, then drop.
        let kb = encode_key(key)?;
        let mut doomed: Vec<Slice> = Vec::new();
        for guard in self.tx.borrow().prefix(&self.keyspace, &kb) {
            doomed.push(guard.key().map_err(backend_read)?);
        }
        let removed = !doomed.is_empty();
        let mut tx = self.tx.borrow_mut();
        for k in doomed {
            tx.remove(&self.keyspace, k);
        }
        Ok(removed)
    }

    fn remove_value(&mut self, key: &K, value: &V) -> Result<bool, NetabaseError> {
        let composed = compose_key(key, value)?;
        let existed = self
            .tx
            .borrow()
            .get(&self.keyspace, &composed)
            .map_err(backend_read)?
            .is_some();
        self.tx.borrow_mut().remove(&self.keyspace, composed);
        Ok(existed)
    }
}

impl<'txn, 'db, O: TableOwner, K: TableKey, V: TableValue> StoreTable<O, K, V>
    for FjallWriteMultimapTable<'txn, 'db, O, K, V>
{
    type TableConfig = FjallTableConfig;
}
