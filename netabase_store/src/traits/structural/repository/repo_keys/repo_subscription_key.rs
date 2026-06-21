// @review [ ]
use crate::traits::structural::contract::Scope;
use crate::traits::structural::contract::contract_keys::contract_subscription_key::SubscriptionKey;
use crate::traits::structural::contract::contract_tables::TablesStruct;
use crate::traits::structural::repository::Repository;
use crate::traits::structural::repository::repo_keys::RepositoryTableKey;

pub trait RepositorySubscriptionKey<R: Repository>:
    RepositoryTableKey<<<R as Scope>::Tables as TablesStruct>::Subscription> + SubscriptionKey
{
}
