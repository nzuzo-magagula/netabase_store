// @review [ ]
use crate::traits::structural::contract::Scope;
use crate::traits::structural::contract::contract_tables::TablesStruct;
use crate::traits::structural::contract::contract_values::contract_subscription_value::Subscription;
use crate::traits::structural::definition::Definition;
use crate::traits::structural::definition::def_values::DefinitionTableValue;
use crate::traits::structural::repository::Repository;

pub trait DefinitionSubscriptionValue<R: Repository, D: Definition<R>>:
    DefinitionTableValue<<<D as Scope>::Tables as TablesStruct>::Subscription> + Subscription
{
}
