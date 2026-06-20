// @review [~]
use crate::traits::structural::{
    definition::Definition,
    model::{
        Model, keys::subscription::SubscriptionKey, tables::ModelTable,
        values::subscription::SubscriptionValue,
    },
    repository::Repository,
};

pub trait SubscriptionTable<R: Repository, D: Definition<R>, M: Model<R, D>>: ModelTable
where
    <Self as ModelTable>::Key: SubscriptionKey<R, D, M>,
    <Self as ModelTable>::Value: SubscriptionValue<R, D, M>,
{
}
