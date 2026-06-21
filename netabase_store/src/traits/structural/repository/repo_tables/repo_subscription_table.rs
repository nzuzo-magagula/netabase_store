// @review [~]
use crate::traits::structural::contract::contract_tables::TableStruct;
use crate::traits::structural::contract::contract_tables::contract_subscription_table::SubscriptionTableStruct;
use crate::traits::structural::repository::{
    Repository, repo_keys::repo_subscription_key::RepositorySubscriptionKey, repo_tables::RepositoryTable,
    repo_values::repo_subscription_value::RepositorySubscriptionValue,
};

pub trait RepositorySubscriptionTable<R: Repository>:
    RepositoryTable
    + SubscriptionTableStruct
    + TableStruct<Key: RepositorySubscriptionKey<R>, Value: RepositorySubscriptionValue<R>>
{
}
