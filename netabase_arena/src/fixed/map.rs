use super::{CapacityError, NbVec};
use crate::fixed::vec::ArchivedNbVec;
use core::{cmp::Ordering, fmt, hash};
use rkyv::bytecheck::CheckBytes;
use rkyv::munge::munge;
use rkyv::rancor::{Fallible, Source};
use rkyv::traits::NoUndef;
use rkyv::{Archive, Deserialize, Place, Portable, Serialize};

/// One key/value pair of an [`NbMap`].
///
/// A dedicated struct (rather than a tuple) so the archived form can be a
/// zero-padding `repr(C)` struct with `Resolver = ()`.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Entry<K, V> {
    pub key: K,
    pub value: V,
}

/// The archived form of [`Entry<K, V>`]: key bytes then value bytes,
/// align 1, zero padding (unaligned rkyv layout).
#[derive(Portable)]
#[repr(C)]
pub struct ArchivedEntry<K: Archive, V: Archive> {
    pub key: K::Archived,
    pub value: V::Archived,
}

// SAFETY: repr(C) of two NoUndef fields with align-1 layout — no padding.
unsafe impl<K: Archive, V: Archive> NoUndef for ArchivedEntry<K, V>
where
    K::Archived: NoUndef,
    V::Archived: NoUndef,
{
}

impl<K: Archive, V: Archive> Clone for ArchivedEntry<K, V>
where
    K::Archived: Copy,
    V::Archived: Copy,
{
    fn clone(&self) -> Self {
        *self
    }
}
impl<K: Archive, V: Archive> Copy for ArchivedEntry<K, V>
where
    K::Archived: Copy,
    V::Archived: Copy,
{
}

// SAFETY: zero bytes are the archived default entry; fields are Zeroable.
#[cfg(feature = "pod")]
unsafe impl<K, V> bytemuck::Zeroable for ArchivedEntry<K, V>
where
    K: Archive,
    V: Archive,
    K::Archived: bytemuck::Zeroable,
    V::Archived: bytemuck::Zeroable,
{
}
// SAFETY: Copy + Zeroable + no padding (align-1 repr(C)) + 'static.
#[cfg(feature = "pod")]
unsafe impl<K, V> bytemuck::Pod for ArchivedEntry<K, V>
where
    K: Archive + 'static,
    V: Archive + 'static,
    K::Archived: bytemuck::Pod,
    V::Archived: bytemuck::Pod,
{
}

impl<K, V> Archive for Entry<K, V>
where
    K: Archive,
    V: Archive,
{
    type Archived = ArchivedEntry<K, V>;
    type Resolver = (K::Resolver, V::Resolver);

    fn resolve(&self, resolver: Self::Resolver, out: Place<Self::Archived>) {
        let (kr, vr) = resolver;
        munge!(let ArchivedEntry { key, value } = out);
        self.key.resolve(kr, key);
        self.value.resolve(vr, value);
    }
}

impl<S, K, V> Serialize<S> for Entry<K, V>
where
    S: Fallible + ?Sized,
    K: Serialize<S>,
    V: Serialize<S>,
{
    fn serialize(&self, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
        Ok((
            self.key.serialize(serializer)?,
            self.value.serialize(serializer)?,
        ))
    }
}

impl<D, K, V> Deserialize<Entry<K, V>, D> for ArchivedEntry<K, V>
where
    D: Fallible + ?Sized,
    K: Archive<Resolver = ()>,
    V: Archive<Resolver = ()>,
    K::Archived: Deserialize<K, D>,
    V::Archived: Deserialize<V, D>,
{
    fn deserialize(&self, deserializer: &mut D) -> Result<Entry<K, V>, D::Error> {
        Ok(Entry {
            key: self.key.deserialize(deserializer)?,
            value: self.value.deserialize(deserializer)?,
        })
    }
}

