// @review [~]
use crate::traits::structural::contract::contract_tables::TableStruct;
use crate::traits::structural::contract::contract_tables::contract_relational_table::RelationalTableStruct;
use crate::traits::structural::definition::{
    Definition, def_keys::def_relational_key::DefinitionRelationalKey, def_tables::DefinitionTable,
    def_values::def_relational_value::DefinitionRelationalValue,
};
use crate::traits::structural::repository::Repository;

pub trait DefinitionRelationalTable<R: Repository, D: Definition<R>>:
    DefinitionTable
    + RelationalTableStruct
    + TableStruct<Key: DefinitionRelationalKey<R, D>, Value: DefinitionRelationalValue<R, D>>
{
}
