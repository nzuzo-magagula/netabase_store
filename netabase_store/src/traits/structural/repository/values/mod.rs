// @review [~]
pub mod blob;
pub mod primary;
pub mod relational;
pub mod secondary;
pub mod subscription;

use crate::traits::structural::{Addressable, repository::tables::RepositoryTable};

pub trait RepositoryTableValue<T: RepositoryTable>: Addressable {}
