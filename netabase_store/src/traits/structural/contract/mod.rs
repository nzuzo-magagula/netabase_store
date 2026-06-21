// @review [ ]
use crate::traits::structural::{Addressable, contract::contract_tables::TablesStruct};

pub mod contract_keys;
pub mod contract_tables;
pub mod contract_values;

pub trait Scope: Sized + Addressable {
    type Tables: TablesStruct;
}
