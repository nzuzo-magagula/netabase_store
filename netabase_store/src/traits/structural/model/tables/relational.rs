// @review [~]
use crate::traits::structural::{
    definition::Definition,
    model::{
        Model, keys::relational::RelationalKey, tables::ModelTable,
        values::relational::RelationalValue,
    },
    repository::Repository,
};

pub trait RelationalTable<R: Repository, D: Definition<R>, M: Model<R, D>>: ModelTable
where
    <Self as ModelTable>::Key: RelationalKey<R, D, M>,
    <Self as ModelTable>::Value: RelationalValue<R, D, M>,
{
}
