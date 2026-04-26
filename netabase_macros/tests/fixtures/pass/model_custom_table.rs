// Declaring `custom_table(...)` suppresses the generated default no-op `CustomTableSideEffects`
// impl, so the user supplies their own. This fixture confirms that compiles and routes.
use netabase_macros::{NetabaseModel, netabase_definition};
use netabase_store::errors::NetabaseError;
use netabase_store::traits::structural::database::NetabaseStore;
use netabase_store::traits::structural::database::tables::auxiliary::custom::CustomTableSideEffects;
use netabase_store::traits::structural::database::transactions::repository::{
    RepositoryReadTx, RepositoryWriteTx,
};
use netabase_store::traits::structural::schema::definitions::NetabaseDefinition;
use netabase_store::traits::structural::schema::repositories::{NetabaseRepository, NoRepository};
use rkyv::{Archive, Deserialize, Serialize};

#[netabase_definition(CustomDefinition, repository(NoRepository))]
pub mod definition {
    use super::*;

    #[derive(Archive, Serialize, Deserialize, NetabaseModel, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
    #[rkyv(derive(Debug))]
    #[netabase(definition(CustomDefinition))]
    #[netabase(custom_table(AuditLog, u64, u64))]
    pub struct CustomModel {
        #[netabase(PrimaryKey)]
        pub id: u64,
    }
}

use definition::{CustomModel, CustomModelPrimaryKey};

impl<R: NetabaseRepository, D: NetabaseDefinition<R>> CustomTableSideEffects<R, D, CustomModel>
    for CustomModel
{
    fn on_insert<'db, DB: NetabaseStore<R>>(
        &self,
        _txn: &mut impl RepositoryWriteTx<'db, R, DB>,
        _model: &Self,
    ) -> Result<(), NetabaseError> {
        Ok(())
    }
    fn on_delete<'db, DB: NetabaseStore<R>>(
        &self,
        _txn: &mut impl RepositoryWriteTx<'db, R, DB>,
        _key: &CustomModelPrimaryKey,
    ) -> Result<(), NetabaseError> {
        Ok(())
    }
    fn on_get<'db, DB: NetabaseStore<R>>(
        &self,
        _txn: &impl RepositoryReadTx<'db, R, DB>,
        _key: &CustomModelPrimaryKey,
    ) -> Result<(), NetabaseError> {
        Ok(())
    }
}

fn main() {}
