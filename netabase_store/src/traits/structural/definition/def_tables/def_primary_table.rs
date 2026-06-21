// @review [~]
use crate::traits::structural::contract::contract_tables::TableStruct;
use crate::traits::structural::contract::contract_tables::contract_primary_table::PrimaryTableStruct;
use crate::traits::structural::definition::{
    Definition, def_keys::def_primary_key::DefinitionPrimaryKey, def_tables::DefinitionTable,
    def_values::def_primary_value::DefinitionPrimaryValue,
};
use crate::traits::structural::repository::Repository;

pub trait DefinitionPrimaryTable<R: Repository, D: Definition<R>>:
    DefinitionTable
    + PrimaryTableStruct
    + TableStruct<Key: DefinitionPrimaryKey<R, D>, Value: DefinitionPrimaryValue<R, D>>
{
}
