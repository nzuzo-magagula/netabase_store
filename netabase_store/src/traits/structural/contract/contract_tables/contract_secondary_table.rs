// @review [ ]
use crate::traits::structural::contract::contract_tables::TableStruct;

pub trait SecondaryTableStruct:
    TableStruct<
        Key: crate::traits::structural::contract::contract_keys::contract_secondary_key::SecondaryKey,
        Value: crate::traits::structural::contract::contract_values::contract_secondary_value::Secondary,
    >
{
}
