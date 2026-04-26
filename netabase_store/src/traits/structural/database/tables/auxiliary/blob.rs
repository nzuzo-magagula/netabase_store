// @review [ ]
use super::TableSideEffect;
use crate::errors::NetabaseError;
use crate::traits::structural::database::NetabaseStore;
use crate::traits::structural::database::tables::core::NodeStorageMode;
use crate::traits::structural::database::transactions::repository::RepositoryWriteTx;
use crate::traits::structural::schema::models::keys::NetabaseModelKeys;
use crate::traits::structural::schema::{
    definitions::NetabaseDefinition, models::NetabaseModelWithKeys,
    repositories::NetabaseRepository,
};

pub trait BlobTables<
    R: NetabaseRepository,
    D: NetabaseDefinition<R>,
    M: NetabaseModelWithKeys<R, D>,
>: TableSideEffect<R, D, M>
{
    fn orchestrate_insert<'db, DB: NetabaseStore<R>>(
        &mut self,
        txn: &mut impl RepositoryWriteTx<'db, R, DB>,
        model: &M,
    ) -> Result<(), NetabaseError>;

    fn orchestrate_delete<'db, DB: NetabaseStore<R>>(
        &mut self,
        txn: &mut impl RepositoryWriteTx<'db, R, DB>,
        key: &<M::Keys as NetabaseModelKeys<R, D, M>>::PrimaryKey,
    ) -> Result<(), NetabaseError>;
}

impl<R: NetabaseRepository, D: NetabaseDefinition<R>, M: NetabaseModelWithKeys<R, D>>
    BlobTables<R, D, M> for ()
{
    fn orchestrate_insert<'db, DB: NetabaseStore<R>>(
        &mut self,
        _txn: &mut impl RepositoryWriteTx<'db, R, DB>,
        _model: &M,
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

pub trait BlobStorageMode<
    R: NetabaseRepository,
    D: NetabaseDefinition<R>,
    M: NetabaseModelWithKeys<R, D>,
>: NodeStorageMode
{
}
