// @review [ ]
use crate::traits::structural::contract::Scope;
use crate::traits::structural::contract::contract_keys::contract_primary_key::PrimaryKey;
use crate::traits::structural::contract::contract_tables::TablesStruct;
use crate::traits::structural::model::model_keys::TableKey;
use crate::traits::structural::{definition::Definition, model::Model, repository::Repository};

pub trait ModelPrimaryKey<R: Repository, D: Definition<R>, M: Model<R, D>>:
    TableKey<<<M as Scope>::Tables as TablesStruct>::Primary> + PrimaryKey
{
}
