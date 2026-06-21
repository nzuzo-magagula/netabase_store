// @review [ ]
use crate::traits::structural::contract::Scope;
use crate::traits::structural::contract::contract_tables::TablesStruct;
use crate::traits::structural::contract::contract_values::contract_primary_value::Primary;
use crate::traits::structural::repository::Repository;
use crate::traits::structural::repository::repo_values::RepositoryTableValue;

pub trait RepositoryPrimaryValue<R: Repository>:
    RepositoryTableValue<<<R as Scope>::Tables as TablesStruct>::Primary> + Primary
{
}
