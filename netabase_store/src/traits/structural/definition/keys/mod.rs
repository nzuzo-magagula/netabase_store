// @review [~]
use crate::traits::structural::{Addressable, definition::tables::DefinitionTable};

pub mod blob;
pub mod primary;
pub mod relational;
pub mod secondary;
pub mod subscription;

pub trait DefinitionTableKey<T: DefinitionTable>: Addressable {}
