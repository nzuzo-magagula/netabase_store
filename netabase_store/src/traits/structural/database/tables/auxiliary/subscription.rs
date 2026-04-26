// @review [ ]
use super::TableSideEffect;
use crate::errors::NetabaseError;
use crate::traits::structural::database::NetabaseStore;
use crate::traits::structural::database::tables::core::{ModelHash, NodeStorageMode};
use crate::traits::structural::database::transactions::repository::RepositoryWriteTx;
use crate::traits::structural::schema::models::keys::NetabaseModelKeys;
use crate::traits::structural::schema::{
    definitions::NetabaseDefinition, models::NetabaseModelWithKeys,
    repositories::NetabaseRepository,
};

pub trait SubscriptionTables<
    R: NetabaseRepository,
    D: NetabaseDefinition<R>,
    M: NetabaseModelWithKeys<R, D>,
>: TableSideEffect<R, D, M>
{
    // TODO(#subscription/id-2f1): V[F(orchestrate_insert)], "Define routing for non-model subscriptions and how they share keys with sibling enums."
    /// Write `hash` for `model` into the subscription topic tables named by `keys`
    /// (the model-owned subscription topics this insert opted into), plus any
    /// compile-time parent subscriptions the model declared via `subscribe(...)`.
    fn orchestrate_insert<'db, DB: NetabaseStore<R>>(
        &mut self,
        txn: &mut impl RepositoryWriteTx<'db, R, DB>,
        model: &M,
        hash: ModelHash,
        keys: &[<M::Keys as NetabaseModelKeys<R, D, M>>::SubscriptionKeys],
    ) -> Result<(), NetabaseError>;

    fn orchestrate_delete<'db, DB: NetabaseStore<R>>(
        &mut self,
        txn: &mut impl RepositoryWriteTx<'db, R, DB>,
        key: &<M::Keys as NetabaseModelKeys<R, D, M>>::PrimaryKey,
    ) -> Result<(), NetabaseError>;
}

impl<R: NetabaseRepository, D: NetabaseDefinition<R>, M: NetabaseModelWithKeys<R, D>>
    SubscriptionTables<R, D, M> for ()
{
    fn orchestrate_insert<'db, DB: NetabaseStore<R>>(
        &mut self,
        _txn: &mut impl RepositoryWriteTx<'db, R, DB>,
        _model: &M,
        _hash: ModelHash,
        _keys: &[<M::Keys as NetabaseModelKeys<R, D, M>>::SubscriptionKeys],
    ) -> Result<(), NetabaseError> {
        Ok(())
    }

    fn orchestrate_delete<'db, DB: NetabaseStore<R>>(
        &mut self,
        _txn: &mut impl RepositoryWriteTx<'db, R, DB>,
        _key: &<M::Keys as NetabaseModelKeys<R, D, M>>::PrimaryKey,
    ) -> Result<(), NetabaseError> {
        Ok(())
    }
}

pub trait SubscriptionStorageMode<
    R: NetabaseRepository,
    D: NetabaseDefinition<R>,
    M: NetabaseModelWithKeys<R, D>,
>: NodeStorageMode
{
}
