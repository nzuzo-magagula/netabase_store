// @review [ ]
use crate::errors::NetabaseError;
use crate::traits::structural::{
    database::NetabaseStore,
    schema::{
        definitions::NetabaseDefinition,
        models::{NetabaseModel, NetabaseModelWithKeys, keys::NetabaseModelKeys},
        repositories::NetabaseRepository,
    },
};

use super::NetabaseTransaction;

pub trait ModelTransaction<
    'db,
    R: NetabaseRepository,
    D: NetabaseDefinition<R>,
    M: NetabaseModel<R, D>,
    DB: NetabaseStore<R>,
>: NetabaseTransaction<'db, R, DB> where
    R: 'db,
{
}

pub trait ModelReadTx<
    'db,
    R: NetabaseRepository,
    D: NetabaseDefinition<R>,
    M: NetabaseModel<R, D>,
    DB: NetabaseStore<R>,
>: ModelTransaction<'db, R, D, M, DB> + ModelReadOps<'db, R, D, M, DB> where
    R: 'db,
{
}

pub trait ModelWriteTx<
    'db,
    R: NetabaseRepository,
    D: NetabaseDefinition<R>,
    M: NetabaseModel<R, D>,
    DB: NetabaseStore<R>,
>: ModelTransaction<'db, R, D, M, DB> + ModelWriteOps<'db, R, D, M, DB> where
    R: 'db,
{
}

pub trait ModelReadOps<'db, R, D, M, DB>
where
    R: NetabaseRepository + 'db,
    D: NetabaseDefinition<R>,
    M: NetabaseModelWithKeys<R, D>,
    DB: NetabaseStore<R>,
{
    fn get(
        &self,
        key: <M::Keys as NetabaseModelKeys<R, D, M>>::PrimaryKey,
    ) -> Result<Option<M>, NetabaseError>;
}

pub trait ModelWriteOps<'db, R, D, M, DB>: ModelReadOps<'db, R, D, M, DB>
where
    R: NetabaseRepository + 'db,
    D: NetabaseDefinition<R>,
    M: NetabaseModelWithKeys<R, D>,
    DB: NetabaseStore<R>,
{
    fn insert(&mut self, model: M) -> Result<(), NetabaseError>;
    fn delete(
        &mut self,
        key: <M::Keys as NetabaseModelKeys<R, D, M>>::PrimaryKey,
    ) -> Result<(), NetabaseError>;

    // TODO(#model_write/id-w001): Q[F(route_insert)], "route_insert is a default impl that just calls self.insert(model). It is identical to insert. At the definition level (definition.rs), route_insert is a required method on the trait with meaningful dispatch logic (match on enum variants). Here it is a no-op alias. Determine whether model-level routing should also dispatch (e.g. across secondary/relational/subscription tables) or if this default is intentionally a pass-through and should be removed."
    fn route_insert(&mut self, model: M) -> Result<(), NetabaseError> {
        self.insert(model)
    }

    fn route_delete(
        &mut self,
        key: <M::Keys as NetabaseModelKeys<R, D, M>>::PrimaryKey,
    ) -> Result<(), NetabaseError> {
        self.delete(key)
    }
}
