// @review [ ]
use crate::traits::structural::contract::Scope;
use crate::traits::structural::contract::contract_tables::TablesStruct;
use crate::traits::structural::contract::contract_values::contract_primary_value::Primary;
use crate::traits::structural::definition::Definition;
use crate::traits::structural::definition::def_values::DefinitionTableValue;
use crate::traits::structural::repository::Repository;

pub trait DefinitionPrimaryValue<R: Repository, D: Definition<R>>:
    DefinitionTableValue<<<D as Scope>::Tables as TablesStruct>::Primary> + Primary
{
}
