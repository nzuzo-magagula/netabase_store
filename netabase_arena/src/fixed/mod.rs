//! Fixed-capacity replacements for heap-allocated collection types.
//!
//! These types are the model-field vocabulary for arena-backed storage:
//! every type here has a **fixed-width, align-1, zero-padding archived form**
//! (`Portable + NoUndef`), so values serialize to a canonical byte string of
//! a size known at compile time and can live directly in [`TypedArena`]
//! slots, redb values, or IPC buffers — no heap, no pointers, no padding.
//!
//! | Heap type            | Fixed replacement      |
//! |----------------------|------------------------|
//! | `String`             | [`NbString<N>`]        |
//! | `Vec<T>`             | [`NbVec<T, N>`]        |
//! | `BTreeMap<K, V>`     | [`NbMap<K, V, N>`]     |
//!
//! # Canonical form
//!
//! Determinism (content hashing, byte-equality of equal values) relies on a
//! **canonical-tail invariant** maintained by every mutating method:
//!
//! - `NbString<N>`: bytes past `len` are always zero.
//! - `NbVec<T, N>` / `NbMap<K, V, N>`: slots past `len` always hold
//!   `T::default()`.
//!
//! Two logically equal values therefore always archive to identical bytes.
//!
//! # Capacity errors
//!
//! Exceeding `N` is reported as [`CapacityError`] by the fallible
//! constructors and `try_*` methods — never a panic, never a reallocation.
//!
//! [`TypedArena`]: crate::TypedArena

mod error;
mod map;
mod option;
mod string;
mod vec;

pub use error::CapacityError;
pub use map::{ArchivedEntry, ArchivedNbMap, Entry, NbMap};
pub use option::{ArchivedNbOption, NbOption};
pub use string::{ArchivedNbString, NbString};
pub use vec::{ArchivedNbVec, NbVec};
