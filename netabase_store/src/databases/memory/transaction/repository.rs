//! Memory-backend transactions: snapshot reads, staged atomic writes.

use crate::databases::memory::tables::{MemoryReadTable, MemoryTableMode, MemoryWriteTable};
use crate::databases::memory::{MemoryMap, MemoryStore};
use crate::errors::NetabaseError;
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

/// Read transaction over a begin-time snapshot of the store.
pub struct MemoryRepoReadTx<'db, R: NetabaseRepository> {
    pub(crate) tables: &'db R::Tables,
    pub(crate) snapshot: MemoryMap,
}

/// Write transaction staging into a working copy; `commit` swaps it into the
/// store, drop discards it.
pub struct MemoryRepoWriteTx<'db, R: NetabaseRepository> {
    pub(crate) tables: &'db R::Tables,
    pub(crate) base: &'db mut MemoryMap,
    pub(crate) work: RefCell<MemoryMap>,
}

// ── Read transaction ─────────────────────────────────────────────────────────

impl<'db, R: NetabaseRepository> NetabaseTransaction<'db, R, MemoryStore<R>>
    for MemoryRepoReadTx<'db, R>
where
    R: 'db,
{
    type Tables = R::Tables;
    fn tables(&self) -> &Self::Tables {
        self.tables
    }
}

impl<'db, R: NetabaseRepository> RepositoryTransaction<'db, R, MemoryStore<R>>
    for MemoryRepoReadTx<'db, R>
where
    R: 'db,
{
    type ReadTable<'txn, O: TableOwner, K: TableKey, V: TableValue>
        = MemoryReadTable<'txn, O, K, V>
    where
        Self: 'txn;

    fn open_read_table<'txn, O: TableOwner, K: TableKey, V: TableValue>(
        &'txn self,
        name: &'static str,
    ) -> Result<Self::ReadTable<'txn, O, K, V>, NetabaseError> {
        Ok(MemoryReadTable {
            map: &self.snapshot,
            name,
            _marker: PhantomData,
        })
    }

    type ReadMultimapTable<'txn, O: TableOwner, K: TableKey, V: TableValue>
        = MemoryReadTable<'txn, O, K, V>
    where
        Self: 'txn;

    fn open_read_multimap_table<'txn, O: TableOwner, K: TableKey, V: TableValue>(
        &'txn self,
        name: &'static str,
    ) -> Result<Self::ReadMultimapTable<'txn, O, K, V>, NetabaseError> {
        Ok(MemoryReadTable {
            map: &self.snapshot,
            name,
            _marker: PhantomData,
        })
    }
}

impl<'db, R: NetabaseRepository> RepositoryReadOps<'db, R, MemoryStore<R>>
    for MemoryRepoReadTx<'db, R>
where
    R: 'db,
{
    fn get(
        &self,
        _address: R::Address,
        key: <R::Keys as NetabaseRepositoryKeys<R>>::PrimaryKey,
    ) -> Result<Option<R>, NetabaseError> {
        R::route_get::<MemoryStore<R>>(self, key)
    }
}

impl<'db, R: NetabaseRepository> RepositoryReadTx<'db, R, MemoryStore<R>>
    for MemoryRepoReadTx<'db, R>
where
    R: 'db,
{
}

// ── Write transaction ────────────────────────────────────────────────────────

impl<'db, R: NetabaseRepository> NetabaseTransaction<'db, R, MemoryStore<R>>
    for MemoryRepoWriteTx<'db, R>
where
    R: 'db,
{
    type Tables = R::Tables;
    fn tables(&self) -> &Self::Tables {
        self.tables
    }
}

impl<'db, R: NetabaseRepository> RepositoryTransaction<'db, R, MemoryStore<R>>
    for MemoryRepoWriteTx<'db, R>
