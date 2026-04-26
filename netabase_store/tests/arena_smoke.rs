//! Heap-free `ArenaStore` contract: point ops, ordered range, multimap set
//! semantics, atomic commit, drop-rollback, and capacity exhaustion — over a
//! caller-supplied `&mut [u8]` region, no proc macros.

#![cfg(feature = "arena-store")]

use netabase_store::databases::arena::ArenaStore;
use netabase_store::databases::arena::region::{COUNT_HDR, RECORD_SIZE};
use netabase_store::traits::structural::database::NetabaseStore;
use netabase_store::traits::structural::database::tables::core::{TableReadOps, TableWriteOps};
use netabase_store::traits::structural::database::transactions::repository::{
    RepositoryTransaction, RepositoryWriteTx,
};
use netabase_store::traits::structural::schema::repositories::NoRepository;

type R = NoRepository;
const TBL: &str = "smoke_plain";
const MULTI: &str = "smoke_multi";

/// A buffer holding `records` records per half (base + work).
fn buffer(records: usize) -> Vec<u8> {
    vec![0u8; 2 * (COUNT_HDR + records * RECORD_SIZE)]
}

#[test]
fn arena_store_write_read_cycle() {
    let mut buf = buffer(64);
    let mut store = ArenaStore::<R>::open(&mut buf).unwrap();

    {
        let txn = store.write_transaction().unwrap();
        {
            let mut t = txn.open_write_table::<R, u32, u64>(TBL).unwrap();
            t.insert(&3, &30).unwrap();
            t.insert(&1, &10).unwrap();
            t.insert(&2, &20).unwrap();
            t.insert(&2, &22).unwrap(); // overwrite (plain table)
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

    // Ordered range == typed order.
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
fn arena_drop_without_commit_rolls_back() {
    let mut buf = buffer(64);
    let mut store = ArenaStore::<R>::open(&mut buf).unwrap();

    // Seed and commit one record.
    {
        let txn = store.write_transaction().unwrap();
        txn.open_write_table::<R, u32, u64>(TBL)
            .unwrap()
            .insert(&1, &10)
            .unwrap();
        txn.commit().unwrap();
    }
    // Stage a second, then drop without commit.
    {
        let txn = store.write_transaction().unwrap();
        txn.open_write_table::<R, u32, u64>(TBL)
            .unwrap()
            .insert(&2, &20)
            .unwrap();
        // no commit
    }
    let txn = store.read_transaction().unwrap();
    let t = txn.open_read_table::<R, u32, u64>(TBL).unwrap();
    assert_eq!(t.get(&1).unwrap().unwrap().to_native(), 10);
    assert!(t.get(&2).unwrap().is_none(), "uncommitted write leaked");
}

#[test]
fn arena_capacity_is_reported_not_panicked() {
    // Two records per half → the third insert in one tx overflows.
    let mut buf = buffer(2);
    let mut store = ArenaStore::<R>::open(&mut buf).unwrap();
    let txn = store.write_transaction().unwrap();
    let mut t = txn.open_write_table::<R, u32, u64>(TBL).unwrap();
    t.insert(&1, &1).unwrap();
    t.insert(&2, &2).unwrap();
    let err = t.insert(&3, &3).unwrap_err();
    assert!(
        matches!(err, netabase_store::errors::NetabaseError::Capacity { .. }),
        "expected Capacity, got {err:?}"
    );
}

#[test]
fn arena_remove_and_multimap_pair_removal() {
    let mut buf = buffer(64);
    let mut store = ArenaStore::<R>::open(&mut buf).unwrap();
    {
        let txn = store.write_transaction().unwrap();
        {
            let mut t = txn.open_write_table::<R, u32, u64>(TBL).unwrap();
            t.insert(&1, &10).unwrap();
            assert!(t.remove(&1).unwrap());
            assert!(!t.remove(&1).unwrap());
        }
        {
            let mut m = txn.open_write_multimap_table::<R, u32, u64>(MULTI).unwrap();
            m.insert(&5, &50).unwrap();
            m.insert(&5, &51).unwrap();
            assert!(m.remove_value(&5, &50).unwrap());
        }
        txn.commit().unwrap();
    }
    let txn = store.read_transaction().unwrap();
    assert!(txn.open_read_table::<R, u32, u64>(TBL).unwrap().get(&1).unwrap().is_none());
    let m = txn.open_read_multimap_table::<R, u32, u64>(MULTI).unwrap();
    let vals: Vec<u64> = m.get_all(&5).unwrap().map(|r| r.unwrap().1.to_native()).collect();
    assert_eq!(vals, vec![51]);
}
