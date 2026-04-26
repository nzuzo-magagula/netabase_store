//! fjall transactions: snapshot reads, single-writer atomic writes.

use super::super::FjallStore;
use super::super::keyspace;
use super::super::tables::{
    FjallReadMultimapTable, FjallReadTable, FjallWriteMultimapTable, FjallWriteTable,
};
use crate::errors::{NetabaseError, StorageErrorKind};
use crate::traits::structural::database::tables::InsertConfig;
use crate::traits::structural::database::tables::core::{TableKey, TableOwner, TableValue};
use crate::traits::structural::{
    database::transactions::NetabaseTransaction,
    database::transactions::repository::{
        RepositoryReadOps, RepositoryReadTx, RepositoryTransaction, RepositoryWriteOps,
        RepositoryWriteTx,
    },
    schema::models::keys::NetabaseRepositoryKeys,
    schema::repositories::NetabaseRepository,
};
use core::cell::RefCell;
use core::marker::PhantomData;
use fjall::{SingleWriterTxDatabase, SingleWriterWriteTx, Snapshot};

pub struct FjallRepoReadTx<'db, R: NetabaseRepository> {
    pub(crate) tables: &'db R::Tables,
    pub(crate) db: &'db SingleWriterTxDatabase,
    pub(crate) snapshot: Snapshot,
}

pub struct FjallRepoWriteTx<'db, R: NetabaseRepository> {
    pub(crate) tables: &'db R::Tables,
    pub(crate) db: &'db SingleWriterTxDatabase,
    pub(crate) tx: RefCell<SingleWriterWriteTx<'db>>,
}

// ── Read transaction ─────────────────────────────────────────────────────────

impl<'db, R: NetabaseRepository> NetabaseTransaction<'db, R, FjallStore<R>>
    for FjallRepoReadTx<'db, R>
where
    R: 'db,
{
    type Tables = R::Tables;
    fn tables(&self) -> &Self::Tables {
        self.tables
    }
}

impl<'db, R: NetabaseRepository> RepositoryTransaction<'db, R, FjallStore<R>>
    for FjallRepoReadTx<'db, R>
where
    R: 'db,
{
    type ReadTable<'txn, O: TableOwner, K: TableKey, V: TableValue>
        = FjallReadTable<'txn, O, K, V>
    where
        Self: 'txn;

    fn open_read_table<'txn, O: TableOwner, K: TableKey, V: TableValue>(
        &'txn self,
        name: &'static str,
    ) -> Result<Self::ReadTable<'txn, O, K, V>, NetabaseError> {
        Ok(FjallReadTable {
            snapshot: &self.snapshot,
            keyspace: keyspace(self.db, name)?,
            _marker: PhantomData,
        })
    }

    type ReadMultimapTable<'txn, O: TableOwner, K: TableKey, V: TableValue>
        = FjallReadMultimapTable<'txn, O, K, V>
    where
        Self: 'txn;

    fn open_read_multimap_table<'txn, O: TableOwner, K: TableKey, V: TableValue>(
        &'txn self,
        name: &'static str,
    ) -> Result<Self::ReadMultimapTable<'txn, O, K, V>, NetabaseError> {
        Ok(FjallReadMultimapTable {
            snapshot: &self.snapshot,
            keyspace: keyspace(self.db, name)?,
            _marker: PhantomData,
        })
    }
}

impl<'db, R: NetabaseRepository> RepositoryReadOps<'db, R, FjallStore<R>> for FjallRepoReadTx<'db, R>
where
    R: 'db,
{
    fn get(
        &self,
        _address: R::Address,
        key: <R::Keys as NetabaseRepositoryKeys<R>>::PrimaryKey,
    ) -> Result<Option<R>, NetabaseError> {
        R::route_get::<FjallStore<R>>(self, key)
    }
}

impl<'db, R: NetabaseRepository> RepositoryReadTx<'db, R, FjallStore<R>> for FjallRepoReadTx<'db, R> where
    R: 'db
{
}

// ── Write transaction ────────────────────────────────────────────────────────

impl<'db, R: NetabaseRepository> NetabaseTransaction<'db, R, FjallStore<R>>
    for FjallRepoWriteTx<'db, R>
where
    R: 'db,
{
    type Tables = R::Tables;
    fn tables(&self) -> &Self::Tables {
        self.tables
    }
}

