//! Arena transactions.
//!
//! - Read tx: borrows the live **base** slab immutably (`&self`); no isolation
//!   machinery is needed because `write_transaction(&mut self)` makes a
//!   concurrent writer impossible.
//! - Write tx: holds the base slab plus a **work** copy. Table ops stage into
//!   `work` (read-your-writes); `commit` copies `work` back over `base` in one
//!   shot (infallible — atomic), and dropping without committing leaves `base`
//!   untouched (rollback).

use super::super::ArenaStore;
use super::super::region_slab::WorkSlab;
use super::super::tables::{ArenaReadTable, ArenaTableMode, ArenaWriteTable};
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

pub struct ArenaReadTx<'buf, 'db, R: NetabaseRepository> {
    pub(crate) tables: &'db R::Tables,
    pub(crate) base: &'db [u8],
    pub(crate) _marker: PhantomData<&'buf ()>,
}

pub struct ArenaWriteTx<'buf, 'db, R: NetabaseRepository> {
    pub(crate) tables: &'db R::Tables,
    /// The live store slab; overwritten by `work` on commit.
    pub(crate) base: &'db mut [u8],
    /// The staged working copy. Reads and writes go here during the txn.
    pub(crate) work: RefCell<WorkSlab<'db>>,
    pub(crate) _marker: PhantomData<&'buf ()>,
}

// ── Read transaction ─────────────────────────────────────────────────────────

impl<'buf, 'db, R: NetabaseRepository> NetabaseTransaction<'db, R, ArenaStore<'buf, R>>
    for ArenaReadTx<'buf, 'db, R>
where
    'buf: 'db,
    R: 'db,
{
    type Tables = R::Tables;
    fn tables(&self) -> &Self::Tables {
        self.tables
    }
}

impl<'buf, 'db, R: NetabaseRepository> RepositoryTransaction<'db, R, ArenaStore<'buf, R>>
    for ArenaReadTx<'buf, 'db, R>
where
    'buf: 'db,
    R: 'db,
{
    type ReadTable<'txn, O: TableOwner, K: TableKey, V: TableValue>
        = ArenaReadTable<'txn, O, K, V>
    where
        Self: 'txn;

    fn open_read_table<'txn, O: TableOwner, K: TableKey, V: TableValue>(
        &'txn self,
        name: &'static str,
    ) -> Result<Self::ReadTable<'txn, O, K, V>, NetabaseError> {
        Ok(ArenaReadTable {
            slab: self.base,
            name,
            _marker: PhantomData,
        })
    }

    type ReadMultimapTable<'txn, O: TableOwner, K: TableKey, V: TableValue>
        = ArenaReadTable<'txn, O, K, V>
    where
        Self: 'txn;

    fn open_read_multimap_table<'txn, O: TableOwner, K: TableKey, V: TableValue>(
        &'txn self,
        name: &'static str,
    ) -> Result<Self::ReadMultimapTable<'txn, O, K, V>, NetabaseError> {
        Ok(ArenaReadTable {
            slab: self.base,
            name,
            _marker: PhantomData,
        })
    }
}

impl<'buf, 'db, R: NetabaseRepository> RepositoryReadOps<'db, R, ArenaStore<'buf, R>>
    for ArenaReadTx<'buf, 'db, R>
where
    'buf: 'db,
    R: 'db,
{
    fn get(
        &self,
        _address: R::Address,
        key: <R::Keys as NetabaseRepositoryKeys<R>>::PrimaryKey,
    ) -> Result<Option<R>, NetabaseError> {
        R::route_get::<ArenaStore<'buf, R>>(self, key)
    }
}

impl<'buf, 'db, R: NetabaseRepository> RepositoryReadTx<'db, R, ArenaStore<'buf, R>>
    for ArenaReadTx<'buf, 'db, R>
where
    'buf: 'db,
    R: 'db,
{
}

// ── Write transaction ────────────────────────────────────────────────────────

impl<'buf, 'db, R: NetabaseRepository> NetabaseTransaction<'db, R, ArenaStore<'buf, R>>
    for ArenaWriteTx<'buf, 'db, R>
where
    'buf: 'db,
    R: 'db,
{
    type Tables = R::Tables;
    fn tables(&self) -> &Self::Tables {
        self.tables
    }
}

impl<'buf, 'db, R: NetabaseRepository> RepositoryTransaction<'db, R, ArenaStore<'buf, R>>
    for ArenaWriteTx<'buf, 'db, R>
