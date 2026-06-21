// @review [~]
use crate::traits::structural::contract::contract_tables::TableStruct;
use crate::traits::structural::contract::contract_tables::contract_relational_table::RelationalTableStruct;
use crate::traits::structural::{
    definition::Definition,
    model::{
        Model, model_keys::model_relational_key::ModelRelationalKey, model_tables::ModelTable,
        model_values::model_relational_value::ModelRelationalValue,
    },
    repository::Repository,
};

pub trait ModelRelationalTable<R: Repository, D: Definition<R>, M: Model<R, D>>:
    ModelTable
    + RelationalTableStruct
    + TableStruct<Key: ModelRelationalKey<R, D, M>, Value: ModelRelationalValue<R, D, M>>
{
}
