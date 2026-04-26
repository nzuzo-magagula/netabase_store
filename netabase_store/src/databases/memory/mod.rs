//! In-memory reference backend (std-only, test-oriented).
//!
//! Semantics mirror the persistent backends so it can serve as the
//! differential-testing oracle:
//! - **Snapshot reads**: a read transaction clones the map at begin time and
//!   never observes later writes.
//! - **Atomic commits**: a write transaction stages into a working copy;
//!   `commit` swaps it in, drop discards it. No torn states.
//! - **Ordering**: keys are the order-preserving encodings in a `BTreeMap`,
//!   so iteration order equals the typed `Ord` — identical to redb/fjall.
//!
//! Single-writer is enforced at compile time by
//! `NetabaseStore::write_transaction(&mut self)`.

use crate::errors::NetabaseError;
use crate::traits::structural::{
    database::NetabaseStore, schema::repositories::NetabaseRepository,
};
use std::collections::BTreeMap;

pub mod tables;
pub mod transaction;

use transaction::repository::{MemoryRepoReadTx, MemoryRepoWriteTx};

/// `(table name, encoded key bytes) -> value byte strings` (one entry for a
/// plain table, several for a multimap).
pub(crate) type MemoryMap = BTreeMap<(&'static str, Vec<u8>), Vec<Vec<u8>>>;

pub struct MemoryStore<R: NetabaseRepository> {
    pub(crate) tables: R::Tables,
    pub(crate) map: MemoryMap,
}

impl<R: NetabaseRepository> NetabaseStore<R> for MemoryStore<R> {
    /// The memory store needs no external resource.
    type Resource = ();

    fn open(_: ()) -> Result<Self, NetabaseError> {
        Ok(Self {
            tables: R::TABLES,
            map: BTreeMap::new(),
        })
    }

    type NetabaseStoreReadTransaction<'db>
        = MemoryRepoReadTx<'db, R>
    where
        Self: 'db,
        R: 'db;
    type NetabaseStoreWriteTransaction<'db>
        = MemoryRepoWriteTx<'db, R>
    where
        Self: 'db,
        R: 'db;

    fn read_transaction<'a>(
        &'a self,
    ) -> Result<Self::NetabaseStoreReadTransaction<'a>, NetabaseError>
    where
        R: 'a,
    {
        Ok(MemoryRepoReadTx {
            tables: &self.tables,
            // Snapshot isolation: later writes are invisible to this txn.
            snapshot: self.map.clone(),
        })
    }

    fn write_transaction<'a>(
        &'a mut self,
    ) -> Result<Self::NetabaseStoreWriteTransaction<'a>, NetabaseError>
    where
        R: 'a,
    {
        let work = self.map.clone();
        Ok(MemoryRepoWriteTx {
            tables: &self.tables,
            base: &mut self.map,
            work: core::cell::RefCell::new(work),
        })
    }
}
