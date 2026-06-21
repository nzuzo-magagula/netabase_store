// @review [~]
use crate::traits::structural::contract::contract_tables::{TableStruct, TablesStruct};
use crate::traits::structural::repository::Repository;
use crate::traits::structural::repository::repo_keys::RepositoryTableKey;
use crate::traits::structural::repository::repo_values::RepositoryTableValue;

pub mod repo_blob_table;
pub mod repo_primary_table;
pub mod repo_relational_table;
pub mod repo_secondary_table;
pub mod repo_subscription_table;

pub trait RepositoryTable:
    TableStruct<Key: RepositoryTableKey<Self>, Value: RepositoryTableValue<Self>>
{
}

pub trait RepositoryTables<R: Repository>: TablesStruct {}
