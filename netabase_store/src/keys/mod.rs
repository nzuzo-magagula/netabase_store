//! Key encoding: the order-preserving byte form used by every backend.

pub mod ordered;

pub use ordered::{KeyBuf, KeyCodecError, OrderedKeyEncoding};
