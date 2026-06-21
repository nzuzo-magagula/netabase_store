// @review [~]
use crate::traits::structural::Addressable;
use crate::traits::structural::definition::Definition;
use crate::traits::structural::definition::keys::blob::DefinitionBlobKey;
use crate::traits::structural::definition::keys::primary::DefinitionPrimaryKey;
use crate::traits::structural::definition::keys::relational::DefinitionRelationalKey;
use crate::traits::structural::definition::keys::secondary::DefinitionSecondaryKey;
use crate::traits::structural::definition::keys::subscription::DefinitionSubscriptionKey;
use crate::traits::structural::definition::tables::blob::DefinitionBlobTable;
use crate::traits::structural::definition::tables::primary::DefinitionPrimaryTable;
use crate::traits::structural::definition::tables::relational::DefinitionRelationalTable;
use crate::traits::structural::definition::tables::secondary::DefinitionSecondaryTable;
use crate::traits::structural::definition::tables::subscription::DefinitionSubscriptionTable;
use crate::traits::structural::definition::values::blob::DefinitionBlobValue;
use crate::traits::structural::definition::values::primary::DefinitionPrimaryValue;
use crate::traits::structural::definition::values::relational::DefinitionRelationalValue;
use crate::traits::structural::definition::values::secondary::DefinitionSecondaryValue;
use crate::traits::structural::definition::values::subscription::DefinitionSubscriptionValue;
use crate::traits::structural::definition::{keys::DefinitionTableKey, values::DefinitionTableValue};
use crate::traits::structural::repository::Repository;

pub mod blob;
pub mod primary;
pub mod relational;
pub mod secondary;
pub mod subscription;

pub trait DefinitionTable: std::marker::Sized + Addressable {
    type Key: DefinitionTableKey<Self>;
    type Value: DefinitionTableValue<Self>;
}

pub trait DefinitionTables<R: Repository, D: Definition<R>>: Addressable
where
    <<Self as DefinitionTables<R, D>>::Primary as DefinitionTable>::Key: DefinitionPrimaryKey<R, D>,
    <<Self as DefinitionTables<R, D>>::Secondary as DefinitionTable>::Key:
        DefinitionSecondaryKey<R, D>,
    <<Self as DefinitionTables<R, D>>::Relational as DefinitionTable>::Key:
        DefinitionRelationalKey<R, D>,
    <<Self as DefinitionTables<R, D>>::Subscription as DefinitionTable>::Key:
        DefinitionSubscriptionKey<R, D>,
    <<Self as DefinitionTables<R, D>>::Blob as DefinitionTable>::Key: DefinitionBlobKey<R, D>,
    <<Self as DefinitionTables<R, D>>::Primary as DefinitionTable>::Value:
        DefinitionPrimaryValue<R, D>,
    <<Self as DefinitionTables<R, D>>::Secondary as DefinitionTable>::Value:
        DefinitionSecondaryValue<R, D>,
    <<Self as DefinitionTables<R, D>>::Relational as DefinitionTable>::Value:
        DefinitionRelationalValue<R, D>,
    <<Self as DefinitionTables<R, D>>::Subscription as DefinitionTable>::Value:
        DefinitionSubscriptionValue<R, D>,
    <<Self as DefinitionTables<R, D>>::Blob as DefinitionTable>::Value: DefinitionBlobValue<R, D>,
{
    type Primary: DefinitionPrimaryTable<R, D>;
    type Secondary: DefinitionSecondaryTable<R, D>;
    type Relational: DefinitionRelationalTable<R, D>;
    type Subscription: DefinitionSubscriptionTable<R, D>;
    type Blob: DefinitionBlobTable<R, D>;
}
