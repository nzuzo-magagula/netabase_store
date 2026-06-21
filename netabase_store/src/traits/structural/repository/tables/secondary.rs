// @review [~]
use crate::traits::structural::repository::values::secondary::RepositorySecondaryValue;
use crate::traits::structural::repository::{
    Repository, keys::secondary::RepositorySecondaryKey, tables::RepositoryTable,
};

pub trait RepositorySecondaryTable<R: Repository>: RepositoryTable
where
    <Self as RepositoryTable>::Key: RepositorySecondaryKey<R>,
    <Self as RepositoryTable>::Value: RepositorySecondaryValue<R>,
{
}
