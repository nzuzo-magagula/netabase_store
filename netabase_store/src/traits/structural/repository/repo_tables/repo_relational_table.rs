// @review [~]
use crate::traits::structural::contract::contract_tables::TableStruct;
use crate::traits::structural::contract::contract_tables::contract_relational_table::RelationalTableStruct;
use crate::traits::structural::repository::{
    Repository, repo_keys::repo_relational_key::RepositoryRelationalKey, repo_tables::RepositoryTable,
    repo_values::repo_relational_value::RepositoryRelationalValue,
};

pub trait RepositoryRelationalTable<R: Repository>:
    RepositoryTable
    + RelationalTableStruct
    + TableStruct<Key: RepositoryRelationalKey<R>, Value: RepositoryRelationalValue<R>>
{
}
