// @review [~]
use crate::traits::structural::repository::values::blob::RepositoryBlobValue;
use crate::traits::structural::repository::{
    Repository, keys::blob::RepositoryBlobKey, tables::RepositoryTable,
};

pub trait RepositoryBlobTable<R: Repository>: RepositoryTable
where
    <Self as RepositoryTable>::Key: RepositoryBlobKey<R>,
    <Self as RepositoryTable>::Value: RepositoryBlobValue<R>,
{
}
