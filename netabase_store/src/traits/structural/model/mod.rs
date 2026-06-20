// @review [~]
use crate::traits::structural::{
    definition::Definition, model::tables::ModelTables, repository::Repository,
};

pub mod keys;
pub mod tables;
pub mod values;

pub trait Model<R: Repository, D: Definition<R>>: Sized {
    type Tables: ModelTables<R, D, Self>;
}
