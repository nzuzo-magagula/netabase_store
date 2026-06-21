// @review [ ]
use crate::traits::structural::repository::keys::RepositoryTableKey;
use crate::traits::structural::repository::tables::RepositoryTables;
use crate::traits::structural::repository::Repository;

pub trait RepositoryBlobKey<R: Repository>:
    RepositoryTableKey<<<R as Repository>::Tables as RepositoryTables<R>>::Blob>
{
}