impl<K: Archive, V: Archive> fmt::Debug for ArchivedEntry<K, V>
where
    K::Archived: fmt::Debug,
    V::Archived: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ArchivedEntry")
            .field("key", &self.key)
            .field("value", &self.value)
            .finish()
    }
}
impl<K: Archive, V: Archive> PartialEq for ArchivedEntry<K, V>
where
    K::Archived: PartialEq,
    V::Archived: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key && self.value == other.value
    }
}
impl<K: Archive, V: Archive> Eq for ArchivedEntry<K, V>
where
    K::Archived: Eq,
    V::Archived: Eq,
{
}
impl<K: Archive, V: Archive> PartialOrd for ArchivedEntry<K, V>
where
    K::Archived: PartialOrd,
    V::Archived: PartialOrd,
{
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match self.key.partial_cmp(&other.key) {
            Some(Ordering::Equal) => self.value.partial_cmp(&other.value),
            other_ord => other_ord,
        }
    }
}
impl<K: Archive, V: Archive> Ord for ArchivedEntry<K, V>
where
    K::Archived: Ord,
    V::Archived: Ord,
{
    fn cmp(&self, other: &Self) -> Ordering {
        self.key.cmp(&other.key).then_with(|| self.value.cmp(&other.value))
    }
}
impl<K: Archive, V: Archive> hash::Hash for ArchivedEntry<K, V>
where
    K::Archived: hash::Hash,
    V::Archived: hash::Hash,
{
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.key.hash(state);
        self.value.hash(state);
    }
}

// SAFETY: delegates to the fields' own CheckBytes in declaration order.
unsafe impl<C, K, V> CheckBytes<C> for ArchivedEntry<K, V>
where
    C: Fallible + ?Sized,
    C::Error: Source,
    K: Archive,
    V: Archive,
    K::Archived: CheckBytes<C>,
    V::Archived: CheckBytes<C>,
{
    unsafe fn check_bytes(value: *const Self, context: &mut C) -> Result<(), C::Error> {
        // SAFETY: in-bounds field projections of `value`; the caller's
        // contract covers the whole struct.
        unsafe {
            K::Archived::check_bytes(&raw const (*value).key, context)?;
            V::Archived::check_bytes(&raw const (*value).value, context)?;
        }
        Ok(())
    }
}

/// A fixed-capacity ordered map with inline storage.
///
/// Replaces `BTreeMap<K, V>` in arena-backed models. Entries are kept sorted
/// by key, so lookups are binary searches and iteration is in key order.
/// Exceeding the capacity is a [`CapacityError`], never a heap allocation.
///
/// Inherits the canonical-tail invariant from [`NbVec`]: slots past `len`
/// hold `Entry::default()`.
#[derive(Clone)]
pub struct NbMap<K, V, const N: usize> {
    entries: NbVec<Entry<K, V>, N>,
}

/// The archived form of [`NbMap<K, V, N>`].
pub type ArchivedNbMap<K, V, const N: usize> = ArchivedNbVec<Entry<K, V>, N>;

impl<K, V, const N: usize> NbMap<K, V, N>
where
    K: Ord,
{
    /// The fixed entry capacity `N`.
    pub const CAPACITY: usize = N;

    /// The empty map.
    #[must_use]
    pub fn new() -> Self
    where
        K: Default,
        V: Default,
    {
        Self {
            entries: NbVec::new(),
        }
    }

    /// Insert or replace. Returns the previous value for the key, or
    /// [`CapacityError`] if a *new* key does not fit.
    pub fn try_insert(&mut self, key: K, value: V) -> Result<Option<V>, CapacityError> {
        match self
            .entries
            .as_slice()
            .binary_search_by(|e| e.key.cmp(&key))
        {
            Ok(pos) => {
                let slot = self.entries.get_mut(pos).expect("binary_search position");
                Ok(Some(core::mem::replace(&mut slot.value, value)))
            }
            Err(pos) => {
                self.entries.try_insert(pos, Entry { key, value })?;
                Ok(None)
            }
        }
    }

    /// Look up a value by key.
    pub fn get(&self, key: &K) -> Option<&V> {
        self.entries
            .as_slice()
            .binary_search_by(|e| e.key.cmp(key))
            .ok()
            .and_then(|pos| self.entries.get(pos))
            .map(|e| &e.value)
    }

    /// Look up a value by key, mutably.
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        let pos = self
            .entries
            .as_slice()
            .binary_search_by(|e| e.key.cmp(key))
            .ok()?;
        self.entries.get_mut(pos).map(|e| &mut e.value)
    }

    /// Remove an entry by key, returning its value.
    pub fn remove(&mut self, key: &K) -> Option<V>
    where
        K: Default,
        V: Default,
    {
        let pos = self
            .entries
            .as_slice()
            .binary_search_by(|e| e.key.cmp(key))
            .ok()?;
        self.entries.remove(pos).map(|e| e.value)
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.get(key).is_some()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate entries in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.entries.iter().map(|e| (&e.key, &e.value))
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.entries.iter().map(|e| &e.key)
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.entries.iter().map(|e| &e.value)
    }

    /// Remove all entries.
    pub fn clear(&mut self)
    where
        K: Default,
        V: Default,
    {
        self.entries.clear();
    }
}

