// @review [ ]
use crate::traits::structural::contract::contract_tables::TableStruct;

pub trait SubscriptionTableStruct:
    TableStruct<
        Key: crate::traits::structural::contract::contract_keys::contract_subscription_key::SubscriptionKey,
        Value: crate::traits::structural::contract::contract_values::contract_subscription_value::Subscription,
    >
{
}
