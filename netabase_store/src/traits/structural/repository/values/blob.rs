// @review [ ]
use crate::traits::structural::repository::tables::RepositoryTables;
use crate::traits::structural::repository::values::RepositoryTableValue;
use crate::traits::structural::repository::Repository;

pub trait RepositoryBlobValue<R: Repository>:
    RepositoryTableValue<<<R as Repository>::Tables as RepositoryTables<R>>::Blob>
{
}
