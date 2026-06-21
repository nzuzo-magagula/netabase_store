// @review [ ]
use crate::traits::structural::Addressable;

pub mod contract_blob_value;
pub mod contract_primary_value;
pub mod contract_relational_value;
pub mod contract_secondary_value;
pub mod contract_subscription_value;

pub trait Value: Addressable {}
