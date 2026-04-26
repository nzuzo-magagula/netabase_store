//! Validates the full model→definition→repository macro chain compiles and
//! round-trips a value through the memory backend with the new rkyv codec.

use netabase_arena::fixed::NbString;
use netabase_macros::{NetabaseModel, netabase_definition, netabase_repository};
use netabase_store::traits::structural::database::NetabaseStore;
use netabase_store::traits::structural::database::transactions::repository::{
    RepositoryReadOps, RepositoryWriteOps, RepositoryWriteTx,
};
use rkyv::{Archive, Deserialize, Serialize};

#[netabase_repository(SmokeRepo)]
pub mod repositories {
    use super::*;

    #[netabase_definition(SmokeDef, repository(SmokeRepo))]
    pub mod definition {
        use super::*;

        #[derive(
            Archive, Serialize, Deserialize, NetabaseModel,
            Clone, Debug, PartialEq, Eq, PartialOrd, Ord,
        )]
        #[rkyv(derive(Debug))]
        #[netabase(definition(SmokeDef), capacity = 64)]
        pub struct Account {
            #[netabase(PrimaryKey)]
            pub id: u64,
            pub name: NbString<24>,
        }
    }
}

use repositories::definition::{Account, AccountPrimaryKey};

fn round_trip<DB: NetabaseStore<SmokeRepoItem>>(store: &mut DB) {
    let account = Account {
        id: 7,
        name: NbString::try_from_str("ada").unwrap(),
    };
    let item = SmokeRepoItem::SmokeDef(repositories::SmokeDef::Account(account.clone()));

    {
        let mut txn = store.write_transaction().unwrap();
        txn.insert(item).unwrap();
        txn.commit().unwrap();
    }

    let txn = store.read_transaction().unwrap();
    let key = SmokeRepoPrimaryKey::SmokeDef(repositories::SmokeDefPrimaryKey::Account(AccountPrimaryKey(7)));
    let got = txn
        .get(SmokeRepoAddress::SmokeDef(repositories::SmokeDefAddress::Account), key)
        .unwrap();
    assert!(got.is_some(), "value should round-trip");
}

#[test]
fn memory_chain_round_trips() {
    let mut store = netabase_store::databases::memory::MemoryStore::<SmokeRepoItem>::open(()).unwrap();
    round_trip(&mut store);
}

#[test]
fn redb_chain_round_trips() {
    let dir = std::env::temp_dir().join(format!("nb_chain_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut store =
        netabase_store::databases::redb::RedbStore::<SmokeRepoItem>::open(dir.join("chain.redb"))
            .unwrap();
    round_trip(&mut store);
}
