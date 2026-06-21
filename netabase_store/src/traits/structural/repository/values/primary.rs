// @review [ ]
use crate::traits::structural::repository::tables::RepositoryTables;
use crate::traits::structural::repository::values::RepositoryTableValue;
use crate::traits::structural::repository::Repository;

pub trait RepositoryPrimaryValue<R: Repository>:
    RepositoryTableValue<<<R as Repository>::Tables as RepositoryTables<R>>::Primary>
{
}
