// @review [ ]
use crate::traits::structural::definition::tables::DefinitionTables;
use crate::traits::structural::definition::values::DefinitionTableValue;
use crate::traits::structural::{definition::Definition, repository::Repository};

pub trait DefinitionSecondaryValue<R: Repository, D: Definition<R>>:
    DefinitionTableValue<<<D as Definition<R>>::Tables as DefinitionTables<R, D>>::Secondary>
{
}
