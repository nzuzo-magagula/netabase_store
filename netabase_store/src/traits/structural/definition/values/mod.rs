// @review [~]
pub mod blob;
pub mod primary;
pub mod relational;
pub mod secondary;
pub mod subscription;

use crate::traits::structural::{Addressable, definition::tables::DefinitionTable};

pub trait DefinitionTableValue<T: DefinitionTable>: Addressable {}
