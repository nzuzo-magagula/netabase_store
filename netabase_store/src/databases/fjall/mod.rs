//! fjall backend: an LSM key/value store with real single-writer
//! transactions and key-composition multimaps.

pub mod tables;
pub mod transaction;

use crate::errors::{NetabaseError, StorageErrorKind};
use crate::traits::structural::{database::NetabaseStore, schema::repositories::NetabaseRepository};
use fjall::{KeyspaceCreateOptions, SingleWriterTxDatabase, SingleWriterTxKeyspace};
use transaction::{FjallRepoReadTx, FjallRepoWriteTx};

pub struct FjallStore<R: NetabaseRepository> {
    pub(crate) db: SingleWriterTxDatabase,
    pub(crate) tables: R::Tables,
}

/// Open (creating if absent) the keyspace backing one table. Idempotent — the
/// per-table partition is the physical table.
pub(crate) fn keyspace(
    db: &SingleWriterTxDatabase,
    name: &'static str,
) -> Result<SingleWriterTxKeyspace, NetabaseError> {
    db.keyspace(name, KeyspaceCreateOptions::default)
        .map_err(|e| NetabaseError::backend(StorageErrorKind::TableOpen, e))
}

impl<R: NetabaseRepository> NetabaseStore<R> for FjallStore<R> {
    type Resource = std::path::PathBuf;

    fn open(path: std::path::PathBuf) -> Result<Self, NetabaseError> {
        let db = SingleWriterTxDatabase::builder(&path)
            .open()
            .map_err(|e| NetabaseError::backend(StorageErrorKind::Open, e))?;
        Ok(Self {
            db,
            tables: R::TABLES,
        })
    }

    type NetabaseStoreReadTransaction<'db>
        = FjallRepoReadTx<'db, R>
    where
        Self: 'db,
        R: 'db;
    type NetabaseStoreWriteTransaction<'db>
        = FjallRepoWriteTx<'db, R>
    where
        Self: 'db,
        R: 'db;

    fn read_transaction<'a>(
        &'a self,
    ) -> Result<Self::NetabaseStoreReadTransaction<'a>, NetabaseError>
    where
        R: 'a,
    {
        Ok(FjallRepoReadTx {
            tables: &self.tables,
            db: &self.db,
            snapshot: self.db.read_tx(),
        })
    }

    fn write_transaction<'a>(
        &'a mut self,
    ) -> Result<Self::NetabaseStoreWriteTransaction<'a>, NetabaseError>
    where
        R: 'a,
    {
        let tables = &self.tables;
        let db = &self.db;
        let tx = self.db.write_tx();
        Ok(FjallRepoWriteTx {
            tables,
            db,
            tx: core::cell::RefCell::new(tx),
        })
    }
}
