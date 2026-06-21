// @review [~]
use crate::traits::structural::contract::contract_tables::TableStruct;
use crate::traits::structural::contract::contract_tables::contract_secondary_table::SecondaryTableStruct;
use crate::traits::structural::{
    definition::Definition,
    model::{
        Model, model_keys::model_secondary_key::ModelSecondaryKey, model_tables::ModelTable, model_values::model_secondary_value::ModelSecondaryValue,
    },
    repository::Repository,
};

pub trait ModelSecondaryTable<R: Repository, D: Definition<R>, M: Model<R, D>>:
    ModelTable
    + SecondaryTableStruct
    + TableStruct<Key: ModelSecondaryKey<R, D, M>, Value: ModelSecondaryValue<R, D, M>>
{
}
