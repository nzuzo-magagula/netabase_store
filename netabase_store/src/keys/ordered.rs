//! Order-preserving, prefix-free key encoding.
//!
//! Every key type encodes to a byte string such that **plain `memcmp` on the
//! encoded bytes agrees with the type's `Ord`**:
//!
//! ```text
//! a.cmp(&b) == encode(a).cmp(&encode(b))          (the order law)
//! decode(encode(a)) == (a, encode(a).len())       (round trip)
//! ```
//!
//! This is what lets every backend — redb's b-tree comparator, fjall's LSM
//! iteration, the arena store's sorted index — order entries with **zero
//! decoding, zero allocation, and no panic** inside the comparator. The
//! encoding is also **prefix-free**, so composite keys are plain
//! concatenation and a category tag byte in front of an enum key makes each
//! variant a contiguous range.
//!
//! ## Scheme
//!
//! | Type            | Encoding                                              |
//! |-----------------|-------------------------------------------------------|
//! | unsigned ints   | fixed-width big-endian                                |
//! | signed ints     | fixed-width big-endian, sign bit flipped              |
//! | `bool`          | one byte, `0`/`1` (validated on decode)               |
//! | `char`          | code point as `u32` big-endian                        |
//! | `()`            | zero bytes                                            |
//! | `[u8; N]`       | the raw bytes                                         |
//! | `NbString<N>`   | bytes with `0x00 → 0x00 0xFF`, terminated by `0x00`   |
//! | `NbVec<T, N>`   | `0x01` before each element, terminated by `0x00`      |
//! | tuples          | concatenation of the fields' encodings                |

use core::fmt;
use netabase_arena::fixed::{NbString, NbVec};

/// Failure encoding or decoding an ordered key. Heap-free and `Copy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyCodecError {
    /// The output buffer is too small for the encoded key.
    BufferTooSmall,
    /// The input ended before the value was complete.
    Truncated,
    /// The bytes are not a valid encoding for the expected type.
    Malformed,
    /// A leading tag byte does not name a known variant.
    UnknownTag(u8),
    /// The decoded value does not fit the fixed capacity of the target type.
    CapacityExceeded,
}

impl fmt::Display for KeyCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BufferTooSmall => write!(f, "key buffer too small"),
            Self::Truncated => write!(f, "key bytes truncated"),
            Self::Malformed => write!(f, "malformed key bytes"),
            Self::UnknownTag(tag) => write!(f, "unknown key variant tag {tag:#04x}"),
            Self::CapacityExceeded => write!(f, "decoded key exceeds fixed capacity"),
        }
    }
}

impl core::error::Error for KeyCodecError {}

/// `max` usable in const contexts (for folding encoded-length maxima of
/// generated key enums on stable Rust).
#[must_use]
pub const fn const_max(a: usize, b: usize) -> usize {
    if a > b { a } else { b }
}

/// A type with an order-preserving, prefix-free byte encoding.
///
/// Implementors must uphold the **order law** (`memcmp` on encodings agrees
/// with `Ord`) and the **round-trip law** (`decode ∘ encode == id`, consuming
/// exactly the encoded bytes). Both laws are exercised by the test suite for
/// every implementation in this module and for every macro-generated key.
pub trait OrderedKeyEncoding: Ord + Sized {
    /// Upper bound on the encoded length, used to size [`KeyBuf`]s and
    /// arena index slots at compile time.
    const MAX_ENCODED_LEN: usize;

    /// Encode into the front of `out`, returning the number of bytes
    /// written. Fails with [`KeyCodecError::BufferTooSmall`] only if `out`
    /// is shorter than the actual encoding.
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, KeyCodecError>;

    /// Decode a value from the front of `bytes`, returning it and the
    /// number of bytes consumed (prefix-freeness makes this unambiguous).
    fn decode(bytes: &[u8]) -> Result<(Self, usize), KeyCodecError>;

    /// Encode into a fresh [`KeyBuf`]. `N` must be at least
    /// [`MAX_ENCODED_LEN`](Self::MAX_ENCODED_LEN); generated code passes it
    /// as `{ <T as OrderedKeyEncoding>::MAX_ENCODED_LEN }`.
    fn encode_key_buf<const N: usize>(&self) -> Result<KeyBuf<N>, KeyCodecError> {
        let mut buf = KeyBuf::new();
        let written = self.encode_into(&mut buf.buf)?;
        buf.len = written as u32;
        Ok(buf)
    }

