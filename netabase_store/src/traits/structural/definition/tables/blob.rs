// @review [~]
use crate::traits::structural::definition::values::blob::DefinitionBlobValue;
use crate::traits::structural::{
    definition::{Definition, keys::blob::DefinitionBlobKey, tables::DefinitionTable},
    repository::Repository,
};

pub trait DefinitionBlobTable<R: Repository, D: Definition<R>>: DefinitionTable
where
    <Self as DefinitionTable>::Key: DefinitionBlobKey<R, D>,
    <Self as DefinitionTable>::Value: DefinitionBlobValue<R, D>,
{
}
