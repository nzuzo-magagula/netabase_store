// @review [~]
use crate::traits::structural::repository::values::subscription::RepositorySubscriptionValue;
use crate::traits::structural::repository::{
    Repository, keys::subscription::RepositorySubscriptionKey, tables::RepositoryTable,
};

pub trait RepositorySubscriptionTable<R: Repository>: RepositoryTable
where
    <Self as RepositoryTable>::Key: RepositorySubscriptionKey<R>,
    <Self as RepositoryTable>::Value: RepositorySubscriptionValue<R>,
{
}
