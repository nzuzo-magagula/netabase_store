// @review [~]
use crate::traits::structural::contract::contract_tables::TableStruct;
use crate::traits::structural::contract::contract_tables::contract_blob_table::BlobTableStruct;
use crate::traits::structural::repository::{
    Repository, repo_keys::repo_blob_key::RepositoryBlobKey, repo_tables::RepositoryTable,
    repo_values::repo_blob_value::RepositoryBlobValue,
};

pub trait RepositoryBlobTable<R: Repository>:
    RepositoryTable + BlobTableStruct + TableStruct<Key: RepositoryBlobKey<R>, Value: RepositoryBlobValue<R>>
{
}
