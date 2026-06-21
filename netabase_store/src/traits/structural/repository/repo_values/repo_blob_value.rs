// @review [ ]
use crate::traits::structural::contract::Scope;
use crate::traits::structural::contract::contract_tables::TablesStruct;
use crate::traits::structural::contract::contract_values::contract_blob_value::Blob;
use crate::traits::structural::repository::Repository;
use crate::traits::structural::repository::repo_values::RepositoryTableValue;

pub trait RepositoryBlobValue<R: Repository>:
    RepositoryTableValue<<<R as Scope>::Tables as TablesStruct>::Blob> + Blob
{
}
