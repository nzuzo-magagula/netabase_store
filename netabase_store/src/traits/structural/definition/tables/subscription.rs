// @review [~]
use crate::traits::structural::definition::values::subscription::DefinitionSubscriptionValue;
use crate::traits::structural::{
    definition::{
        Definition, keys::subscription::DefinitionSubscriptionKey, tables::DefinitionTable,
    },
    repository::Repository,
};

pub trait DefinitionSubscriptionTable<R: Repository, D: Definition<R>>: DefinitionTable
where
    <Self as DefinitionTable>::Key: DefinitionSubscriptionKey<R, D>,
    <Self as DefinitionTable>::Value: DefinitionSubscriptionValue<R, D>,
{
}
