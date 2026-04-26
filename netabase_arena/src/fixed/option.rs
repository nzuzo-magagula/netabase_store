//! A canonical-form optional.
//!
//! `core::Option<T>`'s archived form cannot be `NoUndef`: when `None`, the
//! payload bytes are undefined. [`NbOption`] keeps the payload slot always
//! valid — `T::default()` when absent — so every byte is defined and equal
//! values archive to identical bytes (the canonical-form invariant shared by
//! the other fixed types).

use core::{fmt, hash};
use rkyv::bytecheck::CheckBytes;
use rkyv::munge::munge;
use rkyv::rancor::{Fallible, Source};
use rkyv::traits::NoUndef;
use rkyv::{Archive, Deserialize, Place, Portable, Serialize};

/// An optional value with a fixed-width, always-initialized archived form.
///
/// # Invariants
/// - **Canonical absent form**: when `None`, the payload holds
///   `T::default()`.
#[derive(Clone)]
pub struct NbOption<T> {
    present: bool,
    value: T,
}

/// Archived form of [`NbOption<T>`]: one tag byte (0/1) then the payload.
/// Align 1, zero padding (unaligned rkyv layout), all bytes defined.
#[derive(Portable)]
#[repr(C)]
pub struct ArchivedNbOption<T: Archive> {
    tag: u8,
    value: T::Archived,
}

// SAFETY: u8 tag + NoUndef payload in repr(C) with align-1 layout: no padding.
unsafe impl<T: Archive> NoUndef for ArchivedNbOption<T> where T::Archived: NoUndef {}

impl<T> NbOption<T> {
    /// The absent value (payload holds `T::default()`).
    #[must_use]
    pub fn none() -> Self
    where
        T: Default,
    {
        Self {
            present: false,
            value: T::default(),
        }
    }

    /// A present value.
    #[must_use]
    pub fn some(value: T) -> Self {
        Self {
            present: true,
            value,
        }
    }

    #[must_use]
    pub fn is_some(&self) -> bool {
        self.present
    }

    #[must_use]
    pub fn is_none(&self) -> bool {
        !self.present
    }

    /// Borrow the value if present.
    pub fn as_option(&self) -> Option<&T> {
        self.present.then_some(&self.value)
    }

    /// Convert into `core::Option`.
    pub fn into_option(self) -> Option<T> {
        self.present.then_some(self.value)
    }

    /// Replace with a present value, returning the previous state.
    pub fn replace(&mut self, value: T) -> Option<T>
    where
        T: Default,
    {
        let old = core::mem::replace(&mut self.value, value);
        let was_present = core::mem::replace(&mut self.present, true);
        was_present.then_some(old)
    }

    /// Clear to the canonical absent form.
    pub fn take(&mut self) -> Option<T>
    where
        T: Default,
    {
        let old = core::mem::take(&mut self.value);
        let was_present = core::mem::replace(&mut self.present, false);
        was_present.then_some(old)
    }
}

impl<T: Default> Default for NbOption<T> {
    fn default() -> Self {
        Self::none()
    }
}

impl<T: Default> From<Option<T>> for NbOption<T> {
    fn from(opt: Option<T>) -> Self {
        match opt {
            Some(v) => Self::some(v),
            None => Self::none(),
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for NbOption<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.as_option() {
            Some(v) => f.debug_tuple("NbSome").field(v).finish(),
            None => f.write_str("NbNone"),
        }
    }
}

// Logical comparison: None < Some(_), matching core::Option's Ord.
impl<T: PartialEq> PartialEq for NbOption<T> {
    fn eq(&self, other: &Self) -> bool {
        self.as_option() == other.as_option()
    }
}
impl<T: Eq> Eq for NbOption<T> {}
impl<T: PartialOrd> PartialOrd for NbOption<T> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        self.as_option().partial_cmp(&other.as_option())
    }
}
impl<T: Ord> Ord for NbOption<T> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.as_option().cmp(&other.as_option())
    }
}
impl<T: hash::Hash> hash::Hash for NbOption<T> {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.as_option().hash(state);
    }
}

// ── rkyv integration ─────────────────────────────────────────────────────────

