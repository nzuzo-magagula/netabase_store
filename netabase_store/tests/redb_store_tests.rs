//! Raw-table API over the redb backend: point ops, overwrite, range, and multimap, with both
//! integer and `NbString` keys (the order-preserving encoding makes both range-correct).
use netabase_arena::fixed::NbString;
use netabase_store::databases::redb::RedbStore;
use netabase_store::traits::structural::{
    database::{
        NetabaseStore,
        tables::{TableOwner, TableReadOps, TableWriteOps},
        transactions::repository::RepositoryWriteTx,
    },
    schema::repositories::NoRepository,
};
use std::time::{SystemTime, UNIX_EPOCH};

struct TestOwner;
impl TableOwner for TestOwner {}

type Key = NbString<16>;

fn k(s: &str) -> Key {
    NbString::try_from_str(s).unwrap()
}

fn temp_db_path(prefix: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time moved backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("{}_{}.redb", prefix, nanos))
}

#[test]
fn test_redb_store_opens_transactions() {
    let path = temp_db_path("netabase_redb_basic");
    let mut store = RedbStore::<NoRepository>::open(path.clone()).unwrap();
    {
        let _read = store.read_transaction().unwrap();
    }
    {
        let _write = store.write_transaction().unwrap();
    }
    let _ = std::fs::remove_file(path);
}

#[test]
fn test_redb_typed_key_value_table_ops() {
    let path = temp_db_path("netabase_redb_typed");
    let mut store = RedbStore::<NoRepository>::open(path.clone()).unwrap();
    let tx = store.write_transaction().unwrap();

    let mut table = tx.open_write_table::<TestOwner, u64, u64>("typed_kv").unwrap();
    table.insert(&7, &77).unwrap();
    assert_eq!(table.get_value(&7).unwrap(), Some(77));
    assert!(table.remove(&7).unwrap());
    assert!(table.get(&7).unwrap().is_none());

    let _ = std::fs::remove_file(path);
}

#[test]
fn test_redb_string_key_overwrite_and_missing() {
    let path = temp_db_path("netabase_redb_overwrite");
    let mut store = RedbStore::<NoRepository>::open(path.clone()).unwrap();
    let tx = store.write_transaction().unwrap();

    let mut table = tx.open_write_table::<TestOwner, Key, u64>("str_kv").unwrap();
    table.insert(&k("key"), &1).unwrap();
    assert_eq!(table.get_value(&k("key")).unwrap(), Some(1));

    table.insert(&k("key"), &2).unwrap(); // overwrite
    assert_eq!(table.get_value(&k("key")).unwrap(), Some(2));

    assert!(table.get(&k("missing")).unwrap().is_none());
    assert!(!table.remove(&k("missing")).unwrap());

    let _ = std::fs::remove_file(path);
}

#[test]
fn test_redb_range_is_ordered() {
    let path = temp_db_path("netabase_redb_range");
    let mut store = RedbStore::<NoRepository>::open(path.clone()).unwrap();
    let tx = store.write_transaction().unwrap();

    let mut table = tx.open_write_table::<TestOwner, u64, u64>("range_kv").unwrap();
    for i in 10..=20u64 {
        table.insert(&i, &(i * 10)).unwrap();
    }
    let got: Vec<(u64, u64)> = table
        .range(12..=15)
        .unwrap()
        .map(|r| {
            let (key, guard) = r.unwrap();
            (key, guard.to_native())
        })
        .collect();
    assert_eq!(got, vec![(12, 120), (13, 130), (14, 140), (15, 150)]);

    let _ = std::fs::remove_file(path);
}

#[test]
fn test_redb_multimap_ops() {
    let path = temp_db_path("netabase_redb_multimap");
    let mut store = RedbStore::<NoRepository>::open(path.clone()).unwrap();
    let tx = store.write_transaction().unwrap();

    let mut table = tx.open_write_multimap_table::<TestOwner, Key, u64>("multimap").unwrap();
    table.insert(&k("group1"), &1).unwrap();
    table.insert(&k("group1"), &2).unwrap();
    table.insert(&k("group2"), &3).unwrap();

    let mut g1: Vec<u64> = table
        .get_all(&k("group1"))
        .unwrap()
        .map(|r| r.unwrap().1.to_native())
        .collect();
    g1.sort_unstable();
    assert_eq!(g1, vec![1, 2]);

    let g2: Vec<u64> = table
        .get_all(&k("group2"))
        .unwrap()
        .map(|r| r.unwrap().1.to_native())
        .collect();
    assert_eq!(g2, vec![3]);

    let mut all: Vec<(String, u64)> = table
        .range(k("group1")..k("group3"))
        .unwrap()
        .map(|r| {
            let (key, guard) = r.unwrap();
            (key.as_str().to_string(), guard.to_native())
        })
        .collect();
    all.sort();
    assert_eq!(
        all,
        vec![
            ("group1".to_string(), 1),
            ("group1".to_string(), 2),
            ("group2".to_string(), 3),
        ]
    );

    let _ = std::fs::remove_file(path);
}
