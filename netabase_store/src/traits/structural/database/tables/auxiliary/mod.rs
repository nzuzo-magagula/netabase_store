// @review [ ]
pub mod blob;
pub mod custom;
pub mod relational;
pub mod secondary;
pub mod subscription;

use crate::traits::structural::database::tables::core::{NodeStorageMode, TableStorageMode};
use crate::traits::structural::schema::models::keys::NetabaseModelKeys;
use crate::traits::structural::schema::{
    definitions::NetabaseDefinition, models::NetabaseModelWithKeys,
    repositories::NetabaseRepository,
};

pub trait TableSideEffect<
    R: NetabaseRepository,
    D: NetabaseDefinition<R>,
    M: NetabaseModelWithKeys<R, D>,
>
{
    fn on_insert(&mut self, model: &M) -> Result<(), crate::errors::NetabaseError>;
    fn on_delete(
        &mut self,
        key: &<M::Keys as NetabaseModelKeys<R, D, M>>::PrimaryKey,
    ) -> Result<(), crate::errors::NetabaseError>;
}

impl<R: NetabaseRepository, D: NetabaseDefinition<R>, M: NetabaseModelWithKeys<R, D>>
    TableSideEffect<R, D, M> for ()
{
    fn on_insert(&mut self, _model: &M) -> Result<(), crate::errors::NetabaseError> {
        Ok(())
    }
    fn on_delete(
        &mut self,
        _key: &<M::Keys as NetabaseModelKeys<R, D, M>>::PrimaryKey,
    ) -> Result<(), crate::errors::NetabaseError> {
        Ok(())
    }
}

pub trait AuxiliaryTables<
    R: NetabaseRepository,
    D: NetabaseDefinition<R>,
    M: NetabaseModelWithKeys<R, D>,
>: TableSideEffect<R, D, M>
{
    type Secondary: secondary::SecondaryTables<R, D, M>;
    type Relational: relational::RelationalTables<R, D, M>;
    type Subscription: subscription::SubscriptionTables<R, D, M>;
    type Blob: blob::BlobTables<R, D, M>;
    type Custom: custom::CustomTables<R, D, M>;
}

impl<R: NetabaseRepository, D: NetabaseDefinition<R>, M: NetabaseModelWithKeys<R, D>>
    AuxiliaryTables<R, D, M> for ()
{
    type Secondary = ();
    type Relational = ();
    type Subscription = ();
    type Blob = ();
    type Custom = ();
}

pub trait AuxiliaryStorageMode<
    R: NetabaseRepository,
    D: NetabaseDefinition<R>,
    M: NetabaseModelWithKeys<R, D>,
>: NodeStorageMode
{
    const SECONDARY_MODE: TableStorageMode = TableStorageMode::Sharded;
    const RELATIONAL_MODE: TableStorageMode = TableStorageMode::Sharded;
    const SUBSCRIPTION_MODE: TableStorageMode = TableStorageMode::Sharded;
    const BLOB_MODE: TableStorageMode = TableStorageMode::Sharded;
    const CUSTOM_MODE: TableStorageMode = TableStorageMode::Sharded;

    /// Pure-shard dedup flags: when true, the corresponding field category is omitted from the
    /// Primary record (the aux table is the source of truth) and rehydrated on read. Blob is always
    /// rehydrated on `get`; relational rehydration is emitted only when `RELATIONAL_DEDUP` is set.
    const BLOB_DEDUP: bool = false;
    const RELATIONAL_DEDUP: bool = false;
}
