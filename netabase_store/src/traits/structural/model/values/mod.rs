// @review [~]
pub mod blob;
pub mod primary;
pub mod relational;
pub mod secondary;
pub mod subscription;

use crate::traits::structural::model::tables::ModelTable;

pub trait TableValue<T: ModelTable> {}
