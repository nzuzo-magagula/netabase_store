// @review [ ]
use crate::traits::structural::Addressable;

pub mod contract_blob_key;
pub mod contract_primary_key;
pub mod contract_relational_key;
pub mod contract_secondary_key;
pub mod contract_subscription_key;

pub trait Key: Addressable {}
