//! redb backend: real ACID transactions over raw byte tables.

pub mod tables;
pub mod transaction;

use crate::databases::redb::transaction::{RedbRepoReadTx, RedbRepoWriteTx};
use crate::errors::{NetabaseError, StorageErrorKind};
use crate::traits::structural::{database::NetabaseStore, schema::repositories::NetabaseRepository};
use redb::ReadableDatabase;

pub struct RedbStore<R: NetabaseRepository> {
    pub(crate) db: redb::Database,
    pub(crate) tables: R::Tables,
}

impl<R: NetabaseRepository> NetabaseStore<R> for RedbStore<R> {
    type Resource = std::path::PathBuf;

    fn open(path: std::path::PathBuf) -> Result<Self, NetabaseError> {
        let db = redb::Database::create(&path)
            .map_err(|e| NetabaseError::backend(StorageErrorKind::Open, e))?;
        Ok(Self {
            db,
            tables: R::TABLES,
        })
    }

    type NetabaseStoreReadTransaction<'db>
        = RedbRepoReadTx<'db, R>
    where
        Self: 'db,
        R: 'db;
    type NetabaseStoreWriteTransaction<'db>
        = RedbRepoWriteTx<'db, R>
    where
        Self: 'db,
        R: 'db;

    fn read_transaction<'a>(
        &'a self,
    ) -> Result<Self::NetabaseStoreReadTransaction<'a>, NetabaseError>
    where
        R: 'a,
    {
        let txn = self
            .db
            .begin_read()
            .map_err(|e| NetabaseError::backend(StorageErrorKind::TransactionBegin, e))?;
        Ok(RedbRepoReadTx {
            tables: &self.tables,
            txn,
        })
    }

    fn write_transaction<'a>(
        &'a mut self,
    ) -> Result<Self::NetabaseStoreWriteTransaction<'a>, NetabaseError>
    where
        R: 'a,
    {
        let txn = self
            .db
            .begin_write()
            .map_err(|e| NetabaseError::backend(StorageErrorKind::TransactionBegin, e))?;
        Ok(RedbRepoWriteTx {
            tables: &self.tables,
            txn,
        })
    }
}