    /// Decode requiring that every input byte is consumed.
    fn decode_exact(bytes: &[u8]) -> Result<Self, KeyCodecError> {
        let (value, consumed) = Self::decode(bytes)?;
        if consumed != bytes.len() {
            return Err(KeyCodecError::Malformed);
        }
        Ok(value)
    }
}

/// A fixed-capacity byte buffer holding one encoded key.
///
/// `N` is typically `{ <T as OrderedKeyEncoding>::MAX_ENCODED_LEN }`. The
/// live encoding is `as_ref()`; comparison is on the live bytes, which by
/// the order law agrees with the source type's `Ord`.
#[derive(Clone, Copy, Debug)]
pub struct KeyBuf<const N: usize> {
    len: u32,
    buf: [u8; N],
}

impl<const N: usize> KeyBuf<N> {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            len: 0,
            buf: [0u8; N],
        }
    }

    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.buf[..self.len as usize]
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.len as usize
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl<const N: usize> Default for KeyBuf<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> AsRef<[u8]> for KeyBuf<N> {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl<const N: usize> PartialEq for KeyBuf<N> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}
impl<const N: usize> Eq for KeyBuf<N> {}
impl<const N: usize> PartialOrd for KeyBuf<N> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl<const N: usize> Ord for KeyBuf<N> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.as_slice().cmp(other.as_slice())
    }
}

/// Encode a key into a fixed-capacity byte value (`NbVec<u8, N>`): the
/// representation used when an ordered key must be *stored as a value*
/// (e.g. relational tables store the target's encoded primary key).
/// `N` must be at least `K::MAX_ENCODED_LEN`.
pub fn encode_rel_value<K: OrderedKeyEncoding, const N: usize>(
    key: &K,
) -> Result<netabase_arena::fixed::NbVec<u8, N>, KeyCodecError> {
    let mut buf = [0u8; N];
    let written = key.encode_into(&mut buf)?;
    netabase_arena::fixed::NbVec::try_from_slice(&buf[..written])
        .map_err(|_| KeyCodecError::BufferTooSmall)
}

/// Decode a key back out of its fixed-capacity value form.
pub fn decode_rel_value<K: OrderedKeyEncoding, const N: usize>(
    value: &netabase_arena::fixed::ArchivedNbVec<u8, N>,
) -> Result<K, KeyCodecError> {
    // u8's archived form is u8 itself, so the live prefix is plain bytes.
    K::decode_exact(value.as_slice())
}

// ── Integer encodings ────────────────────────────────────────────────────────

macro_rules! impl_unsigned {
    ($($ty:ty),* $(,)?) => {$(
        impl OrderedKeyEncoding for $ty {
            const MAX_ENCODED_LEN: usize = size_of::<$ty>();

            fn encode_into(&self, out: &mut [u8]) -> Result<usize, KeyCodecError> {
                const W: usize = size_of::<$ty>();
                let Some(dst) = out.get_mut(..W) else {
                    return Err(KeyCodecError::BufferTooSmall);
                };
                dst.copy_from_slice(&self.to_be_bytes());
                Ok(W)
            }

            fn decode(bytes: &[u8]) -> Result<(Self, usize), KeyCodecError> {
                const W: usize = size_of::<$ty>();
                let Some(src) = bytes.get(..W) else {
                    return Err(KeyCodecError::Truncated);
                };
                let mut be = [0u8; W];
                be.copy_from_slice(src);
                Ok((<$ty>::from_be_bytes(be), W))
            }
        }
    )*};
}

impl_unsigned!(u8, u16, u32, u64, u128);

macro_rules! impl_signed {
    ($($ty:ty => $uty:ty),* $(,)?) => {$(
        impl OrderedKeyEncoding for $ty {
            const MAX_ENCODED_LEN: usize = size_of::<$ty>();

            fn encode_into(&self, out: &mut [u8]) -> Result<usize, KeyCodecError> {
                // Flipping the sign bit maps the signed range onto the
                // unsigned range monotonically: i::MIN → 0, -1 → MAX/2, …
                let flipped = (*self as $uty) ^ (1 << (<$ty>::BITS - 1));
                flipped.encode_into(out)
            }

            fn decode(bytes: &[u8]) -> Result<(Self, usize), KeyCodecError> {
                let (flipped, consumed) = <$uty>::decode(bytes)?;
                Ok(((flipped ^ (1 << (<$ty>::BITS - 1))) as $ty, consumed))
            }
        }
    )*};
}