where
    R: 'db,
{
    type ReadTable<'txn, O: TableOwner, K: TableKey, V: TableValue>
        = MemoryWriteTable<'txn, O, K, V>
    where
        Self: 'txn;

    fn open_read_table<'txn, O: TableOwner, K: TableKey, V: TableValue>(
        &'txn self,
        name: &'static str,
    ) -> Result<Self::ReadTable<'txn, O, K, V>, NetabaseError> {
        Ok(MemoryWriteTable {
            work: &self.work,
            name,
            mode: MemoryTableMode::Standard,
            _marker: PhantomData,
        })
    }

    type ReadMultimapTable<'txn, O: TableOwner, K: TableKey, V: TableValue>
        = MemoryWriteTable<'txn, O, K, V>
    where
        Self: 'txn;

    fn open_read_multimap_table<'txn, O: TableOwner, K: TableKey, V: TableValue>(
        &'txn self,
        name: &'static str,
    ) -> Result<Self::ReadMultimapTable<'txn, O, K, V>, NetabaseError> {
        Ok(MemoryWriteTable {
            work: &self.work,
            name,
            mode: MemoryTableMode::Multimap,
            _marker: PhantomData,
        })
    }
}

impl<'db, R: NetabaseRepository> RepositoryReadOps<'db, R, MemoryStore<R>>
    for MemoryRepoWriteTx<'db, R>
where
    R: 'db,
{
    fn get(
        &self,
        _address: R::Address,
        key: <R::Keys as NetabaseRepositoryKeys<R>>::PrimaryKey,
    ) -> Result<Option<R>, NetabaseError> {
        R::route_get::<MemoryStore<R>>(self, key)
    }
}

impl<'db, R: NetabaseRepository> RepositoryReadTx<'db, R, MemoryStore<R>>
    for MemoryRepoWriteTx<'db, R>
where
    R: 'db,
{
}

impl<'db, R: NetabaseRepository> RepositoryWriteOps<'db, R, MemoryStore<R>>
    for MemoryRepoWriteTx<'db, R>
where
    R: 'db,
{
    fn insert(&mut self, item: R) -> Result<(), NetabaseError> {
        item.route_insert::<MemoryStore<R>, _>(self, &InsertConfig::new())
    }

    fn delete(
        &mut self,
        address: R::Address,
        key: <R::Keys as NetabaseRepositoryKeys<R>>::PrimaryKey,
    ) -> Result<(), NetabaseError> {
        R::route_delete::<MemoryStore<R>>(address, key, self)
    }
}

impl<'db, R: NetabaseRepository> RepositoryWriteTx<'db, R, MemoryStore<R>>
    for MemoryRepoWriteTx<'db, R>
where
    R: 'db,
{
    type WriteTable<'txn, O: TableOwner, K: TableKey, V: TableValue>
        = MemoryWriteTable<'txn, O, K, V>
    where
        Self: 'txn;

    fn open_write_table<'txn, O: TableOwner, K: TableKey, V: TableValue>(
        &'txn self,
        name: &'static str,
    ) -> Result<Self::WriteTable<'txn, O, K, V>, NetabaseError> {
        Ok(MemoryWriteTable {
            work: &self.work,
            name,
            mode: MemoryTableMode::Standard,
            _marker: PhantomData,
        })
    }

    type WriteMultimapTable<'txn, O: TableOwner, K: TableKey, V: TableValue>
        = MemoryWriteTable<'txn, O, K, V>
    where
        Self: 'txn;

    fn open_write_multimap_table<'txn, O: TableOwner, K: TableKey, V: TableValue>(
        &'txn self,
        name: &'static str,
    ) -> Result<Self::WriteMultimapTable<'txn, O, K, V>, NetabaseError> {
        Ok(MemoryWriteTable {
            work: &self.work,
            name,
            mode: MemoryTableMode::Multimap,
            _marker: PhantomData,
        })
    }

    /// Atomically publish the working copy.
    fn commit(self) -> Result<(), NetabaseError> {
        *self.base = self.work.into_inner();
        Ok(())
    }
}
