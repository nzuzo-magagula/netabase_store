// @review [~]
use crate::traits::structural::contract::contract_tables::TableStruct;
use crate::traits::structural::contract::contract_tables::contract_subscription_table::SubscriptionTableStruct;
use crate::traits::structural::{
    definition::Definition,
    model::{
        Model, model_keys::model_subscription_key::ModelSubscriptionKey, model_tables::ModelTable,
        model_values::model_subscription_value::ModelSubscriptionValue,
    },
    repository::Repository,
};

pub trait ModelSubscriptionTable<R: Repository, D: Definition<R>, M: Model<R, D>>:
    ModelTable
    + SubscriptionTableStruct
    + TableStruct<Key: ModelSubscriptionKey<R, D, M>, Value: ModelSubscriptionValue<R, D, M>>
{
}