impl_signed!(i8 => u8, i16 => u16, i32 => u32, i64 => u64, i128 => u128);

// ── Other scalar encodings ───────────────────────────────────────────────────

impl OrderedKeyEncoding for bool {
    const MAX_ENCODED_LEN: usize = 1;

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, KeyCodecError> {
        let Some(dst) = out.first_mut() else {
            return Err(KeyCodecError::BufferTooSmall);
        };
        *dst = *self as u8;
        Ok(1)
    }

    fn decode(bytes: &[u8]) -> Result<(Self, usize), KeyCodecError> {
        match bytes.first() {
            Some(0) => Ok((false, 1)),
            Some(1) => Ok((true, 1)),
            Some(_) => Err(KeyCodecError::Malformed),
            None => Err(KeyCodecError::Truncated),
        }
    }
}

impl OrderedKeyEncoding for char {
    const MAX_ENCODED_LEN: usize = 4;

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, KeyCodecError> {
        (*self as u32).encode_into(out)
    }

    fn decode(bytes: &[u8]) -> Result<(Self, usize), KeyCodecError> {
        let (code, consumed) = u32::decode(bytes)?;
        let c = char::from_u32(code).ok_or(KeyCodecError::Malformed)?;
        Ok((c, consumed))
    }
}

impl OrderedKeyEncoding for () {
    const MAX_ENCODED_LEN: usize = 0;

    fn encode_into(&self, _: &mut [u8]) -> Result<usize, KeyCodecError> {
        Ok(0)
    }

    fn decode(_: &[u8]) -> Result<(Self, usize), KeyCodecError> {
        Ok(((), 0))
    }
}

impl<const N: usize> OrderedKeyEncoding for [u8; N] {
    const MAX_ENCODED_LEN: usize = N;

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, KeyCodecError> {
        let Some(dst) = out.get_mut(..N) else {
            return Err(KeyCodecError::BufferTooSmall);
        };
        dst.copy_from_slice(self);
        Ok(N)
    }

    fn decode(bytes: &[u8]) -> Result<(Self, usize), KeyCodecError> {
        let Some(src) = bytes.get(..N) else {
            return Err(KeyCodecError::Truncated);
        };
        let mut arr = [0u8; N];
        arr.copy_from_slice(src);
        Ok((arr, N))
    }
}

// ── Strings: escape 0x00 → 0x00 0xFF, terminate with a bare 0x00 ────────────
//
// Order proof sketch: for the first position where the raw strings differ,
// the encodings differ at the corresponding position with the same ordering
// (escaping is order-preserving on bytes since 0x00 maps to 0x00FF which
// still sorts below every other first byte except a shorter string's bare
// terminator 0x00 — and "prefix ends here" must sort below "prefix
// continues", which 0x00 < anything guarantees).

impl<const N: usize> OrderedKeyEncoding for NbString<N> {
    /// Worst case: every byte escaped (×2) plus the terminator.
    const MAX_ENCODED_LEN: usize = 2 * N + 1;

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, KeyCodecError> {
        let mut written = 0usize;
        for &b in self.as_str().as_bytes() {
            if b == 0x00 {
                let Some(dst) = out.get_mut(written..written + 2) else {
                    return Err(KeyCodecError::BufferTooSmall);
                };
                dst.copy_from_slice(&[0x00, 0xFF]);
                written += 2;
            } else {
                let Some(dst) = out.get_mut(written) else {
                    return Err(KeyCodecError::BufferTooSmall);
                };
                *dst = b;
                written += 1;
            }
        }
        let Some(dst) = out.get_mut(written) else {
            return Err(KeyCodecError::BufferTooSmall);
        };
        *dst = 0x00;
        Ok(written + 1)
    }

