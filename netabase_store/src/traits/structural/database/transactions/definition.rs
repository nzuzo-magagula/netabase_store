// @review [x]
use crate::errors::NetabaseError;
use crate::traits::structural::{
    database::NetabaseStore,
    database::transactions::repository::{RepositoryReadTx, RepositoryWriteTx},
    schema::{
        definitions::NetabaseDefinition, models::keys::NetabaseDefinitionKeys,
        repositories::NetabaseRepository,
    },
};

use super::NetabaseTransaction;

pub trait DefinitionTransaction<
    'db,
    R: NetabaseRepository,
    D: NetabaseDefinition<R>,
    DB: NetabaseStore<R>,
>: NetabaseTransaction<'db, R, DB> where
    R: 'db,
{
}

pub trait DefinitionReadTx<
    'db,
    R: NetabaseRepository,
    D: NetabaseDefinition<R>,
    DB: NetabaseStore<R>,
>:
    DefinitionTransaction<'db, R, D, DB>
    + DefinitionReadOps<'db, R, D, DB>
    + RepositoryReadTx<'db, R, DB> where
    R: 'db,
{
}

pub trait DefinitionWriteTx<
    'db,
    R: NetabaseRepository,
    D: NetabaseDefinition<R>,
    DB: NetabaseStore<R>,
>:
    DefinitionTransaction<'db, R, D, DB>
    + DefinitionWriteOps<'db, R, D, DB>
    + RepositoryWriteTx<'db, R, DB> where
    R: 'db,
{
}

pub trait DefinitionReadOps<'db, R, D, DB>
where
    R: NetabaseRepository + 'db,
    D: NetabaseDefinition<R>,
    DB: NetabaseStore<R>,
{
    fn get(
        &self,
        address: D::Address,
        key: <D::Keys as NetabaseDefinitionKeys<R, D>>::PrimaryKey,
    ) -> Result<Option<D>, NetabaseError>;
}

pub trait DefinitionWriteOps<'db, R, D, DB>
where
    R: NetabaseRepository + 'db,
    D: NetabaseDefinition<R>,
    DB: NetabaseStore<R>,
{
    fn insert(&mut self, item: D) -> Result<(), NetabaseError>;
    fn delete(
        &mut self,
        address: D::Address,
        key: <D::Keys as NetabaseDefinitionKeys<R, D>>::PrimaryKey,
    ) -> Result<(), NetabaseError>;
}
