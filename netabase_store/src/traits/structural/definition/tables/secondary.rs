// @review [~]
use crate::traits::structural::definition::values::secondary::DefinitionSecondaryValue;
use crate::traits::structural::{
    definition::{Definition, keys::secondary::DefinitionSecondaryKey, tables::DefinitionTable},
    repository::Repository,
};

pub trait DefinitionSecondaryTable<R: Repository, D: Definition<R>>: DefinitionTable
where
    <Self as DefinitionTable>::Key: DefinitionSecondaryKey<R, D>,
    <Self as DefinitionTable>::Value: DefinitionSecondaryValue<R, D>,
{
}
