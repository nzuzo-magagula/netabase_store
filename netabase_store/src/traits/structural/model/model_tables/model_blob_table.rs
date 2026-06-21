// @review [~]
use crate::traits::structural::contract::contract_tables::TableStruct;
use crate::traits::structural::contract::contract_tables::contract_blob_table::BlobTableStruct;
use crate::traits::structural::{
    definition::Definition,
    model::{Model, model_keys::model_blob_key::ModelBlobKey, model_tables::ModelTable, model_values::model_blob_value::ModelBlobValue},
    repository::Repository,
};

pub trait ModelBlobTable<R: Repository, D: Definition<R>, M: Model<R, D>>:
    ModelTable + BlobTableStruct + TableStruct<Key: ModelBlobKey<R, D, M>, Value: ModelBlobValue<R, D, M>>
{
}
