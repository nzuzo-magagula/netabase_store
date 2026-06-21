// @review [ ]
use crate::traits::structural::{Addressable, repository::tables::RepositoryTables};

pub mod keys;
pub mod tables;
pub mod values;

pub trait Repository: Sized + Addressable {
    type Tables: RepositoryTables<Self>;
}
