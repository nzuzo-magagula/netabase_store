//! Repository-level transaction traits.
//!
//! Backend-agnostic by construction: tables are opened by **static name**
//! and typed as `(O, K, V)` where `K: TableKey` (order-preserving byte
//! encoding) and `V: TableValue` (canonical rkyv bytes). No backend types
//! appear in any bound — a backend stores encoded key bytes against value
//! bytes and never needs to know the typed forms.

use crate::errors::NetabaseError;
use crate::traits::structural::{
    database::NetabaseStore,
    database::tables::core::{
        StoreTable, TableKey, TableOwner, TableReadOps, TableValue, TableWriteOps,
    },
    schema::repositories::NetabaseRepository,
};

pub trait RepositoryTransaction<'db, R: NetabaseRepository, DB: NetabaseStore<R>>:
    crate::traits::structural::database::transactions::NetabaseTransaction<'db, R, DB>
{
    /// Read handle for a plain (one value per key) table.
    type ReadTable<'txn, O: TableOwner, K: TableKey, V: TableValue>: TableReadOps<K, V>
        + StoreTable<O, K, V>
    where
        Self: 'txn;

    fn open_read_table<'txn, O: TableOwner, K: TableKey, V: TableValue>(
        &'txn self,
        name: &'static str,
    ) -> Result<Self::ReadTable<'txn, O, K, V>, NetabaseError>;

    /// Read handle for a multimap (many values per key) table.
    type ReadMultimapTable<'txn, O: TableOwner, K: TableKey, V: TableValue>: TableReadOps<K, V>
        + StoreTable<O, K, V>
    where
        Self: 'txn;

    fn open_read_multimap_table<'txn, O: TableOwner, K: TableKey, V: TableValue>(
        &'txn self,
        name: &'static str,
    ) -> Result<Self::ReadMultimapTable<'txn, O, K, V>, NetabaseError>;
}

pub trait RepositoryReadOps<'db, R: NetabaseRepository, DB: NetabaseStore<R>> {
    fn get(
        &self,
        address: R::Address,
        key: <R::Keys as crate::traits::structural::schema::models::keys::NetabaseRepositoryKeys<
            R,
        >>::PrimaryKey,
    ) -> Result<Option<R>, NetabaseError>;
}

pub trait RepositoryWriteOps<'db, R: NetabaseRepository, DB: NetabaseStore<R>> {
    fn insert(&mut self, item: R) -> Result<(), NetabaseError>;
    fn delete(
        &mut self,
        address: R::Address,
        key: <R::Keys as crate::traits::structural::schema::models::keys::NetabaseRepositoryKeys<
            R,
        >>::PrimaryKey,
    ) -> Result<(), NetabaseError>;
}

pub trait RepositoryReadTx<'db, R: NetabaseRepository, DB: NetabaseStore<R>>:
    RepositoryTransaction<'db, R, DB> + RepositoryReadOps<'db, R, DB>
{
}

pub trait RepositoryWriteTx<'db, R: NetabaseRepository, DB: NetabaseStore<R>>:
    RepositoryTransaction<'db, R, DB> + RepositoryWriteOps<'db, R, DB>
{
    /// Write handle for a plain table.
    type WriteTable<'txn, O: TableOwner, K: TableKey, V: TableValue>: TableWriteOps<K, V>
        + StoreTable<O, K, V>
    where
        Self: 'txn;

    fn open_write_table<'txn, O: TableOwner, K: TableKey, V: TableValue>(
        &'txn self,
        name: &'static str,
    ) -> Result<Self::WriteTable<'txn, O, K, V>, NetabaseError>;

    /// Write handle for a multimap table.
    type WriteMultimapTable<'txn, O: TableOwner, K: TableKey, V: TableValue>: TableWriteOps<K, V>
        + StoreTable<O, K, V>
    where
        Self: 'txn;

    fn open_write_multimap_table<'txn, O: TableOwner, K: TableKey, V: TableValue>(
        &'txn self,
        name: &'static str,
    ) -> Result<Self::WriteMultimapTable<'txn, O, K, V>, NetabaseError>;

    /// Atomically apply every write made through this transaction. Dropping
    /// without committing discards them.
    fn commit(self) -> Result<(), NetabaseError>;
}
