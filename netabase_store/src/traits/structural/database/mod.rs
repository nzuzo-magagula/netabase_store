use crate::traits::structural::database::transactions::{RepositoryReadTx, RepositoryWriteTx};
use crate::{errors::NetabaseError, traits::structural::schema::repositories::NetabaseRepository};

pub mod tables;
pub mod transactions;

pub trait NetabaseStore<R: NetabaseRepository>: Sized {
    /// What this backend opens: a filesystem path for redb/fjall, a borrowed
    /// manifest byte region (`&mut [u8]`) for the arena store.
    type Resource;

    fn open(resource: Self::Resource) -> Result<Self, NetabaseError>;

    type NetabaseStoreReadTransaction<'db>: RepositoryReadTx<'db, R, Self>
    where
        Self: 'db,
        R: 'db;

    type NetabaseStoreWriteTransaction<'db>: RepositoryWriteTx<'db, R, Self>
    where
        Self: 'db,
        R: 'db;

    fn read_transaction<'a>(
        &'a self,
    ) -> Result<Self::NetabaseStoreReadTransaction<'a>, NetabaseError>
    where
        R: 'a;

    /// Begin a write transaction. Takes `&mut self`: the borrow checker
    /// itself proves there is exactly one writer — single-writer semantics
    /// are a compile-time property, not a runtime lock.
    fn write_transaction<'a>(
        &'a mut self,
    ) -> Result<Self::NetabaseStoreWriteTransaction<'a>, NetabaseError>
    where
        R: 'a;
}
