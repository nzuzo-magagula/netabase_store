//! Differential backend contract: one generic body of assertions run against
//! every backend (memory / redb / fjall / arena). Proves behavioural parity —
//! point ops, ordered range, multimap set semantics, atomic commit, and
//! drop-rollback all agree across the volatile and persistent tiers.

use netabase_store::errors::NetabaseError;
use netabase_store::traits::structural::database::NetabaseStore;
use netabase_store::traits::structural::database::tables::core::{TableReadOps, TableWriteOps};
use netabase_store::traits::structural::database::transactions::repository::{
    RepositoryTransaction, RepositoryWriteTx,
};
use netabase_store::traits::structural::schema::repositories::NoRepository;

type R = NoRepository;
const TBL: &str = "contract_plain";
const MULTI: &str = "contract_multi";

/// The full contract, generic over any backend. Every backend must pass this
/// identically.
fn run_contract<DB: NetabaseStore<R>>(store: &mut DB) -> Result<(), NetabaseError> {
    // ── Staged writes commit atomically and are then visible ──
    {
        let txn = store.write_transaction()?;
        {
            let mut t = txn.open_write_table::<R, u32, u64>(TBL)?;
            t.insert(&3, &30)?;
            t.insert(&1, &10)?;
            t.insert(&2, &20)?;
            t.insert(&2, &22)?; // overwrite — plain table keeps one value
        }
        {
            let mut m = txn.open_write_multimap_table::<R, u32, u64>(MULTI)?;
            m.insert(&7, &70)?;
            m.insert(&7, &71)?;
            m.insert(&7, &71)?; // duplicate pair → set semantics
            m.insert(&8, &80)?;
        }
        txn.commit()?;
    }

    {
        let txn = store.read_transaction()?;
        let t = txn.open_read_table::<R, u32, u64>(TBL)?;
        assert_eq!(t.get(&1)?.unwrap().to_native(), 10, "point get");
        assert_eq!(t.get(&2)?.unwrap().to_native(), 22, "overwrite kept latest");
        assert!(t.get(&99)?.is_none(), "absent key");

        // Range order equals typed Ord on every backend.
        let all: Vec<u32> = t.range(..)?.map(|r| r.unwrap().0).collect();
        assert_eq!(all, vec![1, 2, 3], "full range ordered");
        let bounded: Vec<u32> = t.range(2..)?.map(|r| r.unwrap().0).collect();
        assert_eq!(bounded, vec![2, 3], "bounded range");
        let excl: Vec<u32> = t.range(1..3)?.map(|r| r.unwrap().0).collect();
        assert_eq!(excl, vec![1, 2], "half-open range");

        let m = txn.open_read_multimap_table::<R, u32, u64>(MULTI)?;
        let mut v7: Vec<u64> = m.get_all(&7)?.map(|r| r.unwrap().1.to_native()).collect();
        v7.sort_unstable();
        assert_eq!(v7, vec![70, 71], "multimap set semantics");
        let v8: Vec<u64> = m.get_all(&8)?.map(|r| r.unwrap().1.to_native()).collect();
        assert_eq!(v8, vec![80], "multimap single");
        assert!(m.get_all(&99)?.next().is_none(), "absent multimap key");
    }

    // ── Multimap pair removal removes only the targeted pair ──
    {
        let txn = store.write_transaction()?;
        {
            let mut m = txn.open_write_multimap_table::<R, u32, u64>(MULTI)?;
            assert!(m.remove_value(&7, &70)?, "pair existed");
            assert!(!m.remove_value(&7, &999)?, "absent pair");
        }
        txn.commit()?;
    }
    {
        let txn = store.read_transaction()?;
        let m = txn.open_read_multimap_table::<R, u32, u64>(MULTI)?;
        let v7: Vec<u64> = m.get_all(&7)?.map(|r| r.unwrap().1.to_native()).collect();
        assert_eq!(v7, vec![71], "only targeted pair removed");
    }

    // ── Drop-without-commit rolls back ──
    {
        let txn = store.write_transaction()?;
        txn.open_write_table::<R, u32, u64>(TBL)?.insert(&1000, &1)?;
        // dropped, not committed
    }
    {
        let txn = store.read_transaction()?;
        let t = txn.open_read_table::<R, u32, u64>(TBL)?;
        assert!(t.get(&1000)?.is_none(), "uncommitted write rolled back");
        assert_eq!(t.get(&1)?.unwrap().to_native(), 10, "prior state intact");
    }

    // ── Key removal ──
    {
        let txn = store.write_transaction()?;
        {
            let mut t = txn.open_write_table::<R, u32, u64>(TBL)?;
            assert!(t.remove(&3)?, "removed existing");
            assert!(!t.remove(&3)?, "second remove is a no-op");
        }
        txn.commit()?;
    }
    {
        let txn = store.read_transaction()?;
        let t = txn.open_read_table::<R, u32, u64>(TBL)?;
        assert!(t.get(&3)?.is_none(), "key gone after commit");
        let all: Vec<u32> = t.range(..)?.map(|r| r.unwrap().0).collect();
        assert_eq!(all, vec![1, 2], "remaining keys ordered");
    }

    Ok(())
}

fn tempdir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "netabase_contract_{tag}_{}_{}",
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
fn memory_backend_contract() {
    use netabase_store::databases::memory::MemoryStore;
    let mut store = MemoryStore::<R>::open(()).unwrap();
    run_contract(&mut store).unwrap();
}

#[cfg(feature = "redb-backend")]
#[test]
fn redb_backend_contract() {
    use netabase_store::databases::redb::RedbStore;
    let dir = tempdir("redb");
    let mut store = RedbStore::<R>::open(dir.join("db.redb")).unwrap();
    run_contract(&mut store).unwrap();
}

#[cfg(feature = "fjall-backend")]
#[test]
fn fjall_backend_contract() {
    use netabase_store::databases::fjall::FjallStore;
    let dir = tempdir("fjall");
    let mut store = FjallStore::<R>::open(dir.join("db")).unwrap();
    run_contract(&mut store).unwrap();
}

#[cfg(feature = "arena-store")]
#[test]
fn arena_backend_contract() {
    use netabase_store::databases::arena::ArenaStore;
    use netabase_store::databases::arena::region::{COUNT_HDR, RECORD_SIZE};
    let mut buf = vec![0u8; 2 * (COUNT_HDR + 64 * RECORD_SIZE)];
    let mut store = ArenaStore::<R>::open(&mut buf).unwrap();
    run_contract(&mut store).unwrap();
}
