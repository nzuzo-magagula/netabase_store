// @review [~]
use crate::traits::structural::repository::values::relational::RepositoryRelationalValue;
use crate::traits::structural::repository::{
    Repository, keys::relational::RepositoryRelationalKey, tables::RepositoryTable,
};

pub trait RepositoryRelationalTable<R: Repository>: RepositoryTable
where
    <Self as RepositoryTable>::Key: RepositoryRelationalKey<R>,
    <Self as RepositoryTable>::Value: RepositoryRelationalValue<R>,
{
}
