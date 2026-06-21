// @review [~]
use crate::traits::structural::contract::contract_keys::Key;
use crate::traits::structural::contract::contract_tables::TableStruct;

pub mod repo_blob_key;
pub mod repo_primary_key;
pub mod repo_relational_key;
pub mod repo_secondary_key;
pub mod repo_subscription_key;

pub trait RepositoryTableKey<T: TableStruct>: Key {}
