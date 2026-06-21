// @review [ ]
use crate::traits::structural::{
    Addressable, definition::tables::DefinitionTables, repository::Repository,
};

pub mod keys;
pub mod tables;
pub mod values;

pub trait Definition<R: Repository>: Sized + Addressable {
    type Tables: DefinitionTables<R, Self>;
}
