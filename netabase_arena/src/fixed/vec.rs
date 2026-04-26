use super::CapacityError;
use core::{fmt, hash};
use rkyv::bytecheck::CheckBytes;
use rkyv::munge::munge;
use rkyv::rancor::{Fallible, Source};
use rkyv::traits::NoUndef;
use rkyv::{Archive, Deserialize, Place, Portable, Serialize};

/// A fixed-capacity vector with inline storage.
///
/// Replaces `Vec<T>` in arena-backed models. Holds at most `N` elements;
/// exceeding the capacity is a [`CapacityError`], never a heap allocation.
///
/// `T: Default` is required because unused slots physically exist and must
/// hold valid values.
///
/// # Invariants
/// - `len <= N`; elements `items[..len]` are the live prefix.
/// - **Canonical tail**: `items[len..]` always holds `T::default()`, so two
///   equal vectors archive to identical bytes.
#[derive(Clone)]
pub struct NbVec<T, const N: usize> {
    len: u16,
    items: [T; N],
}

/// The archived form of [`NbVec<T, N>`]: `len` as little-endian bytes
/// followed by `N` archived elements.
///
/// With the `unaligned` rkyv layout every element is align-1, so this struct
/// has zero padding and align 1.
#[derive(Portable)]
#[repr(C)]
pub struct ArchivedNbVec<T: Archive, const N: usize> {
    len: [u8; 2],
    items: [T::Archived; N],
}

// SAFETY: len is a u8 array; items are N contiguous NoUndef values. The
// struct is repr(C) with align-1 fields (unaligned rkyv layout), so there is
// no padding between or after fields.
unsafe impl<T: Archive, const N: usize> NoUndef for ArchivedNbVec<T, N> where T::Archived: NoUndef {}

impl<T: Archive, const N: usize> Clone for ArchivedNbVec<T, N>
where
    T::Archived: Copy,
{
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: Archive, const N: usize> Copy for ArchivedNbVec<T, N> where T::Archived: Copy {}

// SAFETY: zero bytes decode as len 0 with all-zero elements, valid whenever
// the elements are themselves Zeroable; repr(C), align 1, no padding.
#[cfg(feature = "pod")]
unsafe impl<T, const N: usize> bytemuck::Zeroable for ArchivedNbVec<T, N>
where
    T: Archive,
    T::Archived: bytemuck::Zeroable,
{
}
// SAFETY: Copy + Zeroable + no padding (align-1 repr(C)) + 'static.
#[cfg(feature = "pod")]
unsafe impl<T, const N: usize> bytemuck::Pod for ArchivedNbVec<T, N>
where
    T: Archive + 'static,
    T::Archived: bytemuck::Pod,
{
}

impl<T, const N: usize> NbVec<T, N> {
    /// Compile-time guard: the length field is a u16.
    const CAPACITY_FITS_U16: () = assert!(N <= u16::MAX as usize, "NbVec capacity exceeds u16");

    /// The fixed element capacity `N`.
    pub const CAPACITY: usize = N;

    /// The empty vector, with all slots holding `T::default()`.
    #[must_use]
    pub fn new() -> Self
    where
        T: Default,
    {
        #[allow(clippy::let_unit_value)]
        let _ = Self::CAPACITY_FITS_U16;
        Self {
            len: 0,
            items: core::array::from_fn(|_| T::default()),
        }
    }

    /// Construct from a slice, failing with [`CapacityError`] if it is
    /// longer than `N`.
    pub fn try_from_slice(values: &[T]) -> Result<Self, CapacityError>
    where
        T: Default + Clone,
    {
        let mut out = Self::new();
        if values.len() > N {
            return Err(CapacityError {
                capacity: N,
                needed: values.len(),
            });
        }
        for v in values {
            // Cannot fail: checked above.
            let _ = out.try_push(v.clone());
        }
        Ok(out)
    }

    /// Append an element, failing with [`CapacityError`] when full.
    pub fn try_push(&mut self, value: T) -> Result<(), CapacityError> {
        let len = self.len as usize;
        if len >= N {
            return Err(CapacityError {
                capacity: N,
                needed: len + 1,
            });
        }
        self.items[len] = value;
        self.len += 1;
        Ok(())
    }

    /// Remove and return the last element, restoring its slot to
    /// `T::default()` (canonical tail).
    pub fn pop(&mut self) -> Option<T>
    where
        T: Default,
    {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        Some(core::mem::take(&mut self.items[self.len as usize]))
    }

    /// Remove and return the element at `idx`, shifting later elements left
    /// and restoring the vacated slot to `T::default()`.
    pub fn remove(&mut self, idx: usize) -> Option<T>
    where
        T: Default,
    {
        let len = self.len as usize;
        if idx >= len {
            return None;
        }
        let value = core::mem::take(&mut self.items[idx]);
        self.items[idx..len].rotate_left(1);
        self.len -= 1;
        Some(value)
    }

    /// Insert `value` at `idx`, shifting later elements right.
    /// Fails with [`CapacityError`] when full; out-of-bounds `idx` inserts
    /// at the end.
    pub fn try_insert(&mut self, idx: usize, value: T) -> Result<(), CapacityError> {
        let len = self.len as usize;
        if len >= N {
            return Err(CapacityError {
                capacity: N,
                needed: len + 1,
            });
        }
        let idx = idx.min(len);
        self.items[idx..=len].rotate_right(1);
        self.items[idx] = value;
        self.len += 1;
        Ok(())
    }

    /// The live prefix as a slice.
    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        &self.items[..self.len as usize]
    }

    /// The live prefix as a mutable slice.
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.items[..self.len as usize]
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.len as usize
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn get(&self, idx: usize) -> Option<&T> {
        self.as_slice().get(idx)
    }

    pub fn get_mut(&mut self, idx: usize) -> Option<&mut T> {
        self.as_mut_slice().get_mut(idx)
    }

    pub fn iter(&self) -> core::slice::Iter<'_, T> {
        self.as_slice().iter()
    }

    pub fn iter_mut(&mut self) -> core::slice::IterMut<'_, T> {
        self.as_mut_slice().iter_mut()
    }

    /// Shorten to `new_len`, restoring removed slots to `T::default()`.
    pub fn truncate(&mut self, new_len: usize)
    where
        T: Default,
    {
        let len = self.len as usize;
        if new_len >= len {
            return;
        }
        for slot in &mut self.items[new_len..len] {
            *slot = T::default();
        }
        self.len = new_len as u16;
    }

    /// Remove all elements, restoring every slot to `T::default()`.
    pub fn clear(&mut self)
    where
        T: Default,
    {
        self.truncate(0);
    }
}

