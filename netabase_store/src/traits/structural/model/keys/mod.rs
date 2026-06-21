// @review [~]
use crate::traits::structural::{Addressable, model::tables::ModelTable};

pub mod blob;
pub mod primary;
pub mod relational;
pub mod secondary;
pub mod subscription;

pub trait TableKey<T: ModelTable>: Addressable {}
