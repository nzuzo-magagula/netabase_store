//! redb transactions: ACID reads/writes over raw byte tables.

use crate::databases::redb::RedbStore;
use crate::databases::redb::tables::{
    BytesMultimapDef, BytesTableDef, RedbReadMultimapTable, RedbReadTable, RedbWriteMultimapTable,
    RedbWriteTable,
};
use crate::errors::{NetabaseError, StorageErrorKind};
use crate::traits::structural::database::tables::InsertConfig;
use crate::traits::structural::database::tables::core::{TableKey, TableOwner, TableValue};
use crate::traits::structural::{
    database::transactions::NetabaseTransaction,
    database::transactions::definition::{
        DefinitionTransaction, DefinitionWriteOps, DefinitionWriteTx,
    },
    database::transactions::repository::{
        RepositoryReadOps, RepositoryReadTx, RepositoryTransaction, RepositoryWriteOps,
        RepositoryWriteTx,
    },
    schema::{
        definitions::NetabaseDefinition,
        models::keys::{NetabaseDefinitionKeys, NetabaseRepositoryKeys},
        repositories::NetabaseRepository,
    },
};
use core::marker::PhantomData;

pub struct RedbRepoReadTx<'db, R: NetabaseRepository> {
    pub(crate) tables: &'db R::Tables,
    pub(crate) txn: redb::ReadTransaction,
}

pub struct RedbRepoWriteTx<'db, R: NetabaseRepository> {
    pub(crate) tables: &'db R::Tables,
    pub(crate) txn: redb::WriteTransaction,
}

pub struct RedbDefinitionTransaction<'db, R: NetabaseRepository, D: NetabaseDefinition<R>> {
    pub(crate) parent: &'db mut RedbRepoWriteTx<'db, R>,
    pub(crate) _marker: PhantomData<D>,
}

fn table_open(err: impl core::error::Error + Send + Sync + 'static) -> NetabaseError {
    NetabaseError::backend(StorageErrorKind::TableOpen, err)
}

// ── Read transaction ─────────────────────────────────────────────────────────

impl<'db, R: NetabaseRepository> NetabaseTransaction<'db, R, RedbStore<R>>
    for RedbRepoReadTx<'db, R>
where
    R: 'db,
{
    type Tables = R::Tables;
    fn tables(&self) -> &Self::Tables {
        self.tables
    }
}

impl<'db, R: NetabaseRepository> RepositoryTransaction<'db, R, RedbStore<R>>
    for RedbRepoReadTx<'db, R>
where
    R: 'db,
{
    type ReadTable<'txn, O: TableOwner, K: TableKey, V: TableValue>
        = RedbReadTable<O, K, V>
    where
        Self: 'txn;

    fn open_read_table<'txn, O: TableOwner, K: TableKey, V: TableValue>(
        &'txn self,
        name: &'static str,
    ) -> Result<Self::ReadTable<'txn, O, K, V>, NetabaseError> {
        let table = self
            .txn
            .open_table(BytesTableDef::new(name))
            .map_err(table_open)?;
        Ok(RedbReadTable {
            table,
            _marker: PhantomData,
        })
    }

    type ReadMultimapTable<'txn, O: TableOwner, K: TableKey, V: TableValue>
        = RedbReadMultimapTable<O, K, V>
    where
        Self: 'txn;

    fn open_read_multimap_table<'txn, O: TableOwner, K: TableKey, V: TableValue>(
        &'txn self,
        name: &'static str,
    ) -> Result<Self::ReadMultimapTable<'txn, O, K, V>, NetabaseError> {
        let table = self
            .txn
            .open_multimap_table(BytesMultimapDef::new(name))
            .map_err(table_open)?;
        Ok(RedbReadMultimapTable {
            table,
            _marker: PhantomData,
        })
    }
}

impl<'db, R: NetabaseRepository> RepositoryReadOps<'db, R, RedbStore<R>> for RedbRepoReadTx<'db, R>
where
    R: 'db,
{
    fn get(
        &self,
        _address: R::Address,
        key: <R::Keys as NetabaseRepositoryKeys<R>>::PrimaryKey,
    ) -> Result<Option<R>, NetabaseError> {
        R::route_get::<RedbStore<R>>(self, key)
    }
}

impl<'db, R: NetabaseRepository> RepositoryReadTx<'db, R, RedbStore<R>> for RedbRepoReadTx<'db, R> where
    R: 'db
{
}

// ── Write transaction ────────────────────────────────────────────────────────

impl<'db, R: NetabaseRepository> NetabaseTransaction<'db, R, RedbStore<R>>
    for RedbRepoWriteTx<'db, R>
where
    R: 'db,
{
    type Tables = R::Tables;
    fn tables(&self) -> &Self::Tables {
        self.tables
    }
}

impl<'db, R: NetabaseRepository> RepositoryTransaction<'db, R, RedbStore<R>>
    for RedbRepoWriteTx<'db, R>
