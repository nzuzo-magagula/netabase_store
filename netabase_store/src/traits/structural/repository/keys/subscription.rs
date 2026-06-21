// @review [ ]
use crate::traits::structural::repository::keys::RepositoryTableKey;
use crate::traits::structural::repository::tables::RepositoryTables;
use crate::traits::structural::repository::Repository;

pub trait RepositorySubscriptionKey<R: Repository>:
    RepositoryTableKey<<<R as Repository>::Tables as RepositoryTables<R>>::Subscription>
{
}