impl<K: Ord + Default, V: Default, const N: usize> Default for NbMap<K, V, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: fmt::Debug + Ord, V: fmt::Debug, const N: usize> fmt::Debug for NbMap<K, V, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<K: PartialEq, V: PartialEq, const N: usize> PartialEq for NbMap<K, V, N> {
    fn eq(&self, other: &Self) -> bool {
        self.entries == other.entries
    }
}
impl<K: Eq, V: Eq, const N: usize> Eq for NbMap<K, V, N> {}
impl<K: Ord, V: Ord, const N: usize> PartialOrd for NbMap<K, V, N> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl<K: Ord, V: Ord, const N: usize> Ord for NbMap<K, V, N> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.entries.cmp(&other.entries)
    }
}
impl<K: hash::Hash, V: hash::Hash, const N: usize> hash::Hash for NbMap<K, V, N> {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.entries.hash(state);
    }
}

// ── rkyv integration: delegate wholesale to the inner NbVec ─────────────────

impl<K, V, const N: usize> Archive for NbMap<K, V, N>
where
    K: Archive,
    V: Archive,
{
    type Archived = ArchivedNbMap<K, V, N>;
    type Resolver = <NbVec<Entry<K, V>, N> as Archive>::Resolver;

    fn resolve(&self, resolver: Self::Resolver, out: Place<Self::Archived>) {
        self.entries.resolve(resolver, out);
    }
}

impl<S, K, V, const N: usize> Serialize<S> for NbMap<K, V, N>
where
    S: Fallible + ?Sized,
    K: Serialize<S>,
    V: Serialize<S>,
{
    fn serialize(&self, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
        Serialize::<S>::serialize(&self.entries, serializer)
    }
}

impl<D, K, V, const N: usize> Deserialize<NbMap<K, V, N>, D> for ArchivedNbMap<K, V, N>
where
    D: Fallible + ?Sized,
    K: Archive<Resolver = ()> + Default,
    V: Archive<Resolver = ()> + Default,
    K::Archived: Deserialize<K, D>,
    V::Archived: Deserialize<V, D>,
{
    fn deserialize(&self, deserializer: &mut D) -> Result<NbMap<K, V, N>, D::Error> {
        Ok(NbMap {
            entries: Deserialize::<NbVec<Entry<K, V>, N>, D>::deserialize(self, deserializer)?,
        })
    }
}

impl<K, V, const N: usize> ArchivedNbMap<K, V, N>
where
    K: Archive<Resolver = ()>,
    V: Archive<Resolver = ()>,
{
    /// Binary-search the live prefix with a caller-supplied key comparator.
    ///
    /// The comparator receives each probed archived key and must return how
    /// that key compares to the target (`Less` means "probe is before the
    /// target").
    pub fn get_with<F>(&self, mut probe: F) -> Option<&V::Archived>
    where
        F: FnMut(&K::Archived) -> Ordering,
    {
        self.as_slice()
            .binary_search_by(|e| probe(&e.key))
            .ok()
            .map(|pos| &self.as_slice()[pos].value)
    }

    /// Iterate archived entries in key order.
    pub fn iter_entries(&self) -> impl Iterator<Item = (&K::Archived, &V::Archived)> {
        self.as_slice().iter().map(|e| (&e.key, &e.value))
    }
}
