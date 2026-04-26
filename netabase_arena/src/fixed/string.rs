use super::CapacityError;
use core::{fmt, hash, str};
use rkyv::bytecheck::CheckBytes;
use rkyv::munge::munge;
use rkyv::rancor::{Fallible, Source};
use rkyv::traits::NoUndef;
use rkyv::{Archive, Deserialize, Place, Portable, Serialize};

/// A fixed-capacity UTF-8 string with inline storage.
///
/// Replaces `String` in arena-backed models. Holds at most `N` bytes of
/// UTF-8; exceeding the capacity is a [`CapacityError`], never a heap
/// allocation.
///
/// # Invariants
/// - `len <= N` and `buf[..len]` is valid UTF-8.
/// - **Canonical tail**: `buf[len..]` is always zero, so two equal strings
///   are always byte-identical (and archive to identical bytes).
#[derive(Clone, Copy)]
pub struct NbString<const N: usize> {
    len: u16,
    buf: [u8; N],
}

/// The archived form of [`NbString<N>`]: `len` as little-endian bytes
/// followed by the buffer. Align 1, zero padding, fully defined bytes.
#[derive(Portable, Clone, Copy)]
#[repr(C)]
pub struct ArchivedNbString<const N: usize> {
    len: [u8; 2],
    buf: [u8; N],
}

// SAFETY: all fields are u8 arrays — align 1, no padding, every byte is
// always part of an initialized field.
unsafe impl<const N: usize> NoUndef for ArchivedNbString<N> {}

// SAFETY: all-u8 layout — any byte pattern is readable, all-zero is the
// valid empty string, the type is Copy with no padding.
#[cfg(feature = "pod")]
unsafe impl<const N: usize> bytemuck::Zeroable for ArchivedNbString<N> {}
#[cfg(feature = "pod")]
unsafe impl<const N: usize> bytemuck::Pod for ArchivedNbString<N> {}

impl<const N: usize> NbString<N> {
    /// Compile-time guard: the length field is a u16.
    const CAPACITY_FITS_U16: () = assert!(N <= u16::MAX as usize, "NbString capacity exceeds u16");

    /// The fixed byte capacity `N`.
    pub const CAPACITY: usize = N;

    /// The empty string.
    #[must_use]
    pub const fn new() -> Self {
        #[allow(clippy::let_unit_value)]
        let _ = Self::CAPACITY_FITS_U16;
        Self {
            len: 0,
            buf: [0u8; N],
        }
    }

    /// Construct from `s`, failing with [`CapacityError`] if `s.len() > N`.
    pub fn try_from_str(s: &str) -> Result<Self, CapacityError> {
        let mut out = Self::new();
        out.try_push_str(s)?;
        Ok(out)
    }

    /// Append `s`, failing with [`CapacityError`] if it does not fit.
    /// On failure the string is unchanged.
    pub fn try_push_str(&mut self, s: &str) -> Result<(), CapacityError> {
        let new_len = self.len as usize + s.len();
        if new_len > N {
            return Err(CapacityError {
                capacity: N,
                needed: new_len,
            });
        }
        self.buf[self.len as usize..new_len].copy_from_slice(s.as_bytes());
        self.len = new_len as u16;
        Ok(())
    }

    /// Append one char, failing with [`CapacityError`] if it does not fit.
    pub fn try_push(&mut self, c: char) -> Result<(), CapacityError> {
        let mut tmp = [0u8; 4];
        self.try_push_str(c.encode_utf8(&mut tmp))
    }

    /// View as `&str`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        // SAFETY: invariant — buf[..len] is valid UTF-8.
        unsafe { str::from_utf8_unchecked(&self.buf[..self.len as usize]) }
    }

    /// Length in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len as usize
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Reset to empty, re-zeroing the tail to keep the canonical form.
    pub fn clear(&mut self) {
        self.buf[..self.len as usize].fill(0);
        self.len = 0;
    }
}

impl<const N: usize> Default for NbString<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> TryFrom<&str> for NbString<N> {
    type Error = CapacityError;
    fn try_from(s: &str) -> Result<Self, CapacityError> {
        Self::try_from_str(s)
    }
}