impl<T: Default, const N: usize> Default for NbVec<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const N: usize> core::ops::Deref for NbVec<T, N> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T: fmt::Debug, const N: usize> fmt::Debug for NbVec<T, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.as_slice()).finish()
    }
}

// Logical comparison is on the live prefix — same law for the archived form.
impl<T: PartialEq, const N: usize> PartialEq for NbVec<T, N> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}
impl<T: Eq, const N: usize> Eq for NbVec<T, N> {}
impl<T: PartialOrd, const N: usize> PartialOrd for NbVec<T, N> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        self.as_slice().partial_cmp(other.as_slice())
    }
}
impl<T: Ord, const N: usize> Ord for NbVec<T, N> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.as_slice().cmp(other.as_slice())
    }
}
impl<T: hash::Hash, const N: usize> hash::Hash for NbVec<T, N> {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state);
    }
}

impl<'a, T, const N: usize> IntoIterator for &'a NbVec<T, N> {
    type Item = &'a T;
    type IntoIter = core::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

// ── rkyv integration ─────────────────────────────────────────────────────────
//
// Elements are restricted to `Archive<Resolver = ()>`: fixed-width archived
// forms with no out-of-line data. That is exactly the family of types allowed
// in arena-backed models (primitives, Nb* types, and generated model types).

impl<T, const N: usize> Archive for NbVec<T, N>
where
    T: Archive,
{
    type Archived = ArchivedNbVec<T, N>;
    type Resolver = [T::Resolver; N];

    fn resolve(&self, resolver: Self::Resolver, out: Place<Self::Archived>) {
        munge!(let ArchivedNbVec { len, items } = out);
        len.write(self.len.to_le_bytes());
        <[T; N]>::resolve(&self.items, resolver, items);
    }
}

impl<S, T, const N: usize> Serialize<S> for NbVec<T, N>
where
    S: Fallible + ?Sized,
    T: Serialize<S>,
{
    fn serialize(&self, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
        <[T; N]>::serialize(&self.items, serializer)
    }
}

impl<D, T, const N: usize> Deserialize<NbVec<T, N>, D> for ArchivedNbVec<T, N>
where
    D: Fallible + ?Sized,
    T: Archive + Default,
    T::Archived: Deserialize<T, D>,
{
    fn deserialize(&self, deserializer: &mut D) -> Result<NbVec<T, N>, D::Error> {
        let mut out: [T; N] = core::array::from_fn(|_| T::default());
        // Fill every slot from the archived array so all N values (live
        // prefix + canonical tail) round-trip exactly.
        for (slot, archived) in out.iter_mut().zip(self.items.iter()) {
            *slot = archived.deserialize(deserializer)?;
        }
        Ok(NbVec {
            len: u16::from_le_bytes(self.len),
            items: out,
        })
    }
}

impl<T: Archive, const N: usize> ArchivedNbVec<T, N> {
    #[must_use]
    pub fn len(&self) -> usize {
        u16::from_le_bytes(self.len) as usize
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The live prefix as a slice of archived elements.
    #[must_use]
    pub fn as_slice(&self) -> &[T::Archived] {
        &self.items[..self.len().min(N)]
    }

    pub fn get(&self, idx: usize) -> Option<&T::Archived> {
        self.as_slice().get(idx)
    }

    pub fn iter(&self) -> core::slice::Iter<'_, T::Archived> {
        self.as_slice().iter()
    }
}

impl<T: Archive, const N: usize> fmt::Debug for ArchivedNbVec<T, N>
where
    T::Archived: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.as_slice()).finish()
    }
}
impl<T: Archive, const N: usize> PartialEq for ArchivedNbVec<T, N>
where
    T::Archived: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}
