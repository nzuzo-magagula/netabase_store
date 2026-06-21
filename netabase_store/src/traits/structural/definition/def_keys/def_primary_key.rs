// @review [ ]
use crate::traits::structural::contract::Scope;
use crate::traits::structural::contract::contract_keys::contract_primary_key::PrimaryKey;
use crate::traits::structural::contract::contract_tables::TablesStruct;
use crate::traits::structural::definition::Definition;
use crate::traits::structural::definition::def_keys::DefinitionTableKey;
use crate::traits::structural::repository::Repository;

pub trait DefinitionPrimaryKey<R: Repository, D: Definition<R>>:
    DefinitionTableKey<<<D as Scope>::Tables as TablesStruct>::Primary> + PrimaryKey
{
}