impl<const N: usize> core::ops::Deref for NbString<N> {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl<const N: usize> fmt::Debug for NbString<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl<const N: usize> fmt::Display for NbString<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// Logical comparison is on string content — for both native and archived
// forms, so the order law `a.cmp(b) == archived(a).cmp(archived(b))` holds.
impl<const N: usize> PartialEq for NbString<N> {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}
impl<const N: usize> Eq for NbString<N> {}
impl<const N: usize> PartialOrd for NbString<N> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl<const N: usize> Ord for NbString<N> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.as_str().cmp(other.as_str())
    }
}
impl<const N: usize> hash::Hash for NbString<N> {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}
impl<const N: usize> PartialEq<str> for NbString<N> {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}
impl<const N: usize> PartialEq<&str> for NbString<N> {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

// ── rkyv integration ─────────────────────────────────────────────────────────

impl<const N: usize> Archive for NbString<N> {
    type Archived = ArchivedNbString<N>;
    type Resolver = ();

    fn resolve(&self, _: (), out: Place<Self::Archived>) {
        munge!(let ArchivedNbString { len, buf } = out);
        len.write(self.len.to_le_bytes());
        buf.write(self.buf);
    }
}

impl<S: Fallible + ?Sized, const N: usize> Serialize<S> for NbString<N> {
    fn serialize(&self, _: &mut S) -> Result<(), S::Error> {
        Ok(())
    }
}

impl<D: Fallible + ?Sized, const N: usize> Deserialize<NbString<N>, D> for ArchivedNbString<N> {
    fn deserialize(&self, _: &mut D) -> Result<NbString<N>, D::Error> {
        Ok(NbString {
            len: u16::from_le_bytes(self.len),
            buf: self.buf,
        })
    }
}

impl<const N: usize> ArchivedNbString<N> {
    /// Length in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        u16::from_le_bytes(self.len) as usize
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// View as `&str`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        // SAFETY: invariant (checked at construction or by CheckBytes) —
        // len <= N and buf[..len] is valid UTF-8.
        unsafe { str::from_utf8_unchecked(&self.buf[..self.len()]) }
    }
}

impl<const N: usize> fmt::Debug for ArchivedNbString<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}
impl<const N: usize> fmt::Display for ArchivedNbString<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
impl<const N: usize> PartialEq for ArchivedNbString<N> {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}
impl<const N: usize> Eq for ArchivedNbString<N> {}
impl<const N: usize> PartialOrd for ArchivedNbString<N> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl<const N: usize> Ord for ArchivedNbString<N> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.as_str().cmp(other.as_str())
    }
}
impl<const N: usize> hash::Hash for ArchivedNbString<N> {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}
impl<const N: usize> PartialEq<NbString<N>> for ArchivedNbString<N> {
    fn eq(&self, other: &NbString<N>) -> bool {
        self.as_str() == other.as_str()
    }
}
impl<const N: usize> PartialEq<str> for ArchivedNbString<N> {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

/// Validation failure for [`ArchivedNbString`] bytes.
#[derive(Debug)]
enum NbStringCheckError {
    LenOutOfRange { len: usize, capacity: usize },
    NotUtf8,
    NonCanonicalTail,
}

impl fmt::Display for NbStringCheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LenOutOfRange { len, capacity } => {
                write!(f, "NbString len {len} exceeds capacity {capacity}")
            }
            Self::NotUtf8 => write!(f, "NbString content is not valid UTF-8"),
            Self::NonCanonicalTail => write!(f, "NbString tail bytes are not zero"),
        }
    }
}

impl core::error::Error for NbStringCheckError {}

// SAFETY: check_bytes verifies every invariant `as_str` relies on (len <= N,
// UTF-8 prefix) plus the canonical zero tail, over fully-defined bytes
// (Self: NoUndef, so any byte pattern is at least readable).
unsafe impl<C, const N: usize> CheckBytes<C> for ArchivedNbString<N>
where
    C: Fallible + ?Sized,
    C::Error: Source,
{
    unsafe fn check_bytes(value: *const Self, _: &mut C) -> Result<(), C::Error> {
        // SAFETY: caller guarantees `value` points to size_of::<Self>()
        // initialized bytes; Self is NoUndef so reading them is defined.
        let this = unsafe { &*value };
        let len = u16::from_le_bytes(this.len) as usize;
        if len > N {
            return Err(C::Error::new(NbStringCheckError::LenOutOfRange {
                len,
                capacity: N,
            }));
        }
        if str::from_utf8(&this.buf[..len]).is_err() {
            return Err(C::Error::new(NbStringCheckError::NotUtf8));
        }
        if this.buf[len..].iter().any(|&b| b != 0) {
            return Err(C::Error::new(NbStringCheckError::NonCanonicalTail));
        }
        Ok(())
    }
}
