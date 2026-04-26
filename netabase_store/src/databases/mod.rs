//! Storage backends.
//!
//! - `memory` (std): the reference model — snapshot reads, staged atomic
//!   commits, used for differential testing.
//! - `redb` (`redb-backend`): persistent ACID b-tree store.
//! - `arena` (`arena-store`): heap-free volatile store over a manifest
//!   region (Phase 5).
//! - fjall (`fjall-backend`): being rebuilt with real transactions; module
//!   lands in Phase 6.

#[cfg(feature = "std")]
pub mod memory;

#[cfg(feature = "redb-backend")]
pub mod redb;

#[cfg(feature = "arena-store")]
pub mod arena;

#[cfg(feature = "fjall-backend")]
pub mod fjall;
