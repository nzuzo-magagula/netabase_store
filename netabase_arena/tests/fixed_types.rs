//! Behavioural tests for `netabase_arena::fixed`: canonical byte forms,
//! the order law (native order == archived order), capacity errors,
//! validated access, and TypedArena integration.

use core::mem::{align_of, size_of};
use netabase_arena::fixed::{ArchivedNbString, CapacityError, NbMap, NbOption, NbString, NbVec};
use netabase_arena::TypedArena;
use rkyv::rancor::Error as RancorError;

type S16 = NbString<16>;
type V4 = NbVec<u32, 4>;
type M4 = NbMap<NbString<8>, u64, 4>;

fn to_bytes<T>(value: &T) -> Vec<u8>
where
    T: for<'a> rkyv::Serialize<
        rkyv::api::high::HighSerializer<
            rkyv::util::AlignedVec,
            rkyv::ser::allocator::ArenaHandle<'a>,
            RancorError,
        >,
    >,
{
    rkyv::to_bytes::<RancorError>(value).unwrap().to_vec()
}

// ── Layout: fixed width, align 1, zero padding ───────────────────────────────

#[test]
fn archived_forms_are_fixed_width_align_1() {
    assert_eq!(size_of::<<S16 as rkyv::Archive>::Archived>(), 2 + 16);
    assert_eq!(align_of::<<S16 as rkyv::Archive>::Archived>(), 1);
    assert_eq!(size_of::<<V4 as rkyv::Archive>::Archived>(), 2 + 4 * 4);
    assert_eq!(align_of::<<V4 as rkyv::Archive>::Archived>(), 1);
    assert_eq!(
        size_of::<<M4 as rkyv::Archive>::Archived>(),
        2 + 4 * ((2 + 8) + 8) // len + 4 * (key + value)
    );
    assert_eq!(align_of::<<M4 as rkyv::Archive>::Archived>(), 1);
}

// ── Canonical form: equal values are byte-identical ─────────────────────────

#[test]
fn equal_strings_have_identical_bytes_after_mutation_history() {
    let a = S16::try_from_str("hello").unwrap();
    // Same logical value via a different mutation history.
    let mut b = S16::try_from_str("helloXYZ").unwrap();
    b.clear();
    b.try_push_str("hello").unwrap();
    assert_eq!(a, b);
    assert_eq!(to_bytes(&a), to_bytes(&b), "canonical tail violated");
}

#[test]
fn equal_vecs_have_identical_bytes_after_mutation_history() {
    let mut a = V4::new();
    a.try_push(7).unwrap();
    let mut b = V4::new();
    b.try_push(7).unwrap();
    b.try_push(999).unwrap();
    b.pop();
    assert_eq!(a, b);
    assert_eq!(to_bytes(&a), to_bytes(&b), "canonical tail violated");
}

#[test]
fn equal_maps_have_identical_bytes_regardless_of_insert_order() {
    let mut a = M4::new();
    a.try_insert("k1".try_into().unwrap(), 1).unwrap();
    a.try_insert("k2".try_into().unwrap(), 2).unwrap();
    let mut b = M4::new();
    b.try_insert("k2".try_into().unwrap(), 2).unwrap();
    b.try_insert("k1".try_into().unwrap(), 1).unwrap();
    assert_eq!(a, b);
    assert_eq!(to_bytes(&a), to_bytes(&b));
}

// ── Order law: native cmp == archived cmp == encoded-bytes? (typed only) ────

#[test]
fn string_order_law() {
    let cases = ["", "a", "ab", "b", "ba", "zzz", "Z", "0", "hello world!"];
    for x in cases {
        for y in cases {
            let nx = S16::try_from_str(x).unwrap();
            let ny = S16::try_from_str(y).unwrap();
            let bx = to_bytes(&nx);
            let by = to_bytes(&ny);
            let ax = rkyv::access::<ArchivedNbString<16>, RancorError>(&bx).unwrap();
            let ay = rkyv::access::<ArchivedNbString<16>, RancorError>(&by).unwrap();
            assert_eq!(nx.cmp(&ny), ax.cmp(ay), "order law broken for {x:?} vs {y:?}");
        }
    }
}

