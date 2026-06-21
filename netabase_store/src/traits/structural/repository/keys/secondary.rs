// @review [ ]
use crate::traits::structural::repository::keys::RepositoryTableKey;
use crate::traits::structural::repository::tables::RepositoryTables;
use crate::traits::structural::repository::Repository;

pub trait RepositorySecondaryKey<R: Repository>:
    RepositoryTableKey<<<R as Repository>::Tables as RepositoryTables<R>>::Secondary>
{
}
