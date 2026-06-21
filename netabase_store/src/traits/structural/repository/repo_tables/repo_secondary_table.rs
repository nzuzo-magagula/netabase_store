// @review [~]
use crate::traits::structural::contract::contract_tables::TableStruct;
use crate::traits::structural::contract::contract_tables::contract_secondary_table::SecondaryTableStruct;
use crate::traits::structural::repository::{
    Repository, repo_keys::repo_secondary_key::RepositorySecondaryKey, repo_tables::RepositoryTable,
    repo_values::repo_secondary_value::RepositorySecondaryValue,
};

pub trait RepositorySecondaryTable<R: Repository>:
    RepositoryTable
    + SecondaryTableStruct
    + TableStruct<Key: RepositorySecondaryKey<R>, Value: RepositorySecondaryValue<R>>
{
}
