// @review [ ]
use crate::traits::structural::contract::Scope;
use crate::traits::structural::contract::contract_keys::contract_subscription_key::SubscriptionKey;
use crate::traits::structural::contract::contract_tables::TablesStruct;
use crate::traits::structural::definition::Definition;
use crate::traits::structural::definition::def_keys::DefinitionTableKey;
use crate::traits::structural::repository::Repository;

pub trait DefinitionSubscriptionKey<R: Repository, D: Definition<R>>:
    DefinitionTableKey<<<D as Scope>::Tables as TablesStruct>::Subscription> + SubscriptionKey
{
}
