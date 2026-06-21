use strum::IntoDiscriminant;

// @review [ ]
use crate::traits::structural::Addressable;
use crate::traits::structural::contract::contract_keys::Key;
use crate::traits::structural::contract::contract_values::Value;

pub mod contract_blob_table;
pub mod contract_primary_table;
pub mod contract_relational_table;
pub mod contract_secondary_table;
pub mod contract_subscription_table;

pub trait TableStruct: Sized + Addressable {
    type Key: Key;
    type Value: Value;
}

pub trait TablesStruct: Addressable + IntoDiscriminant {
    type Primary: TableStruct;
    type Secondary: TableStruct;
    type Relational: TableStruct;
    type Subscription: TableStruct;
    type Blob: TableStruct;
}
