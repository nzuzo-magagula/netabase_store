// @review [ ]
use crate::errors::NetabaseError;
use crate::traits::behavioural::TransactionHooks;
use crate::traits::structural::database::NetabaseStore;
use crate::traits::structural::database::tables::{
    NetabaseRepositoryAddress, RepositoryTables, TableOwner,
};
use crate::traits::structural::database::transactions::repository::{
    RepositoryReadTx, RepositoryWriteTx,
};
use crate::traits::structural::schema::models::keys::{
    NetabaseRepositoryKeys, RepositoryBlobKeys, RepositoryRelationalKeys,
    RepositorySecondaryKeys, RepositorySubscriptionKeys,
};
use crate::traits::structural::schema::models::keys::subscription::SubscriptionOwner;

pub trait NetabaseRepository:
    TransactionHooks + SubscriptionOwner<Self> + Sized + Send + Sync + 'static
{
    type Address: NetabaseRepositoryAddress<Self> + Send + Sync + 'static;
    type Keys: NetabaseRepositoryKeys<Self> + Send + Sync + 'static;
    /// The physical table collection for this repository. Owns the orchestration logic
    /// that routes operations down through its definition children. Root of the physical
    /// table tree; reachable without needing a data instance.
    type Tables: RepositoryTables<Self> + Send + Sync + 'static;

    const TABLES: Self::Tables;

    fn route_get<'db, DB: NetabaseStore<Self>>(
        txn: &impl RepositoryReadTx<'db, Self, DB>,
        key: <Self::Keys as NetabaseRepositoryKeys<Self>>::PrimaryKey,
    ) -> Result<Option<Self>, NetabaseError>
    where
        Self: 'db,
    {
        Self::TABLES.orchestrate_get(txn, key)
    }

    fn route_insert<'db, DB: NetabaseStore<Self>, P>(
        self,
        txn: &mut impl RepositoryWriteTx<'db, Self, DB>,
        config: &crate::traits::structural::database::tables::InsertConfig<
            '_,
            <Self::Keys as NetabaseRepositoryKeys<Self>>::SubscriptionKeys,
            P,
        >,
    ) -> Result<(), NetabaseError>
    where
        Self: 'db,
        P: crate::traits::structural::database::tables::InsertPolicy,
    {
        Self::TABLES.orchestrate_insert(txn, self, config)
    }

    fn route_delete<'db, DB: NetabaseStore<Self>>(
        address: Self::Address,
        key: <Self::Keys as NetabaseRepositoryKeys<Self>>::PrimaryKey,
        txn: &mut impl RepositoryWriteTx<'db, Self, DB>,
    ) -> Result<(), NetabaseError>
    where
        Self: 'db,
    {
        Self::TABLES.orchestrate_delete(address, key, txn)
    }
}

pub struct NoRepository;
impl TransactionHooks for NoRepository {}
impl TableOwner for NoRepository {}
impl SubscriptionOwner<Self> for NoRepository {
    type SubscriptionsEnum = NoSubscriptions;
}
pub struct NoSubscriptions;
impl
    crate::traits::structural::schema::models::keys::subscription::SubscriptionKeysEnum<
        NoRepository,
        NoRepository,
    > for NoSubscriptions
{
}

impl NetabaseRepositoryAddress<NoRepository> for () {}
impl RepositorySecondaryKeys<NoRepository> for () {}
impl RepositoryRelationalKeys<NoRepository> for () {}
impl RepositoryBlobKeys<NoRepository> for () {}
impl RepositorySubscriptionKeys<NoRepository> for () {}
impl NetabaseRepositoryKeys<NoRepository> for () {
    type PrimaryKey = ();
    type SecondaryKeys = ();
    type RelationalKeys = ();
    type BlobKeys = ();
    type SubscriptionKeys = ();
}
impl RepositoryTables<NoRepository> for () {
    type Config = ();

    fn orchestrate_insert<
        'db,
        DB: NetabaseStore<NoRepository>,
        P: crate::traits::structural::database::tables::InsertPolicy,
    >(
        &self,
        _txn: &mut impl RepositoryWriteTx<'db, NoRepository, DB>,
        _item: NoRepository,
        _config: &crate::traits::structural::database::tables::InsertConfig<'_, (), P>,
    ) -> Result<(), NetabaseError>
    where
        NoRepository: 'db,
    {
        Ok(())
    }

    fn orchestrate_get<'db, DB: NetabaseStore<NoRepository>>(
        &self,
        _txn: &impl RepositoryReadTx<'db, NoRepository, DB>,
        _key: (),
    ) -> Result<Option<NoRepository>, NetabaseError>
    where
        NoRepository: 'db,
    {
        Ok(None)
    }

    fn orchestrate_delete<'db, DB: NetabaseStore<NoRepository>>(
        &self,
        _address: (),
        _key: (),
        _txn: &mut impl RepositoryWriteTx<'db, NoRepository, DB>,
    ) -> Result<(), NetabaseError>
    where
        NoRepository: 'db,
    {
        Ok(())
    }
}

impl NetabaseRepository for NoRepository {
    type Address = ();
    type Keys = ();
    type Tables = ();
    const TABLES: () = ();
}
