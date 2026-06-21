// @review [~]
use crate::traits::structural::contract::Scope;
use crate::traits::structural::{definition::Definition, repository::Repository};

pub mod model_keys;
pub mod model_tables;
pub mod model_values;

pub trait Model<R: Repository, D: Definition<R>>: Scope {}
