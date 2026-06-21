// @review [~]
use crate::traits::structural::contract::contract_tables::TableStruct;
use crate::traits::structural::contract::contract_tables::contract_primary_table::PrimaryTableStruct;
use crate::traits::structural::repository::{
    Repository, repo_keys::repo_primary_key::RepositoryPrimaryKey, repo_tables::RepositoryTable,
    repo_values::repo_primary_value::RepositoryPrimaryValue,
};

pub trait RepositoryPrimaryTable<R: Repository>:
    RepositoryTable
    + PrimaryTableStruct
    + TableStruct<Key: RepositoryPrimaryKey<R>, Value: RepositoryPrimaryValue<R>>
{
}
