// @review [~]
use crate::traits::structural::repository::values::primary::RepositoryPrimaryValue;
use crate::traits::structural::repository::{
    Repository, keys::primary::RepositoryPrimaryKey, tables::RepositoryTable,
};

pub trait RepositoryPrimaryTable<R: Repository>: RepositoryTable
where
    <Self as RepositoryTable>::Key: RepositoryPrimaryKey<R>,
    <Self as RepositoryTable>::Value: RepositoryPrimaryValue<R>,
{
}
