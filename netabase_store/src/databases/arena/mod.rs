//! The heap-free volatile store over a caller-supplied `&mut [u8]` region.
//!
//! All records live in the byte buffer as a single sorted slab (see
//! [`region`]); the store holds no heap state. The buffer is split in half:
//! the **base** half holds committed records, the **work** half is the
//! write-transaction scratch. A write transaction stages into work
//! (read-your-writes) and `commit` copies work over base in one shot — atomic,
//! and drop-without-commit leaves base untouched (rollback). Single-writer is
//! a compile-time property of `write_transaction(&mut self)`.
//!
//! Manifest-first: the caller pre-sizes the buffer (e.g.
//! `[0u8; 2 * (COUNT_HDR + CAP * RECORD_SIZE)]`); exceeding capacity is an
//! explicit [`NetabaseError::Capacity`], never a reallocation.

pub mod region;
pub mod region_slab;
pub mod tables;
pub mod transaction;

use crate::errors::{NetabaseError, StorageErrorKind};
use crate::traits::structural::{database::NetabaseStore, schema::repositories::NetabaseRepository};
use core::cell::RefCell;
use core::marker::PhantomData;
use region_slab::WorkSlab;
use transaction::{ArenaReadTx, ArenaWriteTx};

pub struct ArenaStore<'buf, R: NetabaseRepository> {
    /// The whole caller-supplied region: `[ base half | work half ]`.
    buf: &'buf mut [u8],
    tables: R::Tables,
}

impl<'buf, R: NetabaseRepository> ArenaStore<'buf, R> {
    /// Byte length of each half (base / work).
    fn half(&self) -> usize {
        self.buf.len() / 2
    }
}

impl<'buf, R: NetabaseRepository> NetabaseStore<R> for ArenaStore<'buf, R> {
    /// A manifest-sized byte region. The caller owns and pre-sizes it; the
    /// store never allocates.
    type Resource = &'buf mut [u8];

    fn open(buf: &'buf mut [u8]) -> Result<Self, NetabaseError> {
        // Each half must at least hold the slab's live-count header, and the
        // region must split evenly into base / work.
        if buf.len() < 2 * region::COUNT_HDR {
            return Err(NetabaseError::Storage(StorageErrorKind::BadResource));
        }
        let half = buf.len() / 2;
        // Initialize the committed (base) half to an empty slab. The work half
        // is overwritten from base at each write-transaction begin.
        region::init(&mut buf[..half]);
        Ok(Self {
            buf,
            tables: R::TABLES,
        })
    }

    type NetabaseStoreReadTransaction<'db>
        = ArenaReadTx<'buf, 'db, R>
    where
        Self: 'db,
        R: 'db;
    type NetabaseStoreWriteTransaction<'db>
        = ArenaWriteTx<'buf, 'db, R>
    where
        Self: 'db,
        R: 'db;

    fn read_transaction<'a>(
        &'a self,
    ) -> Result<Self::NetabaseStoreReadTransaction<'a>, NetabaseError>
    where
        R: 'a,
    {
        let half = self.half();
        Ok(ArenaReadTx {
            tables: &self.tables,
            base: &self.buf[..half],
            _marker: PhantomData,
        })
    }

    fn write_transaction<'a>(
        &'a mut self,
    ) -> Result<Self::NetabaseStoreWriteTransaction<'a>, NetabaseError>
    where
        R: 'a,
    {
        let half = self.buf.len() / 2;
        let tables = &self.tables;
        let (base, work) = self.buf.split_at_mut(half);
        // Start the work copy as the current committed state (read-your-writes).
        work.copy_from_slice(base);
        Ok(ArenaWriteTx {
            tables,
            base,
            work: RefCell::new(WorkSlab(work)),
            _marker: PhantomData,
        })
    }
}
