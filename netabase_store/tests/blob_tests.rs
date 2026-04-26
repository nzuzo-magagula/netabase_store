//! Blob storage: whole-model and field-level strategies, large multi-chunk payloads, and the
//! fetch-indices / read-chunks surface. Blob fields are fixed-capacity (`NbString`/`NbVec`); their
//! canonical bytes are split into 1KB chunks in the blob table and reassembled on read.
use netabase_arena::fixed::{NbString, NbVec};
use netabase_macros::{netabase_definition, netabase_model, netabase_repository};
use netabase_store::databases::memory::MemoryStore;
use netabase_store::traits::structural::database::NetabaseStore;
use netabase_store::traits::structural::database::transactions::repository::{
    RepositoryWriteOps, RepositoryWriteTx,
};
use netabase_store::traits::structural::schema::definitions::NetabaseDefinition;
use netabase_store::traits::structural::schema::repositories::NetabaseRepository;
use rkyv::{Archive, Deserialize, Serialize};

#[netabase_repository(BlobRepo)]
pub mod repo {
    use super::*;

    #[netabase_definition(BlobDef, repository(BlobRepo))]
    pub mod definition {
        use super::*;

        #[netabase_model]
        #[derive(Archive, Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
        #[rkyv(derive(Debug))]
        #[netabase(definition(BlobDef), blob(strategy = whole))]
        pub struct WholeModel {
            #[netabase(PrimaryKey)]
            pub id: u64,
            #[netabase(blob)]
            pub data: NbString<16384>,
        }

        #[netabase_model]
        #[derive(Archive, Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
        #[rkyv(derive(Debug))]
        #[netabase(definition(BlobDef), blob(strategy = field))]
        pub struct FieldModel {
            #[netabase(PrimaryKey)]
            pub id: u64,
            pub title: NbString<32>,
            #[netabase(blob)]
            pub content: NbString<4096>,
            #[netabase(blob)]
            pub large_data: NbVec<u8, 4096>,
        }

        #[netabase_model]
        #[derive(Archive, Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
        #[rkyv(derive(Debug))]
        #[netabase(definition(BlobDef))]
        pub struct DefaultStrategyModel {
            #[netabase(PrimaryKey)]
            pub id: u64,
            #[netabase(blob)]
            pub data: NbString<256>,
        }

        #[netabase_model]
        #[derive(Archive, Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
        #[rkyv(derive(Debug))]
        #[netabase(definition(BlobDef), blob(strategy = field))]
        pub struct MixedModel {
            #[netabase(PrimaryKey)]
            pub id: u64,
            pub normal_field: NbString<64>,
            #[netabase(blob)]
            pub blob_field: NbString<256>,
        }
    }
}

use repo::BlobDef;

fn store() -> MemoryStore<BlobRepoItem> {
    MemoryStore::<BlobRepoItem>::open(()).unwrap()
}

#[test]
fn test_default_strategy_has_blob_indices() {
    let mut store = store();
    let model = repo::definition::DefaultStrategyModel {
        id: repo::definition::DefaultStrategyModelPrimaryKey(1),
        data: NbString::try_from_str("default strategy").unwrap(),
    };
    let mut txn = store.write_transaction().unwrap();
    txn.insert(BlobRepoItem::BlobDef(BlobDef::DefaultStrategyModel(model)))
        .unwrap();
    RepositoryWriteTx::commit(txn).unwrap();

    let txn = store.read_transaction().unwrap();
    let def = BlobDef::DefaultStrategyModel(Default::default());
    let indices = def
        .route_fetch_blob_indices(
            &txn,
            repo::BlobDefPrimaryKey::DefaultStrategyModel(
                repo::definition::DefaultStrategyModelPrimaryKey(1),
            ),
        )
        .unwrap();
    assert!(!indices.is_empty());
}

