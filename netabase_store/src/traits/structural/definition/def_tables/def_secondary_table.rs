// @review [~]
use crate::traits::structural::contract::contract_tables::TableStruct;
use crate::traits::structural::contract::contract_tables::contract_secondary_table::SecondaryTableStruct;
use crate::traits::structural::definition::{
    Definition, def_keys::def_secondary_key::DefinitionSecondaryKey, def_tables::DefinitionTable,
    def_values::def_secondary_value::DefinitionSecondaryValue,
};
use crate::traits::structural::repository::Repository;

pub trait DefinitionSecondaryTable<R: Repository, D: Definition<R>>:
    DefinitionTable
    + SecondaryTableStruct
    + TableStruct<Key: DefinitionSecondaryKey<R, D>, Value: DefinitionSecondaryValue<R, D>>
{
}
