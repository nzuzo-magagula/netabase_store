// @review [~]
use crate::traits::structural::definition::values::primary::DefinitionPrimaryValue;
use crate::traits::structural::{
    definition::{Definition, keys::primary::DefinitionPrimaryKey, tables::DefinitionTable},
    repository::Repository,
};

pub trait DefinitionPrimaryTable<R: Repository, D: Definition<R>>: DefinitionTable
where
    <Self as DefinitionTable>::Key: DefinitionPrimaryKey<R, D>,
    <Self as DefinitionTable>::Value: DefinitionPrimaryValue<R, D>,
{
}