#[test]
fn vec_order_law() {
    let cases: &[&[u32]] = &[&[], &[1], &[1, 2], &[2], &[1, 2, 3, 4], &[u32::MAX]];
    for x in cases {
        for y in cases {
            let nx = V4::try_from_slice(x).unwrap();
            let ny = V4::try_from_slice(y).unwrap();
            let bx = to_bytes(&nx);
            let by = to_bytes(&ny);
            let ax = rkyv::access::<<V4 as rkyv::Archive>::Archived, RancorError>(&bx).unwrap();
            let ay = rkyv::access::<<V4 as rkyv::Archive>::Archived, RancorError>(&by).unwrap();
            assert_eq!(nx.cmp(&ny), ax.cmp(ay), "order law broken for {x:?} vs {y:?}");
        }
    }
}

// ── Round-trips ──────────────────────────────────────────────────────────────

#[test]
fn string_roundtrip_through_validated_access() {
    let original = S16::try_from_str("καλημέρα").unwrap(); // multibyte UTF-8
    let bytes = to_bytes(&original);
    let archived = rkyv::access::<ArchivedNbString<16>, RancorError>(&bytes).unwrap();
    assert_eq!(archived.as_str(), original.as_str());
    let back: S16 = rkyv::deserialize::<S16, RancorError>(archived).unwrap();
    assert_eq!(back, original);
}

#[test]
fn vec_roundtrip_through_validated_access() {
    let original = V4::try_from_slice(&[10, 20, 30]).unwrap();
    let bytes = to_bytes(&original);
    let archived = rkyv::access::<<V4 as rkyv::Archive>::Archived, RancorError>(&bytes).unwrap();
    assert_eq!(archived.len(), 3);
    assert_eq!(archived.as_slice()[1].to_native(), 20);
    let back: V4 = rkyv::deserialize::<V4, RancorError>(archived).unwrap();
    assert_eq!(back, original);
}

#[test]
fn map_roundtrip_and_archived_lookup() {
    let mut original = M4::new();
    original.try_insert("apple".try_into().unwrap(), 1).unwrap();
    original.try_insert("zebra".try_into().unwrap(), 26).unwrap();
    original.try_insert("mango".try_into().unwrap(), 13).unwrap();

    let bytes = to_bytes(&original);
    let archived =
        rkyv::access::<<M4 as rkyv::Archive>::Archived, RancorError>(&bytes).unwrap();
    assert_eq!(archived.len(), 3);
    // Keys iterate in order.
    let keys: Vec<&str> = archived.iter_entries().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["apple", "mango", "zebra"]);
    // Binary search through the comparator API.
    let v = archived.get_with(|k| k.as_str().cmp("mango")).unwrap();
    assert_eq!(v.to_native(), 13);
    assert!(archived.get_with(|k| k.as_str().cmp("nope")).is_none());

    let back: M4 = rkyv::deserialize::<M4, RancorError>(archived).unwrap();
    assert_eq!(back, original);
}

// ── NbOption: canonical absent form ─────────────────────────────────────────

#[test]
fn option_roundtrip_and_canonical_bytes() {
    type O = NbOption<u32>;
    assert_eq!(size_of::<<O as rkyv::Archive>::Archived>(), 1 + 4);
    assert_eq!(align_of::<<O as rkyv::Archive>::Archived>(), 1);

    let some = O::some(7);
    let none = O::none();
    let some_bytes = to_bytes(&some);
    let none_bytes = to_bytes(&none);
    assert_eq!(some_bytes, vec![1, 7, 0, 0, 0]);
    assert_eq!(none_bytes, vec![0, 0, 0, 0, 0]); // canonical: default payload

    let a = rkyv::access::<<O as rkyv::Archive>::Archived, RancorError>(&some_bytes).unwrap();
    assert_eq!(a.as_option().unwrap().to_native(), 7);
    let back: O = rkyv::deserialize::<O, RancorError>(a).unwrap();
    assert_eq!(back, some);

    // None < Some, matching core::Option's order.
    assert!(none < some);

    // A garbage tag is rejected, never trusted.
    let mut corrupt = some_bytes.clone();
    corrupt[0] = 7;
    assert!(rkyv::access::<<O as rkyv::Archive>::Archived, RancorError>(&corrupt).is_err());
}

// ── Capacity errors ──────────────────────────────────────────────────────────

