// @review [~]
use crate::traits::structural::definition::values::relational::DefinitionRelationalValue;
use crate::traits::structural::{
    definition::{Definition, keys::relational::DefinitionRelationalKey, tables::DefinitionTable},
    repository::Repository,
};

pub trait DefinitionRelationalTable<R: Repository, D: Definition<R>>: DefinitionTable
where
    <Self as DefinitionTable>::Key: DefinitionRelationalKey<R, D>,
    <Self as DefinitionTable>::Value: DefinitionRelationalValue<R, D>,
{
}
