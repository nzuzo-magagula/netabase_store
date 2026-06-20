// @review [~]
use crate::traits::structural::model::Definition;
use crate::traits::structural::model::keys::blob::BlobKey;
use crate::traits::structural::model::keys::primary::PrimaryKey;
use crate::traits::structural::model::keys::relational::RelationalKey;
use crate::traits::structural::model::keys::secondary::SecondaryKey;
use crate::traits::structural::model::keys::subscription::SubscriptionKey;
use crate::traits::structural::model::tables::blob::BlobTable;
use crate::traits::structural::model::tables::primary::PrimaryTable;
use crate::traits::structural::model::tables::relational::RelationalTable;
use crate::traits::structural::model::tables::secondary::SecondaryTable;
use crate::traits::structural::model::tables::subscription::SubscriptionTable;
use crate::traits::structural::model::values::blob::BlobValue;
use crate::traits::structural::model::values::primary::PrimaryValue;
use crate::traits::structural::model::values::relational::RelationalValue;
use crate::traits::structural::model::values::secondary::SecondaryValue;
use crate::traits::structural::model::values::subscription::SubscriptionValue;
use crate::traits::structural::model::{Model, keys::TableKey, values::TableValue};
use crate::traits::structural::repository::Repository;

pub mod blob;
pub mod primary;
pub mod relational;
pub mod secondary;
pub mod subscription;

pub trait ModelTable: std::marker::Sized {
    type Key: TableKey<Self>;
    type Value: TableValue<Self>;
}

pub trait ModelTables<R: Repository, D: Definition<R>, M: Model<R, D>>
where
    <<Self as ModelTables<R, D, M>>::Primary as ModelTable>::Key: PrimaryKey<R, D, M>,
    <<Self as ModelTables<R, D, M>>::Secondary as ModelTable>::Key: SecondaryKey<R, D, M>,
    <<Self as ModelTables<R, D, M>>::Relational as ModelTable>::Key: RelationalKey<R, D, M>,
    <<Self as ModelTables<R, D, M>>::Subscription as ModelTable>::Key: SubscriptionKey<R, D, M>,
    <<Self as ModelTables<R, D, M>>::Blob as ModelTable>::Key: BlobKey<R, D, M>,
    <<Self as ModelTables<R, D, M>>::Primary as ModelTable>::Value: PrimaryValue<R, D, M>,
    <<Self as ModelTables<R, D, M>>::Secondary as ModelTable>::Value: SecondaryValue<R, D, M>,
    <<Self as ModelTables<R, D, M>>::Relational as ModelTable>::Value: RelationalValue<R, D, M>,
    <<Self as ModelTables<R, D, M>>::Subscription as ModelTable>::Value: SubscriptionValue<R, D, M>,
    <<Self as ModelTables<R, D, M>>::Blob as ModelTable>::Value: BlobValue<R, D, M>,
{
    type Primary: PrimaryTable<R, D, M>;
    type Secondary: SecondaryTable<R, D, M>;
    type Relational: RelationalTable<R, D, M>;
    type Subscription: SubscriptionTable<R, D, M>;
    type Blob: BlobTable<R, D, M>;
}
