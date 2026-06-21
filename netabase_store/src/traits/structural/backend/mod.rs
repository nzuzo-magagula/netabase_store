// @review [~]
use crate::traits::structural::contract::{Scope, contract_tables::TableStruct};

pub trait Backend<S: Scope> {
    type TableStore<T: TableStruct>: TableStore<T>;
}

pub trait TableStore<T: TableStruct> {}
