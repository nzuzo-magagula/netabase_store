// @review [~]
use crate::traits::structural::contract::contract_keys::Key;
use crate::traits::structural::contract::contract_tables::TableStruct;

pub mod def_blob_key;
pub mod def_primary_key;
pub mod def_relational_key;
pub mod def_secondary_key;
pub mod def_subscription_key;

pub trait DefinitionTableKey<T: TableStruct>: Key {}
