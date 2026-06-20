// @review [ ]
use crate::traits::structural::model::keys::TableKey;
use crate::traits::structural::model::tables::{ModelTable, ModelTables};
use crate::traits::structural::{definition::Definition, model::Model, repository::Repository};

pub trait RelationalKey<R: Repository, D: Definition<R>, M: Model<R, D>>:
    TableKey<<<M as Model<R, D>>::Tables as ModelTables<R, D, M>>::Relational>
{
}
