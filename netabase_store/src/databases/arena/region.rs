//! The heap-free sorted record slab backing the arena store.
//!
//! A slab is a `&mut [u8]` region laid out as a 4-byte live-count header
//! followed by `CAP` fixed-size records. Records `[0..count)` are live and
//! kept **sorted by `(name, key, value)`** (raw byte order). Because keys are
//! the order-preserving encodings, that byte order *is* the typed order, so a
//! key range is a contiguous slice and all of a table's records are
//! contiguous.
//!
//! Everything here is `core`-only (no heap): inserts shift records in place
//! with `copy_within`, lookups binary-search.

use crate::errors::{CodecErrorKind, NetabaseError, OpKind};
use core::cmp::Ordering;

/// Max table-name length (a `&'static str`). Names longer than this are a bug.
pub const NAME_MAX: usize = 64;
/// Max ordered-encoded key length stored inline.
pub const KEY_MAX: usize = 256;
/// Max canonical value length stored inline. Values larger than this (e.g.
/// very large blob fields) are rejected on the arena backend.
pub const VAL_MAX: usize = 4096;

const LEN_HDR: usize = 6; // name_len(2) + key_len(2) + val_len(2)
/// Size of one record in bytes.
pub const RECORD_SIZE: usize = LEN_HDR + NAME_MAX + KEY_MAX + VAL_MAX;
/// Size of the slab's live-count header.
pub const COUNT_HDR: usize = 4;

/// How many records a slab of `bytes` length can hold.
#[must_use]
pub const fn capacity_for(bytes: usize) -> usize {
    if bytes < COUNT_HDR {
        0
    } else {
        (bytes - COUNT_HDR) / RECORD_SIZE
    }
}

#[inline]
fn rec_off(i: usize) -> usize {
    COUNT_HDR + i * RECORD_SIZE
}

/// Read the live record count.
#[inline]
pub fn count(slab: &[u8]) -> usize {
    u32::from_le_bytes([slab[0], slab[1], slab[2], slab[3]]) as usize
}

#[inline]
fn set_count(slab: &mut [u8], n: usize) {
    slab[0..4].copy_from_slice(&(n as u32).to_le_bytes());
}

/// The name bytes of record `i`.
#[inline]
pub fn rec_name(slab: &[u8], i: usize) -> &[u8] {
    let o = rec_off(i);
    let nl = u16::from_le_bytes([slab[o], slab[o + 1]]) as usize;
    let base = o + LEN_HDR;
    &slab[base..base + nl]
}

/// The key bytes of record `i`.
#[inline]
pub fn rec_key(slab: &[u8], i: usize) -> &[u8] {
    let o = rec_off(i);
    let kl = u16::from_le_bytes([slab[o + 2], slab[o + 3]]) as usize;
    let base = o + LEN_HDR + NAME_MAX;
    &slab[base..base + kl]
}

/// The value bytes of record `i`.
#[inline]
pub fn rec_val(slab: &[u8], i: usize) -> &[u8] {
    let o = rec_off(i);
    let vl = u16::from_le_bytes([slab[o + 4], slab[o + 5]]) as usize;
    let base = o + LEN_HDR + NAME_MAX + KEY_MAX;
    &slab[base..base + vl]
}

/// Compare record `i`'s `(name, key)` against a target `(name, key)`.
#[inline]
fn cmp_name_key(slab: &[u8], i: usize, name: &[u8], key: &[u8]) -> Ordering {
    rec_name(slab, i)
        .cmp(name)
        .then_with(|| rec_key(slab, i).cmp(key))
}

/// Compare record `i`'s full `(name, key, value)` against a target.
#[inline]
fn cmp_full(slab: &[u8], i: usize, name: &[u8], key: &[u8], val: &[u8]) -> Ordering {
    cmp_name_key(slab, i, name, key).then_with(|| rec_val(slab, i).cmp(val))
}

