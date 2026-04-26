// @review [ ]
use super::core::{StoreTable, TableKey, TableOwner, TableValue};

pub trait GetTable<O, K, V>
where
    O: TableOwner,
    K: TableKey,
    V: TableValue,
{
    type Table: StoreTable<O, K, V>;
    fn get_table(&self) -> &Self::Table;
}
