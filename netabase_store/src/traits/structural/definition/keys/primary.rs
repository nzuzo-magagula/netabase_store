// @review [ ]
use crate::traits::structural::definition::keys::DefinitionTableKey;
use crate::traits::structural::definition::tables::DefinitionTables;
use crate::traits::structural::{definition::Definition, repository::Repository};

pub trait DefinitionPrimaryKey<R: Repository, D: Definition<R>>:
    DefinitionTableKey<<<D as Definition<R>>::Tables as DefinitionTables<R, D>>::Primary>
{
}