/// First index whose `(name, key)` is `>= (name, key)`. (Lower bound over the
/// `(name, key)` prefix; values within an equal key follow.)
pub fn lower_bound(slab: &[u8], name: &[u8], key: &[u8]) -> usize {
    let mut lo = 0;
    let mut hi = count(slab);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if cmp_name_key(slab, mid, name, key) == Ordering::Less {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

/// First index whose `(name, key, value)` is `>= (name, key, value)`.
fn lower_bound_full(slab: &[u8], name: &[u8], key: &[u8], val: &[u8]) -> usize {
    let mut lo = 0;
    let mut hi = count(slab);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if cmp_full(slab, mid, name, key, val) == Ordering::Less {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

/// Whether record `i` exists and matches `(name, key)`.
#[inline]
pub fn matches_name_key(slab: &[u8], i: usize, name: &[u8], key: &[u8]) -> bool {
    i < count(slab) && cmp_name_key(slab, i, name, key) == Ordering::Equal
}

fn write_record(slab: &mut [u8], i: usize, name: &[u8], key: &[u8], val: &[u8]) {
    let o = rec_off(i);
    slab[o..o + 2].copy_from_slice(&(name.len() as u16).to_le_bytes());
    slab[o + 2..o + 4].copy_from_slice(&(key.len() as u16).to_le_bytes());
    slab[o + 4..o + 6].copy_from_slice(&(val.len() as u16).to_le_bytes());
    let nb = o + LEN_HDR;
    slab[nb..nb + name.len()].copy_from_slice(name);
    let kb = o + LEN_HDR + NAME_MAX;
    slab[kb..kb + key.len()].copy_from_slice(key);
    let vb = o + LEN_HDR + NAME_MAX + KEY_MAX;
    slab[vb..vb + val.len()].copy_from_slice(val);
}

fn check_bounds(name: &[u8], key: &[u8], val: &[u8]) -> Result<(), NetabaseError> {
    if name.len() > NAME_MAX || key.len() > KEY_MAX {
        return Err(NetabaseError::KeyCodec(
            crate::errors::KeyCodecError::BufferTooSmall,
        ));
    }
    if val.len() > VAL_MAX {
        return Err(NetabaseError::Codec(CodecErrorKind::SerializeFailed));
    }
    Ok(())
}

fn capacity_err(name: &'static str, cap: usize) -> NetabaseError {
    NetabaseError::Capacity {
        table: name,
        needed: (cap + 1) as u32,
        available: cap as u32,
    }
}

/// Insert a record at sorted position `i`, shifting `[i..count)` right.
fn insert_at(
    slab: &mut [u8],
    i: usize,
    name: &[u8],
    key: &[u8],
    val: &[u8],
    table: &'static str,
) -> Result<(), NetabaseError> {
    let n = count(slab);
    if n >= capacity_for(slab.len()) {
        return Err(capacity_err(table, capacity_for(slab.len())));
    }
    // Shift records [i..n) one slot to the right.
    if i < n {
        let from = rec_off(i);
        let to = rec_off(i + 1);
        let bytes = (n - i) * RECORD_SIZE;
        slab.copy_within(from..from + bytes, to);
    }
    write_record(slab, i, name, key, val);
    set_count(slab, n + 1);
    Ok(())
}

/// Remove record `i`, shifting `[i+1..count)` left.
fn remove_at(slab: &mut [u8], i: usize) {
    let n = count(slab);
    if i + 1 < n {
        let from = rec_off(i + 1);
        let to = rec_off(i);
        let bytes = (n - i - 1) * RECORD_SIZE;
        slab.copy_within(from..from + bytes, to);
    }
    set_count(slab, n - 1);
}

// ── Public table operations ─────────────────────────────────────────────────

/// Plain-table put: one value per `(name, key)`. Replaces any existing values.
pub fn put_unique(
    slab: &mut [u8],
    name: &[u8],
    key: &[u8],
    val: &[u8],
    table: &'static str,
) -> Result<(), NetabaseError> {
    check_bounds(name, key, val)?;
    // Remove all existing records for (name, key), then insert the new one.
    let start = lower_bound(slab, name, key);
    while matches_name_key(slab, start, name, key) {
        remove_at(slab, start);
    }
    insert_at(slab, start, name, key, val, table)
}

/// Multimap put: a distinct `(name, key, value)` record (set semantics — a
/// duplicate pair is a no-op).
pub fn put_multi(
    slab: &mut [u8],
    name: &[u8],
    key: &[u8],
    val: &[u8],
    table: &'static str,
) -> Result<(), NetabaseError> {
    check_bounds(name, key, val)?;
    let i = lower_bound_full(slab, name, key, val);
    if i < count(slab) && cmp_full(slab, i, name, key, val) == Ordering::Equal {
        return Ok(()); // duplicate pair
    }
    insert_at(slab, i, name, key, val, table)
}

/// Remove every value under `(name, key)`. Returns whether anything was removed.
pub fn remove_key(slab: &mut [u8], name: &[u8], key: &[u8]) -> bool {
    let start = lower_bound(slab, name, key);
    let mut removed = false;
    while matches_name_key(slab, start, name, key) {
        remove_at(slab, start);
        removed = true;
    }
    removed
}

/// Remove one specific `(name, key, value)` record from a multimap.
pub fn remove_pair(slab: &mut [u8], name: &[u8], key: &[u8], val: &[u8]) -> bool {
    let i = lower_bound_full(slab, name, key, val);
    if i < count(slab) && cmp_full(slab, i, name, key, val) == Ordering::Equal {
        remove_at(slab, i);
        true
    } else {
        false
    }
}

/// Index of the first record under `(name, key)`, or `None`.
pub fn find_first(slab: &[u8], name: &[u8], key: &[u8]) -> Option<usize> {
    let i = lower_bound(slab, name, key);
    matches_name_key(slab, i, name, key).then_some(i)
}

/// First index whose name is `>= name`. Used to bound full-table range scans.
pub fn lower_bound_name(slab: &[u8], name: &[u8]) -> usize {
    let mut lo = 0;
    let mut hi = count(slab);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if rec_name(slab, mid).cmp(name) == Ordering::Less {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

/// Multimap value-count under `(name, key)` (test/diagnostic helper).
pub fn count_key(slab: &[u8], name: &[u8], key: &[u8]) -> usize {
    let mut i = lower_bound(slab, name, key);
    let mut c = 0;
    while matches_name_key(slab, i, name, key) {
        c += 1;
        i += 1;
    }
    c
}

/// Initialize a fresh slab (zero live records).
pub fn init(slab: &mut [u8]) {
    set_count(slab, 0);
}

/// Surface a "value too large for the arena" / multimap-misuse error.
pub fn unsupported(op: OpKind) -> NetabaseError {
    NetabaseError::Unsupported(op)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> std::vec::Vec<u8> {
        let mut v = std::vec![0u8; COUNT_HDR + 8 * RECORD_SIZE];
        init(&mut v);
        v
    }

    #[test]
    fn unique_put_get_replace_remove() {
        let mut s = fresh();
        put_unique(&mut s, b"T", &[1], b"a", "T").unwrap();
        put_unique(&mut s, b"T", &[2], b"b", "T").unwrap();
        assert_eq!(rec_val(&s, find_first(&s, b"T", &[1]).unwrap()), b"a");
        // Replace value for key 1.
        put_unique(&mut s, b"T", &[1], b"a2", "T").unwrap();
        assert_eq!(count(&s), 2, "unique put replaces, not appends");
        assert_eq!(rec_val(&s, find_first(&s, b"T", &[1]).unwrap()), b"a2");
        assert!(remove_key(&mut s, b"T", &[1]));
        assert!(find_first(&s, b"T", &[1]).is_none());
        assert_eq!(count(&s), 1);
    }

    #[test]
    fn multi_put_set_semantics_and_order() {
        let mut s = fresh();
        put_multi(&mut s, b"M", &[1], b"y", "M").unwrap();
        put_multi(&mut s, b"M", &[1], b"x", "M").unwrap();
        put_multi(&mut s, b"M", &[1], b"x", "M").unwrap(); // dup → no-op
        assert_eq!(count_key(&s, b"M", &[1]), 2);
        // Sorted by value within the key.
        let start = lower_bound(&s, b"M", &[1]);
        assert_eq!(rec_val(&s, start), b"x");
        assert_eq!(rec_val(&s, start + 1), b"y");
        assert!(remove_pair(&mut s, b"M", &[1], b"x"));
        assert_eq!(count_key(&s, b"M", &[1]), 1);
    }

    #[test]
    fn records_globally_sorted_by_name_then_key() {
        let mut s = fresh();
        put_unique(&mut s, b"B", &[1], b"v", "B").unwrap();
        put_unique(&mut s, b"A", &[2], b"v", "A").unwrap();
        put_unique(&mut s, b"A", &[1], b"v", "A").unwrap();
        // Order: (A,1) (A,2) (B,1)
        assert_eq!((rec_name(&s, 0), rec_key(&s, 0)), (&b"A"[..], &[1u8][..]));
        assert_eq!((rec_name(&s, 1), rec_key(&s, 1)), (&b"A"[..], &[2u8][..]));
        assert_eq!((rec_name(&s, 2), rec_key(&s, 2)), (&b"B"[..], &[1u8][..]));
    }

    #[test]
    fn capacity_is_reported() {
        let mut v = std::vec![0u8; COUNT_HDR + 2 * RECORD_SIZE];
        init(&mut v);
        put_unique(&mut v, b"T", &[1], b"a", "T").unwrap();
        put_unique(&mut v, b"T", &[2], b"b", "T").unwrap();
        let err = put_unique(&mut v, b"T", &[3], b"c", "T").unwrap_err();
        assert!(matches!(err, NetabaseError::Capacity { available: 2, .. }));
    }
}