impl<T: Archive, const N: usize> Eq for ArchivedNbVec<T, N> where T::Archived: Eq {}
impl<T: Archive, const N: usize> PartialOrd for ArchivedNbVec<T, N>
where
    T::Archived: PartialOrd,
{
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        self.as_slice().partial_cmp(other.as_slice())
    }
}
impl<T: Archive, const N: usize> Ord for ArchivedNbVec<T, N>
where
    T::Archived: Ord,
{
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.as_slice().cmp(other.as_slice())
    }
}
impl<T: Archive, const N: usize> hash::Hash for ArchivedNbVec<T, N>
where
    T::Archived: hash::Hash,
{
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state);
    }
}

/// Validation failure for [`ArchivedNbVec`] bytes.
#[derive(Debug)]
struct NbVecLenError {
    len: usize,
    capacity: usize,
}

impl fmt::Display for NbVecLenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "NbVec len {} exceeds capacity {}",
            self.len, self.capacity
        )
    }
}

impl core::error::Error for NbVecLenError {}

// SAFETY: validates len <= N and delegates element validation (all N slots,
// live prefix and canonical tail alike) to T::Archived's own CheckBytes.
unsafe impl<C, T, const N: usize> CheckBytes<C> for ArchivedNbVec<T, N>
where
    C: Fallible + ?Sized,
    C::Error: Source,
    T: Archive,
    T::Archived: CheckBytes<C>,
{
    unsafe fn check_bytes(value: *const Self, context: &mut C) -> Result<(), C::Error> {
        // SAFETY: caller guarantees `value` covers size_of::<Self>() bytes.
        // The len field is a plain byte array, readable for any bit pattern.
        let len_bytes = unsafe { &raw const (*value).len };
        let len = u16::from_le_bytes(unsafe { *len_bytes }) as usize;
        if len > N {
            return Err(C::Error::new(NbVecLenError { len, capacity: N }));
        }
        // SAFETY: elements live at the items field; validate each in place.
        let items = unsafe { &raw const (*value).items };
        for i in 0..N {
            // SAFETY: i < N, in-bounds projection of the items array.
            let elem = unsafe { (items as *const T::Archived).add(i) };
            // SAFETY: forwarding the caller's contract element-wise.
            unsafe { T::Archived::check_bytes(elem, context)? };
        }
        Ok(())
    }
}
