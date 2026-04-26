// @review [x]
pub mod definition;
pub mod model;
pub mod repository;

pub use definition::*;
pub use model::*;
pub use repository::*;

use crate::traits::structural::{
    database::{
        NetabaseStore,
        tables::{GetTable, TableKey, TableOwner, TableValue},
    },
    schema::repositories::NetabaseRepository,
};

pub trait NetabaseTransaction<'db, R: NetabaseRepository, DB: NetabaseStore<R>>: 'db {
    type Tables; // TODO(#id-6b5): C[S(Tables)]
    // TODO(#tx_hierarchy/id-7d2): V[Tr(DefinitionTransaction/ModelTransaction)], "Ensure definition/model transactions are thin wrappers over repository tx for atomicity."

    fn table<'a, O, K, V>(&'a self) -> &'a <Self::Tables as GetTable<O, K, V>>::Table
    where
        O: TableOwner,
        K: TableKey,
        V: TableValue,
        Self::Tables: GetTable<O, K, V>,
        R: 'a,
        DB: 'a,
        'db: 'a,
    {
        self.tables().get_table()
    }

    fn tables(&self) -> &Self::Tables;
}
