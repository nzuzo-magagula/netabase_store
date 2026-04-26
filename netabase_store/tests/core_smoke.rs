//! Backend smoke tests over the raw table surface (no proc macros).
//!
//! Uses the trivial `NoRepository` schema and primitive key/value types to
//! prove the Phase 3 core: ordered byte tables, zero-copy guards, snapshot
//! isolation, and atomic commit/rollback on both std backends.

use netabase_store::databases::memory::MemoryStore;
use netabase_store::databases::redb::RedbStore;
use netabase_store::traits::structural::database::NetabaseStore;
use netabase_store::traits::structural::database::tables::core::{
    TableReadOps, TableWriteOps,
};
use netabase_store::traits::structural::database::transactions::repository::{
    RepositoryTransaction, RepositoryWriteTx,
};
use netabase_store::traits::structural::schema::repositories::NoRepository;

type R = NoRepository;
const TBL: &str = "smoke_plain";
const MULTI: &str = "smoke_multi";

fn tempdir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "netabase_smoke_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Shared behavioural checks over any store. Returns after commit so callers
/// can re-open / re-read.
fn write_read_cycle<DB: NetabaseStore<R>>(store: &mut DB) {
    // Stage writes.
    {
        let txn = store.write_transaction().unwrap();
        {
            let mut table = txn.open_write_table::<R, u32, u64>(TBL).unwrap();
            table.insert(&3, &30).unwrap();
            table.insert(&1, &10).unwrap();
            table.insert(&2, &20).unwrap();
            // Overwrite semantics on a plain table.
            table.insert(&2, &22).unwrap();
        }
        {
            let mut multi = txn.open_write_multimap_table::<R, u32, u64>(MULTI).unwrap();
            multi.insert(&7, &70).unwrap();
            multi.insert(&7, &71).unwrap();
            multi.insert(&7, &71).unwrap(); // duplicate pair: set semantics
        }
        txn.commit().unwrap();
    }

    // Read back through a fresh snapshot.
    let txn = store.read_transaction().unwrap();
    let table = txn.open_read_table::<R, u32, u64>(TBL).unwrap();
    assert_eq!(table.get(&1).unwrap().unwrap().to_native(), 10);
    assert_eq!(table.get(&2).unwrap().unwrap().to_native(), 22);
    assert!(table.get(&99).unwrap().is_none());

    // Ordered range scan == typed order.
    let keys: Vec<u32> = table
        .range(..)
        .unwrap()
        .map(|r| r.unwrap().0)
        .collect();
    assert_eq!(keys, vec![1, 2, 3]);
    let bounded: Vec<u32> = table
        .range(2..)
        .unwrap()
        .map(|r| r.unwrap().0)
        .collect();
    assert_eq!(bounded, vec![2, 3]);

    // Multimap: all values under one key.
    let multi = txn.open_read_multimap_table::<R, u32, u64>(MULTI).unwrap();
    let mut vals: Vec<u64> = multi
        .get_all(&7)
        .unwrap()
        .map(|r| r.unwrap().1.to_native())
        .collect();
    vals.sort_unstable();
    assert_eq!(vals, vec![70, 71]);
}

/// Dropping a write transaction without commit must discard its writes.
fn rollback_cycle<DB: NetabaseStore<R>>(store: &mut DB) {
    {
        let txn = store.write_transaction().unwrap();
        {
            let mut table = txn.open_write_table::<R, u32, u64>(TBL).unwrap();
            table.insert(&1000, &1).unwrap();
        }
        // No commit: dropped here.
    }
    let txn = store.read_transaction().unwrap();
    let table = txn.open_read_table::<R, u32, u64>(TBL).unwrap();
    assert!(
        table.get(&1000).unwrap().is_none(),
        "uncommitted write leaked into the store"
    );
}

#[test]
fn memory_store_smoke() {
    let mut store = MemoryStore::<R>::open(()).unwrap();
    write_read_cycle(&mut store);
    rollback_cycle(&mut store);
}

#[test]
fn redb_store_smoke() {
    let dir = tempdir();
    let mut store = RedbStore::<R>::open(dir.join("smoke.redb")).unwrap();
    write_read_cycle(&mut store);
    rollback_cycle(&mut store);
}

// NOTE on isolation: `write_transaction(&mut self)` means a read transaction
// (which borrows the store) can never be live across a write transaction —
// the borrow checker statically rules out reader/writer overlap on one
// handle. "Snapshot isolation" therefore reduces to commit atomicity
// (covered by `rollback_cycle`); there is no runtime overlap to test.

#[test]
fn corruption_is_an_error_not_a_panic() {
    // A value of the wrong width must surface as Codec error on read.
    let mut store = MemoryStore::<R>::open(()).unwrap();
    {
        let txn = store.write_transaction().unwrap();
        // Write a u32 value under a key, then read it back as u64: the
        // 4-byte value fails u64's 8-byte width check.
        txn.open_write_table::<R, u32, u32>(TBL)
            .unwrap()
            .insert(&1, &7)
            .unwrap();
        txn.commit().unwrap();
    }
    let txn = store.read_transaction().unwrap();
    let table = txn.open_read_table::<R, u32, u64>(TBL).unwrap();
    let err = match table.get(&1) {
        Err(e) => e,
        Ok(_) => panic!("expected a Codec error reading mis-sized value"),
    };
    assert!(
        matches!(err, netabase_store::errors::NetabaseError::Codec(_)),
        "expected Codec error, got {err:?}"
    );
}
