// @review [~]
use crate::traits::structural::{Addressable, repository::tables::RepositoryTable};

pub mod blob;
pub mod primary;
pub mod relational;
pub mod secondary;
pub mod subscription;

pub trait RepositoryTableKey<T: RepositoryTable>: Addressable {}
