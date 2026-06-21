// @review [ ]
use crate::traits::structural::contract::Scope;
use crate::traits::structural::contract::contract_tables::TablesStruct;
use crate::traits::structural::contract::contract_values::contract_blob_value::Blob;
use crate::traits::structural::model::model_values::TableValue;
use crate::traits::structural::{definition::Definition, model::Model, repository::Repository};

pub trait ModelBlobValue<R: Repository, D: Definition<R>, M: Model<R, D>>:
    TableValue<<<M as Scope>::Tables as TablesStruct>::Blob> + Blob
{
}