    fn decode(bytes: &[u8]) -> Result<(Self, usize), KeyCodecError> {
        let mut raw = [0u8; N];
        let mut raw_len = 0usize;
        let mut pos = 0usize;
        loop {
            match bytes.get(pos) {
                None => return Err(KeyCodecError::Truncated),
                Some(0x00) => match bytes.get(pos + 1) {
                    Some(0xFF) => {
                        // Escaped NUL.
                        if raw_len >= N {
                            return Err(KeyCodecError::CapacityExceeded);
                        }
                        raw[raw_len] = 0x00;
                        raw_len += 1;
                        pos += 2;
                    }
                    _ => {
                        // Bare terminator.
                        pos += 1;
                        break;
                    }
                },
                Some(&b) => {
                    if raw_len >= N {
                        return Err(KeyCodecError::CapacityExceeded);
                    }
                    raw[raw_len] = b;
                    raw_len += 1;
                    pos += 1;
                }
            }
        }
        let s = core::str::from_utf8(&raw[..raw_len]).map_err(|_| KeyCodecError::Malformed)?;
        let value = NbString::try_from_str(s).map_err(|_| KeyCodecError::CapacityExceeded)?;
        Ok((value, pos))
    }
}

// ── Sequences: 0x01 before each element, 0x00 terminator ────────────────────
//
// "Sequence ends" (0x00) sorts below "sequence continues" (0x01), which is
// exactly the slice Ord rule that a prefix sorts first. Elements are
// prefix-free, so no further framing is needed.

impl<T, const N: usize> OrderedKeyEncoding for NbVec<T, N>
where
    T: OrderedKeyEncoding + Clone + Default,
{
    const MAX_ENCODED_LEN: usize = N * (1 + T::MAX_ENCODED_LEN) + 1;

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, KeyCodecError> {
        let mut written = 0usize;
        for item in self.as_slice() {
            let Some(dst) = out.get_mut(written) else {
                return Err(KeyCodecError::BufferTooSmall);
            };
            *dst = 0x01;
            written += 1;
            written += item.encode_into(&mut out[written..])?;
        }
        let Some(dst) = out.get_mut(written) else {
            return Err(KeyCodecError::BufferTooSmall);
        };
        *dst = 0x00;
        Ok(written + 1)
    }

    fn decode(bytes: &[u8]) -> Result<(Self, usize), KeyCodecError> {
        let mut value = NbVec::new();
        let mut pos = 0usize;
        loop {
            match bytes.get(pos) {
                None => return Err(KeyCodecError::Truncated),
                Some(0x00) => {
                    pos += 1;
                    break;
                }
                Some(0x01) => {
                    pos += 1;
                    let (item, consumed) = T::decode(&bytes[pos..])?;
                    pos += consumed;
                    value
                        .try_push(item)
                        .map_err(|_| KeyCodecError::CapacityExceeded)?;
                }
                Some(_) => return Err(KeyCodecError::Malformed),
            }
        }
        Ok((value, pos))
    }
}

// ── Tuples: concatenation (each component is prefix-free) ───────────────────

macro_rules! impl_tuple {
    ($($name:ident : $idx:tt),+) => {
        impl<$($name: OrderedKeyEncoding),+> OrderedKeyEncoding for ($($name,)+) {
            const MAX_ENCODED_LEN: usize = 0 $(+ $name::MAX_ENCODED_LEN)+;

            fn encode_into(&self, out: &mut [u8]) -> Result<usize, KeyCodecError> {
                let mut written = 0usize;
                $(written += self.$idx.encode_into(&mut out[written..])?;)+
                Ok(written)
            }

            fn decode(bytes: &[u8]) -> Result<(Self, usize), KeyCodecError> {
                let mut pos = 0usize;
                let value = ($({
                    let (v, consumed) = $name::decode(&bytes[pos..])?;
                    pos += consumed;
                    v
                },)+);
                Ok((value, pos))
            }
        }
    };
}

impl_tuple!(A: 0);
impl_tuple!(A: 0, B: 1);
impl_tuple!(A: 0, B: 1, C: 2);
impl_tuple!(A: 0, B: 1, C: 2, D: 3);

