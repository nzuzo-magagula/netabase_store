// @review [~]
use crate::traits::structural::contract::contract_tables::TableStruct;
use crate::traits::structural::contract::contract_values::Value;

pub mod def_blob_value;
pub mod def_primary_value;
pub mod def_relational_value;
pub mod def_secondary_value;
pub mod def_subscription_value;

pub trait DefinitionTableValue<T: TableStruct>: Value {}
