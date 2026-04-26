// @review [ ]
use crate::errors::NetabaseError;
use crate::traits::behavioural::TransactionHooks;
use crate::traits::structural::database::tables::core::{
    DefinitionTables, NodeStorageMode, TableStorageMode,
};
use crate::traits::structural::database::tables::NetabaseDefinitionAddress;
use crate::traits::structural::schema::models::keys::NetabaseDefinitionKeys;
use crate::traits::structural::schema::repositories::NetabaseRepository;
use crate::traits::structural::schema::models::keys::subscription::SubscriptionOwner;
use crate::traits::structural::database::NetabaseStore;
use crate::traits::structural::database::transactions::repository::{
    RepositoryReadTx, RepositoryWriteTx,
};

pub trait NetabaseDefinition<R: NetabaseRepository>:
    TransactionHooks + SubscriptionOwner<R> + Sized + Send + Sync + 'static
{
    type Address: NetabaseDefinitionAddress<R, Self> + Send + Sync + 'static;
    type Keys: NetabaseDefinitionKeys<R, Self> + Send + Sync + 'static;
    /// The physical table collection for this definition. Owns all orchestration logic
    /// for routing reads/writes to the correct model tables. Reachable from the
    /// repository's table tree without requiring a data instance.
    type Tables: DefinitionTables<R, Self> + Send + Sync + 'static;

    const TABLES: Self::Tables;

    fn route_get<'db, DB: NetabaseStore<R>>(
        txn: &impl RepositoryReadTx<'db, R, DB>,
        key: <Self::Keys as NetabaseDefinitionKeys<R, Self>>::PrimaryKey,
    ) -> Result<Option<Self>, NetabaseError>
    where
        R: 'db,
    {
        Self::TABLES.orchestrate_get(txn, key)
    }

    fn route_insert<'db, DB: NetabaseStore<R>, P>(
        self,
        txn: &mut impl RepositoryWriteTx<'db, R, DB>,
        config: &crate::traits::structural::database::tables::InsertConfig<
            '_,
            <Self::Keys as NetabaseDefinitionKeys<R, Self>>::SubscriptionKeys,
            P,
        >,
    ) -> Result<(), NetabaseError>
    where
        R: 'db,
        P: crate::traits::structural::database::tables::InsertPolicy,
    {
        Self::TABLES.orchestrate_insert(txn, self, config)
    }

    fn route_delete<'db, DB: NetabaseStore<R>>(
        address: Self::Address,
        key: <Self::Keys as NetabaseDefinitionKeys<R, Self>>::PrimaryKey,
        txn: &mut impl RepositoryWriteTx<'db, R, DB>,
    ) -> Result<(), NetabaseError>
    where
        R: 'db,
    {
        Self::TABLES.orchestrate_delete(address, key, txn)
    }

    #[cfg(feature = "std")]
    fn route_fetch_blob_indices<'db, DB: NetabaseStore<R>>(
        &self,
        txn: &impl RepositoryReadTx<'db, R, DB>,
        key: <Self::Keys as NetabaseDefinitionKeys<R, Self>>::PrimaryKey,
    ) -> Result<Vec<<Self::Keys as NetabaseDefinitionKeys<R, Self>>::BlobKeys>, NetabaseError>
    where
        R: 'db,
    {
        Self::TABLES.orchestrate_fetch_blob_indices(txn, key)
    }

    #[cfg(feature = "std")]
    fn route_read_blob_chunks<'db, DB: NetabaseStore<R>>(
        &self,
        txn: &impl RepositoryReadTx<'db, R, DB>,
        indices: Vec<<Self::Keys as NetabaseDefinitionKeys<R, Self>>::BlobKeys>,
    ) -> Result<Vec<Vec<u8>>, NetabaseError>
    where
        R: 'db,
    {
        Self::TABLES.orchestrate_read_blob_chunks(txn, indices)
    }
}

pub trait DefinitionStorageMode<R: NetabaseRepository, D: NetabaseDefinition<R>>:
    NodeStorageMode
{
    const MODE: TableStorageMode = TableStorageMode::Sharded;
}