impl<'db, R: NetabaseRepository> RepositoryTransaction<'db, R, FjallStore<R>>
    for FjallRepoWriteTx<'db, R>
where
    R: 'db,
{
    type ReadTable<'txn, O: TableOwner, K: TableKey, V: TableValue>
        = FjallWriteTable<'txn, 'db, O, K, V>
    where
        Self: 'txn;

    fn open_read_table<'txn, O: TableOwner, K: TableKey, V: TableValue>(
        &'txn self,
        name: &'static str,
    ) -> Result<Self::ReadTable<'txn, O, K, V>, NetabaseError> {
        Ok(FjallWriteTable {
            tx: &self.tx,
            keyspace: keyspace(self.db, name)?,
            _marker: PhantomData,
        })
    }

    type ReadMultimapTable<'txn, O: TableOwner, K: TableKey, V: TableValue>
        = FjallWriteMultimapTable<'txn, 'db, O, K, V>
    where
        Self: 'txn;

    fn open_read_multimap_table<'txn, O: TableOwner, K: TableKey, V: TableValue>(
        &'txn self,
        name: &'static str,
    ) -> Result<Self::ReadMultimapTable<'txn, O, K, V>, NetabaseError> {
        Ok(FjallWriteMultimapTable {
            tx: &self.tx,
            keyspace: keyspace(self.db, name)?,
            _marker: PhantomData,
        })
    }
}

impl<'db, R: NetabaseRepository> RepositoryReadOps<'db, R, FjallStore<R>>
    for FjallRepoWriteTx<'db, R>
where
    R: 'db,
{
    fn get(
        &self,
        _address: R::Address,
        key: <R::Keys as NetabaseRepositoryKeys<R>>::PrimaryKey,
    ) -> Result<Option<R>, NetabaseError> {
        R::route_get::<FjallStore<R>>(self, key)
    }
}

impl<'db, R: NetabaseRepository> RepositoryReadTx<'db, R, FjallStore<R>>
    for FjallRepoWriteTx<'db, R>
where
    R: 'db,
{
}

impl<'db, R: NetabaseRepository> RepositoryWriteOps<'db, R, FjallStore<R>>
    for FjallRepoWriteTx<'db, R>
where
    R: 'db,
{
    fn insert(&mut self, item: R) -> Result<(), NetabaseError> {
        item.route_insert::<FjallStore<R>, _>(self, &InsertConfig::new())
    }

    fn delete(
        &mut self,
        address: R::Address,
        key: <R::Keys as NetabaseRepositoryKeys<R>>::PrimaryKey,
    ) -> Result<(), NetabaseError> {
        R::route_delete::<FjallStore<R>>(address, key, self)
    }
}

impl<'db, R: NetabaseRepository> RepositoryWriteTx<'db, R, FjallStore<R>>
    for FjallRepoWriteTx<'db, R>
where
    R: 'db,
{
    type WriteTable<'txn, O: TableOwner, K: TableKey, V: TableValue>
        = FjallWriteTable<'txn, 'db, O, K, V>
    where
        Self: 'txn;

    fn open_write_table<'txn, O: TableOwner, K: TableKey, V: TableValue>(
        &'txn self,
        name: &'static str,
    ) -> Result<Self::WriteTable<'txn, O, K, V>, NetabaseError> {
        Ok(FjallWriteTable {
            tx: &self.tx,
            keyspace: keyspace(self.db, name)?,
            _marker: PhantomData,
        })
    }

    type WriteMultimapTable<'txn, O: TableOwner, K: TableKey, V: TableValue>
        = FjallWriteMultimapTable<'txn, 'db, O, K, V>
    where
        Self: 'txn;

    fn open_write_multimap_table<'txn, O: TableOwner, K: TableKey, V: TableValue>(
        &'txn self,
        name: &'static str,
    ) -> Result<Self::WriteMultimapTable<'txn, O, K, V>, NetabaseError> {
        Ok(FjallWriteMultimapTable {
            tx: &self.tx,
            keyspace: keyspace(self.db, name)?,
            _marker: PhantomData,
        })
    }

    fn commit(self) -> Result<(), NetabaseError> {
        self.tx
            .into_inner()
            .commit()
            .map_err(|e| NetabaseError::backend(StorageErrorKind::Commit, e))
    }
}
