// @review [ ]
use crate::traits::structural::contract::Scope;
use crate::traits::structural::contract::contract_tables::TablesStruct;
use crate::traits::structural::contract::contract_values::contract_relational_value::Relational;
use crate::traits::structural::repository::Repository;
use crate::traits::structural::repository::repo_values::RepositoryTableValue;

pub trait RepositoryRelationalValue<R: Repository>:
    RepositoryTableValue<<<R as Scope>::Tables as TablesStruct>::Relational> + Relational
{
}
