//! fjall backend contract: point ops, ordered range, key-composition
//! multimaps, atomic commit, and drop-rollback — no proc macros.

#![cfg(feature = "fjall-backend")]

use netabase_store::databases::fjall::FjallStore;
use netabase_store::traits::structural::database::NetabaseStore;
use netabase_store::traits::structural::database::tables::core::{TableReadOps, TableWriteOps};
use netabase_store::traits::structural::database::transactions::repository::{
    RepositoryTransaction, RepositoryWriteTx,
};
use netabase_store::traits::structural::schema::repositories::NoRepository;

type R = NoRepository;
const TBL: &str = "smoke_plain";
const MULTI: &str = "smoke_multi";

fn tempdir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "netabase_fjall_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn fjall_write_read_cycle() {
    let dir = tempdir();
    let mut store = FjallStore::<R>::open(dir.join("db")).unwrap();

    {
        let txn = store.write_transaction().unwrap();
        {
            let mut t = txn.open_write_table::<R, u32, u64>(TBL).unwrap();
            t.insert(&3, &30).unwrap();
            t.insert(&1, &10).unwrap();
            t.insert(&2, &20).unwrap();
            t.insert(&2, &22).unwrap(); // overwrite
        }
        {
            let mut m = txn.open_write_multimap_table::<R, u32, u64>(MULTI).unwrap();
            m.insert(&7, &70).unwrap();
            m.insert(&7, &71).unwrap();
            m.insert(&7, &71).unwrap(); // duplicate pair: set semantics
        }
        txn.commit().unwrap();
    }

    let txn = store.read_transaction().unwrap();
    let t = txn.open_read_table::<R, u32, u64>(TBL).unwrap();
    assert_eq!(t.get(&1).unwrap().unwrap().to_native(), 10);
    assert_eq!(t.get(&2).unwrap().unwrap().to_native(), 22);
    assert!(t.get(&99).unwrap().is_none());

    let keys: Vec<u32> = t.range(..).unwrap().map(|r| r.unwrap().0).collect();
    assert_eq!(keys, vec![1, 2, 3]);
    let bounded: Vec<u32> = t.range(2..).unwrap().map(|r| r.unwrap().0).collect();
    assert_eq!(bounded, vec![2, 3]);

    let m = txn.open_read_multimap_table::<R, u32, u64>(MULTI).unwrap();
    let mut vals: Vec<u64> = m
        .get_all(&7)
        .unwrap()
        .map(|r| r.unwrap().1.to_native())
        .collect();
    vals.sort_unstable();
    assert_eq!(vals, vec![70, 71]);
}

#[test]
fn fjall_uncommitted_writes_invisible_then_visible() {
    let dir = tempdir();
    let mut store = FjallStore::<R>::open(dir.join("db")).unwrap();
    // Stage without commit, then drop.
    {
        let txn = store.write_transaction().unwrap();
        txn.open_write_table::<R, u32, u64>(TBL)
            .unwrap()
            .insert(&1, &10)
            .unwrap();
    }
    assert!(
        store
            .read_transaction()
            .unwrap()
            .open_read_table::<R, u32, u64>(TBL)
            .unwrap()
            .get(&1)
            .unwrap()
            .is_none(),
        "uncommitted write leaked"
    );
    // Now commit.
    {
        let txn = store.write_transaction().unwrap();
        txn.open_write_table::<R, u32, u64>(TBL)
            .unwrap()
            .insert(&1, &10)
            .unwrap();
        txn.commit().unwrap();
    }
    assert_eq!(
        store
            .read_transaction()
            .unwrap()
            .open_read_table::<R, u32, u64>(TBL)
            .unwrap()
            .get(&1)
            .unwrap()
            .unwrap()
            .to_native(),
        10
    );
}

#[test]
fn fjall_multimap_pair_removal() {
    let dir = tempdir();
    let mut store = FjallStore::<R>::open(dir.join("db")).unwrap();
    {
        let txn = store.write_transaction().unwrap();
        let mut m = txn.open_write_multimap_table::<R, u32, u64>(MULTI).unwrap();
        m.insert(&5, &50).unwrap();
        m.insert(&5, &51).unwrap();
        assert!(m.remove_value(&5, &50).unwrap());
        drop(m);
        txn.commit().unwrap();
    }
    let txn = store.read_transaction().unwrap();
    let m = txn.open_read_multimap_table::<R, u32, u64>(MULTI).unwrap();
    let vals: Vec<u64> = m.get_all(&5).unwrap().map(|r| r.unwrap().1.to_native()).collect();
    assert_eq!(vals, vec![51]);
}