#[test]
fn string_capacity_error() {
    assert_eq!(
        NbString::<4>::try_from_str("hello"),
        Err(CapacityError {
            capacity: 4,
            needed: 5
        })
    );
    // Failed push leaves the value unchanged.
    let mut s = NbString::<4>::try_from_str("hi").unwrap();
    assert!(s.try_push_str("xyz").is_err());
    assert_eq!(s, "hi");
}

#[test]
fn vec_capacity_error() {
    let mut v = NbVec::<u32, 2>::new();
    v.try_push(1).unwrap();
    v.try_push(2).unwrap();
    assert_eq!(
        v.try_push(3),
        Err(CapacityError {
            capacity: 2,
            needed: 3
        })
    );
    assert_eq!(v.as_slice(), &[1, 2]);
}

#[test]
fn map_capacity_error_and_replace_semantics() {
    let mut m = NbMap::<NbString<8>, u64, 2>::new();
    assert_eq!(m.try_insert("a".try_into().unwrap(), 1).unwrap(), None);
    assert_eq!(m.try_insert("b".try_into().unwrap(), 2).unwrap(), None);
    // Replacement of an existing key is not a capacity event.
    assert_eq!(m.try_insert("a".try_into().unwrap(), 10).unwrap(), Some(1));
    // A new key is.
    assert!(m.try_insert("c".try_into().unwrap(), 3).is_err());
    assert_eq!(m.get(&"a".try_into().unwrap()), Some(&10));
}

// ── Validated access rejects corruption ──────────────────────────────────────

#[test]
fn corrupt_len_is_rejected_not_panicking() {
    let bytes = to_bytes(&S16::try_from_str("hi").unwrap());
    let mut corrupt = bytes.clone();
    corrupt[0] = 0xFF; // len = 0xFF02 > 16
    assert!(rkyv::access::<ArchivedNbString<16>, RancorError>(&corrupt).is_err());
}

#[test]
fn corrupt_utf8_is_rejected() {
    let bytes = to_bytes(&S16::try_from_str("hi").unwrap());
    let mut corrupt = bytes.clone();
    corrupt[2] = 0xC0; // invalid UTF-8 leading byte within len
    assert!(rkyv::access::<ArchivedNbString<16>, RancorError>(&corrupt).is_err());
}

#[test]
fn non_canonical_tail_is_rejected() {
    let bytes = to_bytes(&S16::try_from_str("hi").unwrap());
    let mut corrupt = bytes.clone();
    *corrupt.last_mut().unwrap() = 7; // dirty byte beyond len
    assert!(rkyv::access::<ArchivedNbString<16>, RancorError>(&corrupt).is_err());
}

// ── TypedArena integration: Nb* types satisfy the tightened ArenaType ───────

#[test]
fn nbstring_lives_in_typed_arena() {
    let mut buf = [0u8; 4 * (2 + 16)];
    let mut arena = TypedArena::<S16, 4>::new(&mut buf).unwrap();
    let idx = arena.alloc(S16::try_from_str("slot zero").unwrap()).unwrap();
    assert_eq!(arena.get(idx).unwrap().as_str(), "slot zero");
    arena
        .overwrite(idx, S16::try_from_str("rewritten").unwrap())
        .unwrap();
    assert_eq!(arena.get(idx).unwrap().as_str(), "rewritten");
}

#[test]
fn nbvec_lives_in_typed_arena() {
    let mut buf = [0u8; 4 * (2 + 16)];
    let mut arena = TypedArena::<V4, 4>::new(&mut buf).unwrap();
    let v = V4::try_from_slice(&[1, 2, 3]).unwrap();
    let idx = arena.alloc(v).unwrap();
    let archived = arena.get(idx).unwrap();
    assert_eq!(archived.len(), 3);
    assert_eq!(archived.as_slice()[2].to_native(), 3);
}

#[test]
fn nbmap_lives_in_typed_arena() {
    const SLOT: usize = 2 + 4 * ((2 + 8) + 8);
    let mut buf = [0u8; 2 * SLOT];
    let mut arena = TypedArena::<M4, 2>::new(&mut buf).unwrap();
    let mut m = M4::new();
    m.try_insert("k".try_into().unwrap(), 42).unwrap();
    let idx = arena.alloc(m).unwrap();
    let archived = arena.get(idx).unwrap();
    assert_eq!(
        archived
            .get_with(|k| k.as_str().cmp("k"))
            .unwrap()
            .to_native(),
        42
    );
}
