// @review [~]
use crate::traits::structural::contract::contract_values::Value;
use crate::traits::structural::contract::contract_tables::TableStruct;

pub mod model_blob_value;
pub mod model_primary_value;
pub mod model_relational_value;
pub mod model_secondary_value;
pub mod model_subscription_value;

pub trait TableValue<T: TableStruct>: Value {}
