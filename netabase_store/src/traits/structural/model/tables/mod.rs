// @review [~]
use crate::traits::structural::model::Definition;
use crate::traits::structural::model::{Model, keys::TableKey, values::TableValue};
use crate::traits::structural::repository::Repository;

pub mod blob;
pub mod primary;
pub mod relational;
pub mod secondary;
pub mod subscription;

pub trait ModelTable: std::marker::Sized {
    type Key: TableKey<Self>;
    type Value: TableValue<Self>;
}

pub trait ModelTables<R: Repository, D: Definition<R>, M: Model<R, D>> {
    type Primary: ModelTable;
    type Secondary: ModelTable;
    type Relational: ModelTable;
    type Subscription: ModelTable;
    type Blob: ModelTable;
}