// ── Tests: the order law and round-trip law per implementation ──────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn enc<T: OrderedKeyEncoding>(v: &T) -> Vec<u8> {
        let mut buf = vec![0u8; T::MAX_ENCODED_LEN];
        let n = v.encode_into(&mut buf).unwrap();
        buf.truncate(n);
        buf
    }

    /// Assert both laws over every ordered pair of `values`.
    fn check_laws<T: OrderedKeyEncoding + Clone + core::fmt::Debug>(values: &[T]) {
        for a in values {
            // Round trip, consuming exactly the encoding.
            let bytes = enc(a);
            let (back, consumed) = T::decode(&bytes).unwrap();
            assert_eq!(&back, a, "round-trip failed for {a:?}");
            assert_eq!(consumed, bytes.len(), "partial consume for {a:?}");
            // Prefix-freeness: decoding with trailing garbage consumes the
            // same amount.
            let mut extended = bytes.clone();
            extended.extend_from_slice(&[0xAB, 0xCD]);
            let (back2, consumed2) = T::decode(&extended).unwrap();
            assert_eq!(&back2, a);
            assert_eq!(consumed2, bytes.len());
            for b in values {
                assert_eq!(
                    a.cmp(b),
                    enc(a).cmp(&enc(b)),
                    "order law broken for {a:?} vs {b:?}"
                );
            }
        }
    }

    #[test]
    fn unsigned_laws() {
        check_laws(&[0u32, 1, 2, 255, 256, 65535, 1 << 24, u32::MAX]);
        check_laws(&[0u8, 1, 127, 128, 255]);
        check_laws(&[0u64, u64::MAX, 42]);
        check_laws(&[0u128, u128::MAX, 1 << 90]);
    }

    #[test]
    fn signed_laws() {
        check_laws(&[i32::MIN, -65536, -2, -1, 0, 1, 2, 65535, i32::MAX]);
        check_laws(&[i8::MIN, -1, 0, 1, i8::MAX]);
        check_laws(&[i64::MIN, -1, 0, i64::MAX]);
    }

    #[test]
    fn scalar_laws() {
        check_laws(&[false, true]);
        check_laws(&['\0', 'a', 'b', 'é', '中', char::MAX]);
        check_laws(&[[0u8; 4], [1, 2, 3, 4], [255; 4]]);
    }

    #[test]
    fn string_laws_including_embedded_nul() {
        type S = NbString<8>;
        let values: Vec<S> = [
            "", "a", "ab", "a\0", "a\0b", "a\u{1}", "b", "zzz",
        ]
        .iter()
        .map(|s| S::try_from_str(s).unwrap())
        .collect();
        check_laws(&values);
    }

    #[test]
    fn vec_laws_including_prefix_ordering() {
        type V = NbVec<u16, 4>;
        let values: Vec<V> = [
            &[][..],
            &[0],
            &[0, 0],
            &[1],
            &[1, 2],
            &[1, 2, 3],
            &[2],
            &[u16::MAX],
        ]
        .iter()
        .map(|s| V::try_from_slice(s).unwrap())
        .collect();
        check_laws(&values);
    }

    #[test]
    fn tuple_laws() {
        check_laws(&[
            (0u8, -5i32),
            (0u8, 5i32),
            (1u8, i32::MIN),
            (1u8, 0i32),
            (2u8, 7i32),
        ]);
    }

    #[test]
    fn composite_string_tuple_laws() {
        type S = NbString<4>;
        let s = |x: &str| S::try_from_str(x).unwrap();
        // The classic prefix trap: ("a", 2) must sort before ("ab", 1) —
        // termination makes the shorter string end first.
        check_laws(&[(s("a"), 2u32), (s("ab"), 1u32), (s("b"), 0u32)]);
    }

    #[test]
    fn decode_rejects_garbage() {
        assert_eq!(u32::decode(&[1, 2]).unwrap_err(), KeyCodecError::Truncated);
        assert_eq!(bool::decode(&[7]).unwrap_err(), KeyCodecError::Malformed);
        assert_eq!(
            char::decode(&[0x00, 0x11, 0x00, 0x00]).unwrap_err(),
            KeyCodecError::Malformed, // unpaired surrogate
        );
        assert_eq!(
            NbString::<4>::decode(&[b'a', b'b']).unwrap_err(),
            KeyCodecError::Truncated, // no terminator
        );
        assert_eq!(
            NbString::<2>::decode(&[b'a', b'b', b'c', 0x00]).unwrap_err(),
            KeyCodecError::CapacityExceeded,
        );
        assert_eq!(
            u8::decode_exact(&[1, 2]).unwrap_err(),
            KeyCodecError::Malformed, // trailing bytes
        );
    }

    #[test]
    fn key_buf_holds_encoding() {
        let key = 0xDEADu16;
        let buf = key
            .encode_key_buf::<{ <u16 as OrderedKeyEncoding>::MAX_ENCODED_LEN }>()
            .unwrap();
        assert_eq!(buf.as_slice(), &[0xDE, 0xAD]);
    }
}