impl<T> Archive for NbOption<T>
where
    T: Archive,
{
    type Archived = ArchivedNbOption<T>;
    type Resolver = T::Resolver;

    fn resolve(&self, resolver: Self::Resolver, out: Place<Self::Archived>) {
        munge!(let ArchivedNbOption { tag, value } = out);
        tag.write(self.present as u8);
        self.value.resolve(resolver, value);
    }
}

impl<S, T> Serialize<S> for NbOption<T>
where
    S: Fallible + ?Sized,
    T: Serialize<S>,
{
    fn serialize(&self, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
        self.value.serialize(serializer)
    }
}

impl<D, T> Deserialize<NbOption<T>, D> for ArchivedNbOption<T>
where
    D: Fallible + ?Sized,
    T: Archive<Resolver = ()>,
    T::Archived: Deserialize<T, D>,
{
    fn deserialize(&self, deserializer: &mut D) -> Result<NbOption<T>, D::Error> {
        Ok(NbOption {
            present: self.tag != 0,
            value: self.value.deserialize(deserializer)?,
        })
    }
}

impl<T: Archive> ArchivedNbOption<T> {
    #[must_use]
    pub fn is_some(&self) -> bool {
        self.tag != 0
    }

    #[must_use]
    pub fn is_none(&self) -> bool {
        self.tag == 0
    }

    /// Borrow the archived value if present.
    pub fn as_option(&self) -> Option<&T::Archived> {
        self.is_some().then_some(&self.value)
    }
}

impl<T: Archive> fmt::Debug for ArchivedNbOption<T>
where
    T::Archived: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.as_option() {
            Some(v) => f.debug_tuple("NbSome").field(v).finish(),
            None => f.write_str("NbNone"),
        }
    }
}
impl<T: Archive> PartialEq for ArchivedNbOption<T>
where
    T::Archived: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.as_option() == other.as_option()
    }
}
impl<T: Archive> Eq for ArchivedNbOption<T> where T::Archived: Eq {}
impl<T: Archive> PartialOrd for ArchivedNbOption<T>
where
    T::Archived: PartialOrd,
{
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        self.as_option().partial_cmp(&other.as_option())
    }
}
impl<T: Archive> Ord for ArchivedNbOption<T>
where
    T::Archived: Ord,
{
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.as_option().cmp(&other.as_option())
    }
}
impl<T: Archive> hash::Hash for ArchivedNbOption<T>
where
    T::Archived: hash::Hash,
{
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.as_option().hash(state);
    }
}

impl<T: Archive> Clone for ArchivedNbOption<T>
where
    T::Archived: Copy,
{
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: Archive> Copy for ArchivedNbOption<T> where T::Archived: Copy {}

#[cfg(feature = "pod")]
unsafe impl<T> bytemuck::Zeroable for ArchivedNbOption<T>
where
    T: Archive,
    T::Archived: bytemuck::Zeroable,
{
}
#[cfg(feature = "pod")]
unsafe impl<T> bytemuck::Pod for ArchivedNbOption<T>
where
    T: Archive + 'static,
    T::Archived: bytemuck::Pod,
{
}

/// Validation failure for [`ArchivedNbOption`] bytes.
#[derive(Debug)]
struct NbOptionTagError(u8);

impl fmt::Display for NbOptionTagError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NbOption tag must be 0 or 1, got {}", self.0)
    }
}

impl core::error::Error for NbOptionTagError {}

// SAFETY: validates the tag is 0/1 and the payload via T::Archived's own
// CheckBytes (the payload is always a live value — Default when absent).
unsafe impl<C, T> CheckBytes<C> for ArchivedNbOption<T>
where
    C: Fallible + ?Sized,
    C::Error: Source,
    T: Archive,
    T::Archived: CheckBytes<C>,
{
    unsafe fn check_bytes(value: *const Self, context: &mut C) -> Result<(), C::Error> {
        // SAFETY: tag is a u8 — readable for any bit pattern.
        let tag = unsafe { (*value).tag };
        if tag > 1 {
            return Err(C::Error::new(NbOptionTagError(tag)));
        }
        // SAFETY: in-bounds field projection; forwarding caller's contract.
        unsafe { T::Archived::check_bytes(&raw const (*value).value, context) }
    }
}