where
    'buf: 'db,
    R: 'db,
{
    type ReadTable<'txn, O: TableOwner, K: TableKey, V: TableValue>
        = ArenaWriteTable<'txn, 'db, O, K, V>
    where
        Self: 'txn;

    fn open_read_table<'txn, O: TableOwner, K: TableKey, V: TableValue>(
        &'txn self,
        name: &'static str,
    ) -> Result<Self::ReadTable<'txn, O, K, V>, NetabaseError> {
        Ok(ArenaWriteTable {
            work: &self.work,
            name,
            mode: ArenaTableMode::Standard,
            _marker: PhantomData,
        })
    }

    type ReadMultimapTable<'txn, O: TableOwner, K: TableKey, V: TableValue>
        = ArenaWriteTable<'txn, 'db, O, K, V>
    where
        Self: 'txn;

    fn open_read_multimap_table<'txn, O: TableOwner, K: TableKey, V: TableValue>(
        &'txn self,
        name: &'static str,
    ) -> Result<Self::ReadMultimapTable<'txn, O, K, V>, NetabaseError> {
        Ok(ArenaWriteTable {
            work: &self.work,
            name,
            mode: ArenaTableMode::Multimap,
            _marker: PhantomData,
        })
    }
}

impl<'buf, 'db, R: NetabaseRepository> RepositoryReadOps<'db, R, ArenaStore<'buf, R>>
    for ArenaWriteTx<'buf, 'db, R>
where
    'buf: 'db,
    R: 'db,
{
    fn get(
        &self,
        _address: R::Address,
        key: <R::Keys as NetabaseRepositoryKeys<R>>::PrimaryKey,
    ) -> Result<Option<R>, NetabaseError> {
        R::route_get::<ArenaStore<'buf, R>>(self, key)
    }
}

impl<'buf, 'db, R: NetabaseRepository> RepositoryReadTx<'db, R, ArenaStore<'buf, R>>
    for ArenaWriteTx<'buf, 'db, R>
where
    'buf: 'db,
    R: 'db,
{
}

impl<'buf, 'db, R: NetabaseRepository> RepositoryWriteOps<'db, R, ArenaStore<'buf, R>>
    for ArenaWriteTx<'buf, 'db, R>
where
    'buf: 'db,
    R: 'db,
{
    fn insert(&mut self, item: R) -> Result<(), NetabaseError> {
        item.route_insert::<ArenaStore<'buf, R>, _>(self, &InsertConfig::new())
    }

    fn delete(
        &mut self,
        address: R::Address,
        key: <R::Keys as NetabaseRepositoryKeys<R>>::PrimaryKey,
    ) -> Result<(), NetabaseError> {
        R::route_delete::<ArenaStore<'buf, R>>(address, key, self)
    }
}

impl<'buf, 'db, R: NetabaseRepository> RepositoryWriteTx<'db, R, ArenaStore<'buf, R>>
    for ArenaWriteTx<'buf, 'db, R>
where
    'buf: 'db,
    R: 'db,
{
    type WriteTable<'txn, O: TableOwner, K: TableKey, V: TableValue>
        = ArenaWriteTable<'txn, 'db, O, K, V>
    where
        Self: 'txn;

    fn open_write_table<'txn, O: TableOwner, K: TableKey, V: TableValue>(
        &'txn self,
        name: &'static str,
    ) -> Result<Self::WriteTable<'txn, O, K, V>, NetabaseError> {
        Ok(ArenaWriteTable {
            work: &self.work,
            name,
            mode: ArenaTableMode::Standard,
            _marker: PhantomData,
        })
    }

    type WriteMultimapTable<'txn, O: TableOwner, K: TableKey, V: TableValue>
        = ArenaWriteTable<'txn, 'db, O, K, V>
    where
        Self: 'txn;

    fn open_write_multimap_table<'txn, O: TableOwner, K: TableKey, V: TableValue>(
        &'txn self,
        name: &'static str,
    ) -> Result<Self::WriteMultimapTable<'txn, O, K, V>, NetabaseError> {
        Ok(ArenaWriteTable {
            work: &self.work,
            name,
            mode: ArenaTableMode::Multimap,
            _marker: PhantomData,
        })
    }

    /// Publish the staged work copy over the base slab in one shot — atomic and
    /// infallible (capacity was already enforced at insert time).
    fn commit(self) -> Result<(), NetabaseError> {
        let work = self.work.into_inner();
        let len = self.base.len();
        self.base.copy_from_slice(&work.0[..len]);
        Ok(())
    }
}
