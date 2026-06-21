// @review [~]
use crate::traits::structural::contract::contract_tables::TableStruct;
use crate::traits::structural::contract::contract_tables::contract_primary_table::PrimaryTableStruct;
use crate::traits::structural::{
    definition::Definition,
    model::{Model, model_keys::model_primary_key::ModelPrimaryKey, model_tables::ModelTable, model_values::model_primary_value::ModelPrimaryValue},
    repository::Repository,
};

pub trait ModelPrimaryTable<R: Repository, D: Definition<R>, M: Model<R, D>>:
    ModelTable
    + PrimaryTableStruct
    + TableStruct<Key: ModelPrimaryKey<R, D, M>, Value: ModelPrimaryValue<R, D, M>>
{
}
