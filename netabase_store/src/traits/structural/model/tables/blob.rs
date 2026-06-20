// @review [~]
use crate::traits::structural::model::values::blob::BlobValue;
use crate::traits::structural::{
    definition::Definition,
    model::{Model, keys::blob::BlobKey, tables::ModelTable},
    repository::Repository,
};

pub trait BlobTable<R: Repository, D: Definition<R>, M: Model<R, D>>: ModelTable
where
    <Self as ModelTable>::Key: BlobKey<R, D, M>,
    <Self as ModelTable>::Value: BlobValue<R, D, M>,
{
}
