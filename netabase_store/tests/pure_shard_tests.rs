//! Pure-shard dedup. A `#[netabase(pure)]` model stores a skeleton in the Primary table with blob
//! + relational fields stripped (the aux tables are the source of truth); `get` rehydrates them.
//! Secondary fields stay in the Primary (keyed by value, not reconstructable).
use netabase_arena::fixed::{NbString, NbVec};
use netabase_macros::{NetabaseModel, netabase_definition, netabase_model, netabase_repository};
use rkyv::{Archive, Deserialize, Serialize};

#[netabase_repository(PureRepository)]
pub mod repositories {
    use super::*;

    #[netabase_definition(PureDefinition, repository(PureRepository))]
    pub mod pure_definition {
        use super::*;

        #[netabase_model]
        #[derive(Archive, Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
        #[rkyv(derive(Debug))]
        #[netabase(definition(PureDefinition), pure)]
        pub struct PureUser {
            #[netabase(PrimaryKey)]
            pub id: u32,
            #[netabase(secondary)]
            pub name: NbString<24>,
            #[netabase(relational(to = Note))]
            pub notes: Vec<Note>,
            #[netabase(blob)]
            pub avatar: NbVec<u8, 64>,
        }

        // Same fields, Linear layout: relational rehydration reads the single
        // `PureLinearUser_Relational` table instead of per-field tables.
        #[netabase_model]
        #[derive(Archive, Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
        #[rkyv(derive(Debug))]
        #[netabase(definition(PureDefinition), storage(linear), pure)]
        pub struct PureLinearUser {
            #[netabase(PrimaryKey)]
            pub id: u32,
            #[netabase(secondary)]
            pub name: NbString<24>,
            #[netabase(relational(to = Note))]
            pub notes: Vec<Note>,
            #[netabase(blob)]
            pub avatar: NbVec<u8, 64>,
        }

        #[derive(Archive, Serialize, Deserialize, NetabaseModel, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
        #[rkyv(derive(Debug))]
        #[netabase(definition(PureDefinition))]
        pub struct Note {
            #[netabase(PrimaryKey)]
            pub id: u64,
            pub text: NbString<48>,
        }
    }
}

#[test]
fn test_pure_shard_dedup_roundtrip() {
    use netabase_store::databases::memory::MemoryStore;
    use netabase_store::traits::structural::database::NetabaseStore;
    use netabase_store::traits::structural::database::tables::TableReadOps;
    use netabase_store::traits::structural::database::transactions::repository::{
        RepositoryTransaction, RepositoryWriteOps, RepositoryWriteTx,
    };
    use netabase_store::traits::structural::schema::repositories::NetabaseRepository;
    use repositories::pure_definition::{NotePrimaryKey, PureUser, PureUserName, PureUserPrimaryKey};
    use repositories::{PureDefinition, PureDefinitionPrimaryKey};

    let mut store = MemoryStore::<PureRepositoryItem>::open(()).unwrap();

    let user = PureUser {
        id: PureUserPrimaryKey(1),
        name: PureUserName(NbString::try_from_str("alice").unwrap()),
        notes: NbVec::try_from_slice(&[NotePrimaryKey(10), NotePrimaryKey(20)]).unwrap(),
        avatar: NbVec::try_from_slice(&[1, 2, 3, 4]).unwrap(),
    };

    let mut tx = store.write_transaction().unwrap();
    tx.insert(PureRepositoryItem::PureDefinition(PureDefinition::PureUser(user.clone())))
        .unwrap();

    // Dedup proof: the Primary record is a skeleton — blob + relational stripped, secondary kept.
    let primary = tx
        .open_read_table::<PureUser, PureUserPrimaryKey, PureUser>("PureUser")
        .unwrap();
    let stored = primary.get_value(&PureUserPrimaryKey(1)).unwrap().unwrap();
    assert!(stored.avatar.is_empty(), "blob stripped from Primary");
    assert!(stored.notes.is_empty(), "relational stripped from Primary");
    assert_eq!(stored.name.0.as_str(), "alice", "secondary stays in Primary");
    drop(primary);
    RepositoryWriteTx::commit(tx).unwrap();

    // route_get rehydrates blob (avatar) + relational (notes) to the full model.
    let rtx = store.read_transaction().unwrap();
    let pk = PureRepositoryPrimaryKey::PureDefinition(PureDefinitionPrimaryKey::PureUser(
        PureUserPrimaryKey(1),
    ));
    match PureRepositoryItem::route_get(&rtx, pk).unwrap() {
        Some(PureRepositoryItem::PureDefinition(PureDefinition::PureUser(m))) => {
            assert_eq!(m, user, "get rehydrates to the full inserted model");
        }
        other => panic!("expected PureUser, got {other:?}"),
    }
}

#[test]
fn test_pure_shard_dedup_linear_layout() {
    use netabase_store::databases::memory::MemoryStore;
    use netabase_store::traits::structural::database::NetabaseStore;
    use netabase_store::traits::structural::database::tables::TableReadOps;
    use netabase_store::traits::structural::database::transactions::repository::{
        RepositoryTransaction, RepositoryWriteOps, RepositoryWriteTx,
    };
    use netabase_store::traits::structural::schema::repositories::NetabaseRepository;
    use repositories::pure_definition::{
        NotePrimaryKey, PureLinearUser, PureLinearUserName, PureLinearUserPrimaryKey,
    };
    use repositories::{PureDefinition, PureDefinitionPrimaryKey};

    let mut store = MemoryStore::<PureRepositoryItem>::open(()).unwrap();

    let user = PureLinearUser {
        id: PureLinearUserPrimaryKey(1),
        name: PureLinearUserName(NbString::try_from_str("bob").unwrap()),
        notes: NbVec::try_from_slice(&[NotePrimaryKey(30), NotePrimaryKey(40)]).unwrap(),
        avatar: NbVec::try_from_slice(&[9, 8, 7]).unwrap(),
    };

    let mut tx = store.write_transaction().unwrap();
    tx.insert(PureRepositoryItem::PureDefinition(
        PureDefinition::PureLinearUser(user.clone()),
    ))
    .unwrap();

    let primary = tx
        .open_read_table::<PureLinearUser, PureLinearUserPrimaryKey, PureLinearUser>("PureLinearUser")
        .unwrap();
    let stored = primary.get_value(&PureLinearUserPrimaryKey(1)).unwrap().unwrap();
    assert!(stored.avatar.is_empty(), "blob stripped from Primary (linear)");
    assert!(stored.notes.is_empty(), "relational stripped from Primary (linear)");
    drop(primary);
    RepositoryWriteTx::commit(tx).unwrap();

    let rtx = store.read_transaction().unwrap();
    let pk = PureRepositoryPrimaryKey::PureDefinition(PureDefinitionPrimaryKey::PureLinearUser(
        PureLinearUserPrimaryKey(1),
    ));
    match PureRepositoryItem::route_get(&rtx, pk).unwrap() {
        Some(PureRepositoryItem::PureDefinition(PureDefinition::PureLinearUser(m))) => {
            assert_eq!(m, user, "linear get rehydrates to the full inserted model");
        }
        other => panic!("expected PureLinearUser, got {other:?}"),
    }
}
