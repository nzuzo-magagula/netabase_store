//! Exercises the custom-table side-effect hook. Declaring `custom_table(...)` on a model suppresses
//! the generated default no-op `CustomTableSideEffects` impl, letting the user provide a real one;
//! the orchestrator invokes it during insert/delete/get (honoring `exclude_custom`).
use netabase_arena::fixed::NbString;
use netabase_macros::{netabase_definition, netabase_model, netabase_repository};
use netabase_store::errors::NetabaseError;
use netabase_store::traits::structural::database::NetabaseStore;
use netabase_store::traits::structural::database::tables::codec::serialize_value;
use netabase_store::traits::structural::database::tables::core::{
    Blake3Hasher, ModelHash, NetabaseHasher, TableReadOps, TableWriteOps,
};
use netabase_store::traits::structural::database::transactions::repository::{
    RepositoryReadTx, RepositoryTransaction, RepositoryWriteOps, RepositoryWriteTx,
};
use netabase_store::traits::structural::schema::definitions::NetabaseDefinition;
use netabase_store::traits::structural::schema::repositories::NetabaseRepository;
use rkyv::{Archive, Deserialize, Serialize};

const CUSTOM_TABLE: &str = "AuditModel_Custom";

#[netabase_repository(AuditRepo)]
pub mod repo {
    use super::*;

    #[netabase_definition(AuditDef, repository(AuditRepo))]
    pub mod definition {
        use super::*;

        #[netabase_model]
        #[derive(Archive, Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
        #[rkyv(derive(Debug))]
        #[netabase(definition(AuditDef))]
        #[netabase(custom_table(AuditLog, u64, u64))]
        pub struct AuditModel {
            #[netabase(PrimaryKey)]
            pub id: u64,
            pub label: NbString<24>,
        }
    }
}

use repo::definition::{AuditModel, AuditModelPrimaryKey};

// User-provided custom side effects: mirror each insert/delete into the audit table keyed by the
// model's primary key, valued by the model's content hash.
impl<R: NetabaseRepository, D: NetabaseDefinition<R>>
    netabase_store::traits::structural::database::tables::auxiliary::custom::CustomTableSideEffects<
        R,
        D,
        AuditModel,
    > for AuditModel
{
    fn on_insert<'db, DB: NetabaseStore<R>>(
        &self,
        txn: &mut impl RepositoryWriteTx<'db, R, DB>,
        model: &Self,
    ) -> Result<(), NetabaseError> {
        let hash = Blake3Hasher::hash(&serialize_value(model)?);
        let mut t =
            txn.open_write_table::<AuditModel, AuditModelPrimaryKey, ModelHash>(CUSTOM_TABLE)?;
        t.insert(&model.primary_key(), &hash)?;
        Ok(())
    }

    fn on_delete<'db, DB: NetabaseStore<R>>(
        &self,
        txn: &mut impl RepositoryWriteTx<'db, R, DB>,
        key: &AuditModelPrimaryKey,
    ) -> Result<(), NetabaseError> {
        let mut t =
            txn.open_write_table::<AuditModel, AuditModelPrimaryKey, ModelHash>(CUSTOM_TABLE)?;
        t.remove(key)?;
        Ok(())
    }

    fn on_get<'db, DB: NetabaseStore<R>>(
        &self,
        _txn: &impl RepositoryReadTx<'db, R, DB>,
        _key: &AuditModelPrimaryKey,
    ) -> Result<(), NetabaseError> {
        Ok(())
    }
}

#[test]
fn custom_side_effect_fires_on_insert_and_delete() {
    let mut store =
        netabase_store::databases::memory::MemoryStore::<AuditRepoItem>::open(()).unwrap();

    let model = AuditModel {
        id: AuditModelPrimaryKey(7),
        label: NbString::try_from_str("audited").unwrap(),
    };
    let expected = Blake3Hasher::hash(&serialize_value(&model).unwrap());

    // Insert routes repository → definition → model orchestration, invoking on_insert.
    let mut txn = store.write_transaction().unwrap();
    txn.insert(AuditRepoItem::AuditDef(repo::AuditDef::AuditModel(model.clone())))
        .unwrap();
    RepositoryWriteTx::commit(txn).unwrap();

    {
        let txn = store.read_transaction().unwrap();
        let t = txn
            .open_read_table::<AuditModel, AuditModelPrimaryKey, ModelHash>(CUSTOM_TABLE)
            .unwrap();
        assert_eq!(
            t.get_value(&AuditModelPrimaryKey(7)).unwrap(),
            Some(expected),
            "on_insert should have written the content hash to the custom table"
        );
    }

    // Delete routes through orchestration and invokes on_delete.
    let mut txn = store.write_transaction().unwrap();
    AuditRepoItem::route_delete(
        AuditRepoAddress::AuditDef(repo::AuditDefAddress::AuditModel),
        AuditRepoPrimaryKey::AuditDef(repo::AuditDefPrimaryKey::AuditModel(AuditModelPrimaryKey(7))),
        &mut txn,
    )
    .unwrap();
    RepositoryWriteTx::commit(txn).unwrap();

    let txn = store.read_transaction().unwrap();
    let t = txn
        .open_read_table::<AuditModel, AuditModelPrimaryKey, ModelHash>(CUSTOM_TABLE)
        .unwrap();
    assert!(
        t.get(&AuditModelPrimaryKey(7)).unwrap().is_none(),
        "on_delete should have removed the audit row"
    );
}
