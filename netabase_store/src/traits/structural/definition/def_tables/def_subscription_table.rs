// @review [~]
use crate::traits::structural::contract::contract_tables::TableStruct;
use crate::traits::structural::contract::contract_tables::contract_subscription_table::SubscriptionTableStruct;
use crate::traits::structural::definition::{
    Definition, def_keys::def_subscription_key::DefinitionSubscriptionKey, def_tables::DefinitionTable,
    def_values::def_subscription_value::DefinitionSubscriptionValue,
};
use crate::traits::structural::repository::Repository;

pub trait DefinitionSubscriptionTable<R: Repository, D: Definition<R>>:
    DefinitionTable
    + SubscriptionTableStruct
    + TableStruct<Key: DefinitionSubscriptionKey<R, D>, Value: DefinitionSubscriptionValue<R, D>>
{
}
