// @review [ ]
use crate::traits::structural::contract::contract_tables::TableStruct;

pub trait RelationalTableStruct:
    TableStruct<
        Key: crate::traits::structural::contract::contract_keys::contract_relational_key::RelationalKey,
        Value: crate::traits::structural::contract::contract_values::contract_relational_value::Relational,
    >
{
}