where
    R: 'db,
{
    type ReadTable<'txn, O: TableOwner, K: TableKey, V: TableValue>
        = RedbWriteTable<'txn, O, K, V>
    where
        Self: 'txn;

    fn open_read_table<'txn, O: TableOwner, K: TableKey, V: TableValue>(
        &'txn self,
        name: &'static str,
    ) -> Result<Self::ReadTable<'txn, O, K, V>, NetabaseError> {
        let table = self
            .txn
            .open_table(BytesTableDef::new(name))
            .map_err(table_open)?;
        Ok(RedbWriteTable {
            table,
            _marker: PhantomData,
        })
    }

    type ReadMultimapTable<'txn, O: TableOwner, K: TableKey, V: TableValue>
        = RedbWriteMultimapTable<'txn, O, K, V>
    where
        Self: 'txn;

    fn open_read_multimap_table<'txn, O: TableOwner, K: TableKey, V: TableValue>(
        &'txn self,
        name: &'static str,
    ) -> Result<Self::ReadMultimapTable<'txn, O, K, V>, NetabaseError> {
        let table = self
            .txn
            .open_multimap_table(BytesMultimapDef::new(name))
            .map_err(table_open)?;
        Ok(RedbWriteMultimapTable {
            table,
            _marker: PhantomData,
        })
    }
}

impl<'db, R: NetabaseRepository> RepositoryReadOps<'db, R, RedbStore<R>> for RedbRepoWriteTx<'db, R>
where
    R: 'db,
{
    fn get(
        &self,
        _address: R::Address,
        key: <R::Keys as NetabaseRepositoryKeys<R>>::PrimaryKey,
    ) -> Result<Option<R>, NetabaseError> {
        R::route_get::<RedbStore<R>>(self, key)
    }
}

impl<'db, R: NetabaseRepository> RepositoryReadTx<'db, R, RedbStore<R>> for RedbRepoWriteTx<'db, R> where
    R: 'db
{
}

impl<'db, R: NetabaseRepository> RepositoryWriteOps<'db, R, RedbStore<R>>
    for RedbRepoWriteTx<'db, R>
where
    R: 'db,
{
    fn insert(&mut self, item: R) -> Result<(), NetabaseError> {
        item.route_insert::<RedbStore<R>, _>(self, &InsertConfig::new())
    }

    fn delete(
        &mut self,
        address: R::Address,
        key: <R::Keys as NetabaseRepositoryKeys<R>>::PrimaryKey,
    ) -> Result<(), NetabaseError> {
        R::route_delete::<RedbStore<R>>(address, key, self)
    }
}

impl<'db, R: NetabaseRepository> RepositoryWriteTx<'db, R, RedbStore<R>> for RedbRepoWriteTx<'db, R>
where
    R: 'db,
{
    type WriteTable<'txn, O: TableOwner, K: TableKey, V: TableValue>
        = RedbWriteTable<'txn, O, K, V>
    where
        Self: 'txn;

    fn open_write_table<'txn, O: TableOwner, K: TableKey, V: TableValue>(
        &'txn self,
        name: &'static str,
    ) -> Result<Self::WriteTable<'txn, O, K, V>, NetabaseError> {
        let table = self
            .txn
            .open_table(BytesTableDef::new(name))
            .map_err(table_open)?;
        Ok(RedbWriteTable {
            table,
            _marker: PhantomData,
        })
    }

    type WriteMultimapTable<'txn, O: TableOwner, K: TableKey, V: TableValue>
        = RedbWriteMultimapTable<'txn, O, K, V>
    where
        Self: 'txn;

    fn open_write_multimap_table<'txn, O: TableOwner, K: TableKey, V: TableValue>(
        &'txn self,
        name: &'static str,
    ) -> Result<Self::WriteMultimapTable<'txn, O, K, V>, NetabaseError> {
        let table = self
            .txn
            .open_multimap_table(BytesMultimapDef::new(name))
            .map_err(table_open)?;
        Ok(RedbWriteMultimapTable {
            table,
            _marker: PhantomData,
        })
    }

    fn commit(self) -> Result<(), NetabaseError> {
        self.txn
            .commit()
            .map_err(|e| NetabaseError::backend(StorageErrorKind::Commit, e))
    }
}

// ── Definition passthrough transaction ───────────────────────────────────────

impl<'db, R: NetabaseRepository, D: NetabaseDefinition<R>> NetabaseTransaction<'db, R, RedbStore<R>>
    for RedbDefinitionTransaction<'db, R, D>
where
    R: 'db,
{
    type Tables = R::Tables;
    fn tables(&self) -> &Self::Tables {
        self.parent.tables
    }
}

impl<'db, R: NetabaseRepository, D: NetabaseDefinition<R>>
    RepositoryTransaction<'db, R, RedbStore<R>> for RedbDefinitionTransaction<'db, R, D>
