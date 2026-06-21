// @review [~]
use crate::traits::structural::contract::contract_tables::{TableStruct, TablesStruct};
use crate::traits::structural::definition::Definition;
use crate::traits::structural::model::Model;
use crate::traits::structural::model::model_keys::TableKey;
use crate::traits::structural::model::model_values::TableValue;
use crate::traits::structural::repository::Repository;

pub mod model_blob_table;
pub mod model_primary_table;
pub mod model_relational_table;
pub mod model_secondary_table;
pub mod model_subscription_table;

pub trait ModelTable: TableStruct<Key: TableKey<Self>, Value: TableValue<Self>> {}

pub trait ModelTables<R: Repository, D: Definition<R>, M: Model<R, D>>: TablesStruct {}
