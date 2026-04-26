//! The crate-wide error type.
//!
//! Core variants are heap-free (`no_std`-compatible): fieldless kinds or
//! small `Copy` payloads, never `String`. Rich backend detail (redb/fjall/io
//! errors) is available only under the `std` feature as a boxed source.

use core::fmt;

pub use crate::keys::ordered::KeyCodecError;

/// What kind of storage-level operation failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StorageErrorKind {
    /// Opening or creating the store failed.
    Open,
    /// Opening a table inside a transaction failed.
    TableOpen,
    /// Beginning a transaction failed.
    TransactionBegin,
    /// Committing a transaction failed.
    Commit,
    /// A read operation failed in the backend.
    Read,
    /// A write operation failed in the backend.
    Write,
    /// The provided resource (path, buffer) is unusable.
    BadResource,
}

/// What kind of value-codec failure occurred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CodecErrorKind {
    /// Stored bytes failed validation — on-media corruption or a foreign
    /// byte format. Surfaced as an error, never a panic.
    Corruption,
    /// The value buffer is shorter than the type's fixed width.
    Truncated,
    /// Serializing a value into its canonical byte form failed.
    SerializeFailed,
}

/// What kind of routing failure occurred while walking the schema tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RoutingErrorKind {
    /// The address does not name a child of this node.
    UnknownAddress,
    /// A key round-tripped into a variant that does not belong here.
    WrongVariant,
}

/// An operation category a backend may decline to support.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpKind {
    /// Blob (chunked large data) operations — std-only; the arena store
    /// reports them unsupported.
    Blob,
    /// Multimap table operations.
    Multimap,
    /// Custom-table side effects.
    Custom,
}

/// Boxed, `Display`-rich detail from a `std` backend (redb/fjall/io).
#[cfg(feature = "std")]
pub struct BackendError(pub Box<dyn core::error::Error + Send + Sync + 'static>);

#[cfg(feature = "std")]
impl fmt::Debug for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

/// The crate-wide error type. Core variants carry no heap data.
#[derive(Debug)]
#[non_exhaustive]
pub enum NetabaseError {
    /// A storage-level operation failed. Under `std` the boxed source is
    /// usually attached via [`NetabaseError::backend`] instead.
    Storage(StorageErrorKind),
    /// Value (de)serialization failed; `Corruption` means stored bytes did
    /// not validate.
    Codec(CodecErrorKind),
    /// Order-preserving key encoding or decoding failed.
    KeyCodec(KeyCodecError),
    /// A fixed-capacity region cannot hold the requested data.
    Capacity {
        /// Physical table name (a compile-time constant).
        table: &'static str,
        needed: u32,
        available: u32,
    },
    /// Schema-tree routing failed.
    Routing(RoutingErrorKind),
    /// The store is exclusively held by another transaction.
    Busy,
    /// This backend does not support the operation category.
    Unsupported(OpKind),
    /// Backend-specific failure with its boxed source (std only).
    #[cfg(feature = "std")]
    Backend(StorageErrorKind, BackendError),
}

impl NetabaseError {
    /// Wrap a backend error with its storage-operation kind.
    #[cfg(feature = "std")]
    pub fn backend(
        kind: StorageErrorKind,
        err: impl core::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::Backend(kind, BackendError(Box::new(err)))
    }

    /// The storage kind, when this is a storage/backend error.
    pub fn storage_kind(&self) -> Option<StorageErrorKind> {
        match self {
            Self::Storage(k) => Some(*k),
            #[cfg(feature = "std")]
            Self::Backend(k, _) => Some(*k),
            _ => None,
        }
    }
}

impl fmt::Display for NetabaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage(kind) => write!(f, "storage error: {kind:?}"),
            Self::Codec(kind) => write!(f, "codec error: {kind:?}"),
            Self::KeyCodec(err) => write!(f, "key codec error: {err}"),
            Self::Capacity {
                table,
                needed,
                available,
            } => write!(
                f,
                "capacity exceeded in table {table}: needed {needed}, available {available}"
            ),
            Self::Routing(kind) => write!(f, "routing error: {kind:?}"),
            Self::Busy => write!(f, "store is busy: exclusively held by another transaction"),
            Self::Unsupported(op) => write!(f, "unsupported operation: {op:?}"),
            #[cfg(feature = "std")]
            Self::Backend(kind, err) => write!(f, "backend error ({kind:?}): {}", err.0),
        }
    }
}

impl core::error::Error for NetabaseError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            #[cfg(feature = "std")]
            Self::Backend(_, err) => Some(err.0.as_ref()),
            _ => None,
        }
    }
}

impl From<KeyCodecError> for NetabaseError {
    fn from(err: KeyCodecError) -> Self {
        Self::KeyCodec(err)
    }
}
