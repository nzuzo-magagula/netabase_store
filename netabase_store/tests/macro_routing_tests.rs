//! Hierarchical routing, the owning write-view, and ordered-key round-trips
//! across the model/definition/repository key enums.
use netabase_arena::fixed::NbString;
use netabase_macros::{NetabaseModel, netabase_definition, netabase_repository};
use netabase_store::keys::ordered::{KeyCodecError, OrderedKeyEncoding};
use netabase_store::traits::behavioural::TransactionHooks;
use netabase_store::traits::structural::database::{
    NetabaseStore, transactions::RepositoryWriteOps,
};
use rkyv::{Archive, Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_db_dir(prefix: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time moved backwards")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("{}_{}", prefix, nanos));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[netabase_repository(MyRepo)]
pub mod my_repository {
    use super::*;

    #[netabase_definition(MainDefinition, repository(MyRepo))]
    pub mod main_definition {
        use super::*;

        #[derive(Archive, Serialize, Deserialize, NetabaseModel, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
        #[rkyv(derive(Debug))]
        #[netabase(definition(MainDefinition))]
        pub struct User {
            #[netabase(primary_key)]
            pub id: u64,
            pub name: NbString<32>,
        }
    }
}

use my_repository::main_definition::{User, UserPrimaryKey};

/// Round-trip a key through its ordered encoding (`encode_into` / `decode`).
fn roundtrip_key<K: OrderedKeyEncoding + PartialEq + std::fmt::Debug>(key: &K) {
    let mut buf = vec![0u8; K::MAX_ENCODED_LEN];
    let n = key.encode_into(&mut buf).unwrap();
    let decoded = K::decode_exact(&buf[..n]).unwrap();
    assert_eq!(&decoded, key);
}

#[test]
fn test_hierarchical_routing_insert() {
    use netabase_store::traits::structural::database::tables::TableReadOps;
    use netabase_store::traits::structural::database::transactions::repository::RepositoryTransaction;

    let dir = temp_db_dir("macro_routing");
    let mut store = netabase_store::databases::redb::RedbStore::<MyRepoItem>::open(dir.join("r.redb")).unwrap();

    let user = User {
        id: 42,
        name: NbString::try_from_str("Alice").unwrap(),
    };

    let mut tx = store.write_transaction().unwrap();
    tx.insert(MyRepoItem::MainDefinition(
        my_repository::MainDefinition::User(user.clone()),
    ))
    .unwrap();

    let table = tx
        .open_read_table::<User, UserPrimaryKey, User>("User")
        .unwrap();
    let fetched = table.get_value(&UserPrimaryKey(42)).unwrap().unwrap();
    assert_eq!(fetched.name.as_str(), "Alice");
    drop(table);
    netabase_store::traits::structural::database::transactions::repository::RepositoryWriteTx::commit(tx).unwrap();

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_key_enums_round_trip_and_hooks() {
    let user = User {
        id: 7,
        name: NbString::try_from_str("Bob").unwrap(),
    };

    // Ordered-key round-trips up the tree: model → definition → repository.
    let user_pk = UserPrimaryKey(7);
    roundtrip_key(&user_pk);
    let definition_pk = my_repository::MainDefinitionPrimaryKey::User(user_pk.clone());
    roundtrip_key(&definition_pk);
    let repository_pk = MyRepoPrimaryKey::MainDefinition(definition_pk.clone());
    roundtrip_key(&repository_pk);

    // Transaction hooks exist at every level.
    let definition = my_repository::MainDefinition::User(user.clone());
    let repository = MyRepoItem::MainDefinition(definition.clone());
    user.pre_transaction().unwrap();
    user.post_transaction().unwrap();
    definition.pre_transaction().unwrap();
    definition.post_transaction().unwrap();
    repository.pre_transaction().unwrap();
    repository.post_transaction().unwrap();
}

#[test]
fn test_unknown_variant_tag_is_rejected() {
    // A leading variant tag past the known range decodes to UnknownTag.
    let bad = [99u8, 0, 0, 0, 0, 0, 0, 0, 0];
    assert_eq!(
        my_repository::MainDefinitionPrimaryKey::decode(&bad).unwrap_err(),
        KeyCodecError::UnknownTag(99),
    );
    assert_eq!(
        MyRepoPrimaryKey::decode(&bad).unwrap_err(),
        KeyCodecError::UnknownTag(99),
    );
}

#[test]
fn test_owning_write_view_insert_delete() {
    use my_repository::MainDefinition;
    use my_repository::main_definition::UserWriteView;
    use netabase_store::traits::structural::database::tables::InsertConfig;
    use netabase_store::traits::structural::database::tables::TableReadOps;
    use netabase_store::traits::structural::database::transactions::repository::RepositoryTransaction;

    let dir = temp_db_dir("owning_view");
    let mut store = netabase_store::databases::redb::RedbStore::<MyRepoItem>::open(dir.join("v.redb")).unwrap();
    let mut tx = store.write_transaction().unwrap();

    let cfg = InsertConfig::new();
    {
        let mut view = UserWriteView::<_, MainDefinition, _, _>::open(&mut tx);
        view.insert(
            User {
                id: 7,
                name: NbString::try_from_str("View").unwrap(),
            },
            &cfg,
        )
        .unwrap();
    }
    {
        let table = tx.open_read_table::<User, UserPrimaryKey, User>("User").unwrap();
        assert_eq!(
            table.get_value(&UserPrimaryKey(7)).unwrap().unwrap().name.as_str(),
            "View"
        );
    }
    {
        let mut view = UserWriteView::<_, MainDefinition, _, _>::open(&mut tx);
        view.delete(UserPrimaryKey(7)).unwrap();
    }
    {
        let table = tx.open_read_table::<User, UserPrimaryKey, User>("User").unwrap();
        assert!(table.get(&UserPrimaryKey(7)).unwrap().is_none());
    }
    netabase_store::traits::structural::database::transactions::repository::RepositoryWriteTx::commit(tx).unwrap();

    let _ = std::fs::remove_dir_all(dir);
}
