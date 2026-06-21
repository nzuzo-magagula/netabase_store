// @review [~]
use crate::traits::structural::contract::contract_tables::{TableStruct, TablesStruct};
use crate::traits::structural::definition::Definition;
use crate::traits::structural::definition::def_keys::DefinitionTableKey;
use crate::traits::structural::definition::def_values::DefinitionTableValue;
use crate::traits::structural::repository::Repository;

pub mod def_blob_table;
pub mod def_primary_table;
pub mod def_relational_table;
pub mod def_secondary_table;
pub mod def_subscription_table;

pub trait DefinitionTable:
    TableStruct<Key: DefinitionTableKey<Self>, Value: DefinitionTableValue<Self>>
{
}

pub trait DefinitionTables<R: Repository, D: Definition<R>>: TablesStruct {}
