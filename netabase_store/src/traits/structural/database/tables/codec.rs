//! The single value codec: canonical rkyv bytes, identical in arena slots,
//! redb values, and fjall values.
//!
//! A [`StoreValue`] has a **fixed-width** archived form ([`value_size`]) with
//! no out-of-line data, so serialization fills exactly `value_size::<T>()`
//! bytes and reads are validated zero-copy references into stored bytes.
//! Corrupt bytes surface as [`CodecErrorKind::Corruption`] — never a panic.

use crate::errors::{CodecErrorKind, NetabaseError};
use rkyv::api::low::{LowDeserializer, LowSerializer, LowValidator};
use rkyv::bytecheck::CheckBytes;
use rkyv::rancor;
use rkyv::ser::{allocator::SubAllocator, writer::Buffer};
use rkyv::traits::NoUndef;
use rkyv::{Archive, Deserialize, Portable, Serialize};

/// The no-alloc serializer used for all value writes (same shape as the
/// arena's slot serializer, so byte forms are identical across tiers).
pub type ValueSerializer<'buf, 'alloc> =
    LowSerializer<Buffer<'buf>, SubAllocator<'alloc>, rancor::Error>;

/// A type storable as a table value: fixed-width, validated, zero-copy
/// archived form.
///
/// Written as associated-type bounds in supertrait position so every
/// `T: StoreValue` use site gets the bounds elaborated for free.
pub trait StoreValue:
    Archive<
        Archived: Portable + NoUndef + for<'a> CheckBytes<LowValidator<'a, rancor::Error>>,
    > + for<'buf, 'alloc> Serialize<ValueSerializer<'buf, 'alloc>>
{
}

impl<T> StoreValue for T where
    T: Archive<
            Archived: Portable + NoUndef + for<'a> CheckBytes<LowValidator<'a, rancor::Error>>,
        > + for<'buf, 'alloc> Serialize<ValueSerializer<'buf, 'alloc>>
{
}

/// The exact stored size of `T`'s canonical byte form.
#[must_use]
pub const fn value_size<T: StoreValue>() -> usize {
    size_of::<T::Archived>()
}

/// Validated zero-copy access to a stored value.
///
/// The byte slice must be exactly [`value_size::<T>()`] long and pass
/// `T::Archived`'s validation; anything else is `Codec(Corruption)` /
/// `Codec(Truncated)`.
pub fn access_value<T: StoreValue>(bytes: &[u8]) -> Result<&T::Archived, NetabaseError> {
    if bytes.len() != value_size::<T>() {
        return Err(NetabaseError::Codec(CodecErrorKind::Truncated));
    }
    rkyv::api::low::access::<T::Archived, rancor::Error>(bytes)
        .map_err(|_| NetabaseError::Codec(CodecErrorKind::Corruption))
}

/// Serialize `value` into the front of `out`, returning the bytes written
/// (always [`value_size::<T>()`]).
pub fn serialize_value_into<T: StoreValue>(
    value: &T,
    out: &mut [u8],
) -> Result<usize, NetabaseError> {
    let size = value_size::<T>();
    let Some(dst) = out.get_mut(..size) else {
        return Err(NetabaseError::Codec(CodecErrorKind::SerializeFailed));
    };
    rkyv::api::low::to_bytes_in_with_alloc::<_, _, rancor::Error>(
        value,
        Buffer::from(dst),
        SubAllocator::empty(),
    )
    .map_err(|_| NetabaseError::Codec(CodecErrorKind::SerializeFailed))?;
    Ok(size)
}

/// Serialize `value` to a fresh heap buffer (host-side convenience).
#[cfg(feature = "std")]
pub fn serialize_value<T: StoreValue>(value: &T) -> Result<Vec<u8>, NetabaseError> {
    let mut out = vec![0u8; value_size::<T>()];
    serialize_value_into(value, &mut out)?;
    Ok(out)
}

/// Deserialize an owned `T` from its validated archived form.
pub fn deserialize_value<T: StoreValue>(archived: &T::Archived) -> Result<T, NetabaseError>
where
    T::Archived: Deserialize<T, LowDeserializer<rancor::Error>>,
{
    rkyv::api::low::deserialize::<T, rancor::Error>(archived)
        .map_err(|_| NetabaseError::Codec(CodecErrorKind::Corruption))
}

/// Validated owned read straight from stored bytes.
pub fn read_value<T: StoreValue>(bytes: &[u8]) -> Result<T, NetabaseError>
where
    T::Archived: Deserialize<T, LowDeserializer<rancor::Error>>,
{
    deserialize_value::<T>(access_value::<T>(bytes)?)
}
