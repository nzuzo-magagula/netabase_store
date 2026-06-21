// @review [ ]
use crate::traits::structural::contract::Scope;
use crate::traits::structural::contract::contract_keys::contract_secondary_key::SecondaryKey;
use crate::traits::structural::contract::contract_tables::TablesStruct;
use crate::traits::structural::definition::Definition;
use crate::traits::structural::definition::def_keys::DefinitionTableKey;
use crate::traits::structural::repository::Repository;

pub trait DefinitionSecondaryKey<R: Repository, D: Definition<R>>:
    DefinitionTableKey<<<D as Scope>::Tables as TablesStruct>::Secondary> + SecondaryKey
{
}