#[test]
fn test_large_blob_multi_chunk() {
    let mut store = store();
    let large = "Large content ".repeat(1000); // ~14KB → >13 chunks of 1KB
    let model = repo::definition::WholeModel {
        id: repo::definition::WholeModelPrimaryKey(1),
        data: NbString::try_from_str(&large).unwrap(),
    };
    let mut txn = store.write_transaction().unwrap();
    txn.insert(BlobRepoItem::BlobDef(BlobDef::WholeModel(model.clone())))
        .unwrap();
    RepositoryWriteTx::commit(txn).unwrap();

    let txn = store.read_transaction().unwrap();
    match BlobRepoItem::route_get(
        &txn,
        BlobRepoPrimaryKey::BlobDef(repo::BlobDefPrimaryKey::WholeModel(
            repo::definition::WholeModelPrimaryKey(1),
        )),
    )
    .unwrap()
    {
        Some(BlobRepoItem::BlobDef(BlobDef::WholeModel(m))) => {
            assert_eq!(m.data.as_str(), large);
        }
        other => panic!("expected WholeModel, got {other:?}"),
    }
}

#[test]
fn test_mixed_model_content() {
    let mut store = store();
    let model = repo::definition::MixedModel {
        id: repo::definition::MixedModelPrimaryKey(1),
        normal_field: NbString::try_from_str("in primary table").unwrap(),
        blob_field: NbString::try_from_str("in blob table").unwrap(),
    };
    let mut txn = store.write_transaction().unwrap();
    txn.insert(BlobRepoItem::BlobDef(BlobDef::MixedModel(model.clone())))
        .unwrap();
    RepositoryWriteTx::commit(txn).unwrap();

    let txn = store.read_transaction().unwrap();
    match BlobRepoItem::route_get(
        &txn,
        BlobRepoPrimaryKey::BlobDef(repo::BlobDefPrimaryKey::MixedModel(
            repo::definition::MixedModelPrimaryKey(1),
        )),
    )
    .unwrap()
    {
        Some(BlobRepoItem::BlobDef(BlobDef::MixedModel(m))) => {
            assert_eq!(m, model);
        }
        other => panic!("expected MixedModel, got {other:?}"),
    }
}

#[test]
fn test_whole_model_blobbing() {
    let mut store = store();
    let model = repo::definition::WholeModel {
        id: repo::definition::WholeModelPrimaryKey(1),
        data: NbString::try_from_str(&"whole model blob ".repeat(100)).unwrap(),
    };
    let mut txn = store.write_transaction().unwrap();
    txn.insert(BlobRepoItem::BlobDef(BlobDef::WholeModel(model.clone())))
        .unwrap();
    RepositoryWriteTx::commit(txn).unwrap();

    let txn = store.read_transaction().unwrap();
    match BlobRepoItem::route_get(
        &txn,
        BlobRepoPrimaryKey::BlobDef(repo::BlobDefPrimaryKey::WholeModel(
            repo::definition::WholeModelPrimaryKey(1),
        )),
    )
    .unwrap()
    {
        Some(BlobRepoItem::BlobDef(BlobDef::WholeModel(m))) => assert_eq!(m, model),
        other => panic!("expected WholeModel, got {other:?}"),
    }
}

#[test]
fn test_field_model_blobbing() {
    let mut store = store();
    let model = repo::definition::FieldModel {
        id: repo::definition::FieldModelPrimaryKey(1),
        title: NbString::try_from_str("Field Level Blobbing").unwrap(),
        content: NbString::try_from_str(&"only specific fields ".repeat(50)).unwrap(),
        large_data: NbVec::try_from_slice(&[0u8; 1000]).unwrap(),
    };
    let mut txn = store.write_transaction().unwrap();
    txn.insert(BlobRepoItem::BlobDef(BlobDef::FieldModel(model.clone())))
        .unwrap();
    RepositoryWriteTx::commit(txn).unwrap();

    let txn = store.read_transaction().unwrap();
    let pk = repo::definition::FieldModelPrimaryKey(1);

    // Full get reassembles both blob fields.
    match BlobRepoItem::route_get(
        &txn,
        BlobRepoPrimaryKey::BlobDef(repo::BlobDefPrimaryKey::FieldModel(pk.clone())),
    )
    .unwrap()
    {
        Some(BlobRepoItem::BlobDef(BlobDef::FieldModel(m))) => assert_eq!(m, model),
        other => panic!("expected FieldModel, got {other:?}"),
    }

    // Fetch indices then read chunks.
    let def = BlobDef::FieldModel(Default::default());
    let indices = def
        .route_fetch_blob_indices(&txn, repo::BlobDefPrimaryKey::FieldModel(pk.clone()))
        .unwrap();
    assert!(!indices.is_empty());
    let chunks = def.route_read_blob_chunks(&txn, indices).unwrap();
    assert!(!chunks.is_empty());
}
