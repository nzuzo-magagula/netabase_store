// @review [~]
use crate::traits::structural::{
    definition::Definition,
    model::{Model, keys::primary::PrimaryKey, tables::ModelTable, values::primary::PrimaryValue},
    repository::Repository,
};

pub trait PrimaryTable<R: Repository, D: Definition<R>, M: Model<R, D>>: ModelTable
where
    <Self as ModelTable>::Key: PrimaryKey<R, D, M>,
    <Self as ModelTable>::Value: PrimaryValue<R, D, M>,
{
}
