// @review [~]
use crate::traits::structural::{
    definition::Definition,
    model::{
        Model, keys::secondary::SecondaryKey, tables::ModelTable, values::secondary::SecondaryValue,
    },
    repository::Repository,
};

pub trait SecondaryTable<R: Repository, D: Definition<R>, M: Model<R, D>>: ModelTable
where
    <Self as ModelTable>::Key: SecondaryKey<R, D, M>,
    <Self as ModelTable>::Value: SecondaryValue<R, D, M>,
{
}
