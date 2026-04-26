//! The relational multimap toggle: a sequence relation (`Vec`/`NbVec`) is
//! stored as a one-to-many multimap — one entry per related element, valued by
//! the related model's ordered-encoded primary key.
use netabase_arena::fixed::NbVec;
use netabase_macros::{netabase_definition, netabase_model, netabase_repository};
use rkyv::{Archive, Deserialize, Serialize};

#[netabase_repository(RelRepo)]
pub mod relrepo {
    use super::*;

    #[netabase_definition(RelDef, repository(RelRepo))]
    pub mod reldef {
        use super::*;

        #[netabase_model]
        #[derive(Archive, Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
        #[rkyv(derive(Debug))]
        #[netabase(definition(RelDef))]
        pub struct Friend {
            #[netabase(PrimaryKey)]
            pub id: u64,
        }

        #[netabase_model]
        #[derive(Archive, Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
        #[rkyv(derive(Debug))]
        #[netabase(definition(RelDef))]
        pub struct Person {
            #[netabase(PrimaryKey)]
            pub id: u64,
            // `Vec` relation -> multimap (mutated to `NbVec<FriendPrimaryKey, _>`).
            #[netabase(relational(to = Friend))]
            pub friends: Vec<Friend>,
        }
    }
}

#[test]
fn test_vec_relation_is_multimap() {
    use crate::relrepo::reldef::{
        Friend, FriendPrimaryKey, Person, PersonPrimaryKey, PersonRelationalKeys,
    };
    use netabase_store::databases::memory::MemoryStore;
    use netabase_store::keys::ordered::decode_rel_value;
    use netabase_store::traits::structural::database::NetabaseStore;
    use netabase_store::traits::structural::database::tables::core::TableReadOps;
    use netabase_store::traits::structural::database::transactions::repository::{
        RepositoryTransaction, RepositoryWriteOps, RepositoryWriteTx,
    };

    let mut store = MemoryStore::<RelRepoItem>::open(()).unwrap();

    let _ = Friend { id: FriendPrimaryKey(0) }; // anchor the type import
    let person = Person {
        id: PersonPrimaryKey(1),
        friends: NbVec::try_from_slice(&[
            FriendPrimaryKey(10),
            FriendPrimaryKey(20),
            FriendPrimaryKey(30),
        ])
        .unwrap(),
    };

    let mut txn = store.write_transaction().unwrap();
    txn.insert(RelRepoItem::RelDef(crate::relrepo::RelDef::Person(person)))
        .unwrap();
    RepositoryWriteTx::commit(txn).unwrap();

    // The single owning key maps to three multimap entries (one per friend).
    let txn = store.read_transaction().unwrap();
    let table = txn
        .open_read_multimap_table::<Person, PersonRelationalKeys, crate::relrepo::reldef::PersonRelationalValues>(
            "Person_Relational_friends",
        )
        .unwrap();
    const PK_MAX: usize = <FriendPrimaryKey as netabase_store::keys::ordered::OrderedKeyEncoding>::MAX_ENCODED_LEN;
    let mut friend_ids: Vec<u64> = table
        .get_all(&PersonRelationalKeys::Friends(PersonPrimaryKey(1)))
        .unwrap()
        .map(|entry| {
            let (_, guard) = entry.unwrap();
            let pk: FriendPrimaryKey = decode_rel_value::<_, PK_MAX>(&guard).unwrap();
            pk.0
        })
        .collect();
    friend_ids.sort_unstable();
    assert_eq!(friend_ids, vec![10, 20, 30], "Vec relation -> 3 multimap entries");
}
