// @review [ ]
use crate::traits::structural::model::ModelTables;
use crate::traits::structural::model::keys::TableKey;
use crate::traits::structural::{definition::Definition, model::Model, repository::Repository};

pub trait PrimaryKey<R: Repository, D: Definition<R>, M: Model<R, D>>:
    TableKey<<<M as Model<R, D>>::Tables as ModelTables<R, D, M>>::Primary>
{
}
