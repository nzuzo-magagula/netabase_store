// @review [~]
use crate::traits::structural::contract::contract_tables::TableStruct;
use crate::traits::structural::contract::contract_tables::contract_blob_table::BlobTableStruct;
use crate::traits::structural::definition::{
    Definition, def_keys::def_blob_key::DefinitionBlobKey, def_tables::DefinitionTable,
    def_values::def_blob_value::DefinitionBlobValue,
};
use crate::traits::structural::repository::Repository;

pub trait DefinitionBlobTable<R: Repository, D: Definition<R>>:
    DefinitionTable
    + BlobTableStruct
    + TableStruct<Key: DefinitionBlobKey<R, D>, Value: DefinitionBlobValue<R, D>>
{
}
