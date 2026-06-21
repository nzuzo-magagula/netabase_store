// @review [ ]
use crate::traits::structural::contract::contract_tables::TableStruct;

pub trait PrimaryTableStruct:
    TableStruct<
        Key: crate::traits::structural::contract::contract_keys::contract_primary_key::PrimaryKey,
        Value: crate::traits::structural::contract::contract_values::contract_primary_value::Primary,
    >
{
}
