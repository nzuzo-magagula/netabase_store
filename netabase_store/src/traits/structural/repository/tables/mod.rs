// @review [~]
use crate::traits::structural::Addressable;
use crate::traits::structural::repository::Repository;
use crate::traits::structural::repository::keys::blob::RepositoryBlobKey;
use crate::traits::structural::repository::keys::primary::RepositoryPrimaryKey;
use crate::traits::structural::repository::keys::relational::RepositoryRelationalKey;
use crate::traits::structural::repository::keys::secondary::RepositorySecondaryKey;
use crate::traits::structural::repository::keys::subscription::RepositorySubscriptionKey;
use crate::traits::structural::repository::tables::blob::RepositoryBlobTable;
use crate::traits::structural::repository::tables::primary::RepositoryPrimaryTable;
use crate::traits::structural::repository::tables::relational::RepositoryRelationalTable;
use crate::traits::structural::repository::tables::secondary::RepositorySecondaryTable;
use crate::traits::structural::repository::tables::subscription::RepositorySubscriptionTable;
use crate::traits::structural::repository::values::blob::RepositoryBlobValue;
use crate::traits::structural::repository::values::primary::RepositoryPrimaryValue;
use crate::traits::structural::repository::values::relational::RepositoryRelationalValue;
use crate::traits::structural::repository::values::secondary::RepositorySecondaryValue;
use crate::traits::structural::repository::values::subscription::RepositorySubscriptionValue;
use crate::traits::structural::repository::{keys::RepositoryTableKey, values::RepositoryTableValue};

pub mod blob;
pub mod primary;
pub mod relational;
pub mod secondary;
pub mod subscription;

pub trait RepositoryTable: std::marker::Sized + Addressable {
    type Key: RepositoryTableKey<Self>;
    type Value: RepositoryTableValue<Self>;
}

pub trait RepositoryTables<R: Repository>: Addressable
where
    <<Self as RepositoryTables<R>>::Primary as RepositoryTable>::Key: RepositoryPrimaryKey<R>,
    <<Self as RepositoryTables<R>>::Secondary as RepositoryTable>::Key: RepositorySecondaryKey<R>,
    <<Self as RepositoryTables<R>>::Relational as RepositoryTable>::Key: RepositoryRelationalKey<R>,
    <<Self as RepositoryTables<R>>::Subscription as RepositoryTable>::Key:
        RepositorySubscriptionKey<R>,
    <<Self as RepositoryTables<R>>::Blob as RepositoryTable>::Key: RepositoryBlobKey<R>,
    <<Self as RepositoryTables<R>>::Primary as RepositoryTable>::Value: RepositoryPrimaryValue<R>,
    <<Self as RepositoryTables<R>>::Secondary as RepositoryTable>::Value:
        RepositorySecondaryValue<R>,
    <<Self as RepositoryTables<R>>::Relational as RepositoryTable>::Value:
        RepositoryRelationalValue<R>,
    <<Self as RepositoryTables<R>>::Subscription as RepositoryTable>::Value:
        RepositorySubscriptionValue<R>,
    <<Self as RepositoryTables<R>>::Blob as RepositoryTable>::Value: RepositoryBlobValue<R>,
{
    type Primary: RepositoryPrimaryTable<R>;
    type Secondary: RepositorySecondaryTable<R>;
    type Relational: RepositoryRelationalTable<R>;
    type Subscription: RepositorySubscriptionTable<R>;
    type Blob: RepositoryBlobTable<R>;
}
