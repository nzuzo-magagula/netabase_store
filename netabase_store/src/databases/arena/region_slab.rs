//! The mutable work slab wrapper held behind a `RefCell` in a write
//! transaction, plus encode helpers shared by the table handles.

use super::region::{KEY_MAX, VAL_MAX};
use crate::errors::{CodecErrorKind, NetabaseError};
use crate::traits::structural::database::tables::codec::{serialize_value_into, value_size};
use crate::traits::structural::database::tables::core::{TableKey, TableValue};

/// A borrowed-mutable view of the work-region slab bytes.
pub struct WorkSlab<'buf>(pub &'buf mut [u8]);

/// Encode a key into an inline buffer (no heap).
pub(crate) fn encode_key_inline<K: TableKey>(
    key: &K,
) -> Result<([u8; KEY_MAX], usize), NetabaseError> {
    let mut buf = [0u8; KEY_MAX];
    let n = key.encode_into(&mut buf)?;
    Ok((buf, n))
}

/// Serialize a value into an inline buffer (no heap).
pub(crate) fn serialize_val_inline<V: TableValue>(
    value: &V,
) -> Result<([u8; VAL_MAX], usize), NetabaseError> {
    if value_size::<V>() > VAL_MAX {
        return Err(NetabaseError::Codec(CodecErrorKind::SerializeFailed));
    }
    let mut buf = [0u8; VAL_MAX];
    let n = serialize_value_into(value, &mut buf)?;
    Ok((buf, n))
}