where
    R: 'db,
{
    type ReadTable<'txn, O: TableOwner, K: TableKey, V: TableValue>
        = RedbWriteTable<'txn, O, K, V>
    where
        Self: 'txn;

    fn open_read_table<'txn, O: TableOwner, K: TableKey, V: TableValue>(
        &'txn self,
        name: &'static str,
    ) -> Result<Self::ReadTable<'txn, O, K, V>, NetabaseError> {
        self.parent.open_read_table::<O, K, V>(name)
    }

    type ReadMultimapTable<'txn, O: TableOwner, K: TableKey, V: TableValue>
        = RedbWriteMultimapTable<'txn, O, K, V>
    where
        Self: 'txn;

    fn open_read_multimap_table<'txn, O: TableOwner, K: TableKey, V: TableValue>(
        &'txn self,
        name: &'static str,
    ) -> Result<Self::ReadMultimapTable<'txn, O, K, V>, NetabaseError> {
        self.parent.open_read_multimap_table::<O, K, V>(name)
    }
}

impl<'db, R: NetabaseRepository, D: NetabaseDefinition<R>> RepositoryReadOps<'db, R, RedbStore<R>>
    for RedbDefinitionTransaction<'db, R, D>
where
    R: 'db,
{
    fn get(
        &self,
        address: R::Address,
        key: <R::Keys as NetabaseRepositoryKeys<R>>::PrimaryKey,
    ) -> Result<Option<R>, NetabaseError> {
        self.parent.get(address, key)
    }
}

impl<'db, R: NetabaseRepository, D: NetabaseDefinition<R>> RepositoryReadTx<'db, R, RedbStore<R>>
    for RedbDefinitionTransaction<'db, R, D>
where
    R: 'db,
{
}

impl<'db, R: NetabaseRepository, D: NetabaseDefinition<R>> RepositoryWriteOps<'db, R, RedbStore<R>>
    for RedbDefinitionTransaction<'db, R, D>
where
    R: 'db,
{
    fn insert(&mut self, item: R) -> Result<(), NetabaseError> {
        self.parent.insert(item)
    }

    fn delete(
        &mut self,
        address: R::Address,
        key: <R::Keys as NetabaseRepositoryKeys<R>>::PrimaryKey,
    ) -> Result<(), NetabaseError> {
        self.parent.delete(address, key)
    }
}

// RedbDefinitionTransaction::commit() is a no-op — it does NOT commit the underlying redb write
// transaction. Only the parent RedbRepoWriteTx::commit() flushes data to disk.
impl<'db, R: NetabaseRepository, D: NetabaseDefinition<R>>
    DefinitionTransaction<'db, R, D, RedbStore<R>> for RedbDefinitionTransaction<'db, R, D>
where
    R: 'db,
{
}

impl<'db, R: NetabaseRepository, D: NetabaseDefinition<R>> RepositoryWriteTx<'db, R, RedbStore<R>>
    for RedbDefinitionTransaction<'db, R, D>
where
    R: 'db,
{
    type WriteTable<'txn, O: TableOwner, K: TableKey, V: TableValue>
        = RedbWriteTable<'txn, O, K, V>
    where
        Self: 'txn;

    fn open_write_table<'txn, O: TableOwner, K: TableKey, V: TableValue>(
        &'txn self,
        name: &'static str,
    ) -> Result<Self::WriteTable<'txn, O, K, V>, NetabaseError> {
        self.parent.open_write_table::<O, K, V>(name)
    }

    type WriteMultimapTable<'txn, O: TableOwner, K: TableKey, V: TableValue>
        = RedbWriteMultimapTable<'txn, O, K, V>
    where
        Self: 'txn;

    fn open_write_multimap_table<'txn, O: TableOwner, K: TableKey, V: TableValue>(
        &'txn self,
        name: &'static str,
    ) -> Result<Self::WriteMultimapTable<'txn, O, K, V>, NetabaseError> {
        self.parent.open_write_multimap_table::<O, K, V>(name)
    }

    fn commit(self) -> Result<(), NetabaseError> {
        Ok(())
    }
}

impl<'db, R: NetabaseRepository, D: NetabaseDefinition<R>>
    DefinitionWriteOps<'db, R, D, RedbStore<R>> for RedbDefinitionTransaction<'db, R, D>
where
    R: 'db,
{
    fn insert(&mut self, item: D) -> Result<(), NetabaseError> {
        item.route_insert::<RedbStore<R>, _>(self, &InsertConfig::new())
    }

    fn delete(
        &mut self,
        address: D::Address,
        key: <D::Keys as NetabaseDefinitionKeys<R, D>>::PrimaryKey,
    ) -> Result<(), NetabaseError> {
        D::route_delete::<RedbStore<R>>(address, key, self)
    }
}

impl<'db, R: NetabaseRepository, D: NetabaseDefinition<R>>
    DefinitionWriteTx<'db, R, D, RedbStore<R>> for RedbDefinitionTransaction<'db, R, D>
where
    R: 'db,
{
}
