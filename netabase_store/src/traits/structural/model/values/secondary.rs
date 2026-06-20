// @review [ ]
use crate::traits::structural::model::tables::{ModelTable, ModelTables};
use crate::traits::structural::model::values::TableValue;
use crate::traits::structural::{definition::Definition, model::Model, repository::Repository};

pub trait SecondaryValue<R: Repository, D: Definition<R>, M: Model<R, D>>:
    TableValue<<<M as Model<R, D>>::Tables as ModelTables<R, D, M>>::Secondary>
{
}
