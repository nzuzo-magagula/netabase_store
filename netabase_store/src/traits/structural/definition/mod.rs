// @review [ ]
use crate::traits::structural::contract::Scope;
use crate::traits::structural::repository::Repository;

pub mod def_keys;
pub mod def_tables;
pub mod def_values;

pub trait Definition<R: Repository>: Scope {}
