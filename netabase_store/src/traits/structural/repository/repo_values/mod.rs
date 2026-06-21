// @review [~]
use crate::traits::structural::contract::contract_tables::TableStruct;
use crate::traits::structural::contract::contract_values::Value;

pub mod repo_blob_value;
pub mod repo_primary_value;
pub mod repo_relational_value;
pub mod repo_secondary_value;
pub mod repo_subscription_value;

pub trait RepositoryTableValue<T: TableStruct>: Value {}
