// @review [ ]
use crate::traits::structural::contract::contract_tables::TableStruct;

pub trait BlobTableStruct:
    TableStruct<
        Key: crate::traits::structural::contract::contract_keys::contract_blob_key::BlobKey,
        Value: crate::traits::structural::contract::contract_values::contract_blob_value::Blob,
    >
{
}
