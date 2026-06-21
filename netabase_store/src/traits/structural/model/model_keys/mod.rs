// @review [~]
use crate::traits::structural::contract::contract_keys::Key;
use crate::traits::structural::contract::contract_tables::TableStruct;

pub mod model_blob_key;
pub mod model_primary_key;
pub mod model_relational_key;
pub mod model_secondary_key;
pub mod model_subscription_key;

pub trait TableKey<T: TableStruct>: Key {}
