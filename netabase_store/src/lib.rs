//! Netabase store: a compile-time-validated data abstraction unifying
//! volatile arena memory and persistent key-value backends.
//!
//! The core (traits, errors, key encoding, value codec, arena store) is
//! `#![no_std]` and heap-free; redb/fjall backends and host conveniences
//! live behind the `std`-gated backend features.
#![cfg_attr(not(feature = "std"), no_std)]

pub mod databases;
pub mod errors;
pub mod keys;
pub mod traits;

pub mod reexports {
    pub use netabase_arena;
    pub use rkyv;
    pub use strum;

    #[cfg(feature = "redb-backend")]
    pub use redb;
}
