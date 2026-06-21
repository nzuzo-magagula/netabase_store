//! A type-bound wrapper around [`bumpalo::Bump`].
//!
//! [`TypedBump<T>`] is a bump allocator that only ever allocates values of a
//! single type `T`. Binding the allocator to one type lets us drive bumpalo's
//! `MIN_ALIGN` const generic straight from the type's alignment (a faster
//! allocation fast-path, since the bump pointer stays type-aligned and bumpalo
//! skips per-allocation realignment), and lets us layer a typed index on top.
//!
//! This is a deliberately small first draft: it exists to learn how bumpalo
//! behaves and to pin down what we'll be mapping before moving on to
//! operation-specific allocators.
//!
//! Allocated types must be [`rkyv::Portable`] so the bytes are
//! network-shareable. `Portable` values are flat plain-old-data with no owning
//! resources, which is what makes it sound to never run their destructors —
//! bumpalo does not run `Drop` for bump-allocated values.

// `Bump<{ T::ALIGN }>` feeds an associated const into a const-generic position,
// which is a generic const expression — nightly only.
#![feature(generic_const_exprs)]
#![allow(incomplete_features)]

use core::marker::PhantomData;
use core::ptr::NonNull;

use bumpalo::Bump;

/// Marker trait for types storable in a [`TypedBump`].
///
/// Forces the [`rkyv::Portable`] requirement and surfaces the type's alignment
/// as [`ArenaType::ALIGN`], which drives the backing allocator's `MIN_ALIGN`.
/// Blanket-implemented for every `Portable + Sized` type, so there is no
/// boilerplate at the call site.
pub trait ArenaType: rkyv::Portable + Sized {
    /// The type's alignment, used as bumpalo's `MIN_ALIGN`.
    const ALIGN: usize = core::mem::align_of::<Self>();
}

impl<T: rkyv::Portable + Sized> ArenaType for T {}

/// A typed, insertion-ordered handle to a value allocated in a [`TypedBump`].
///
/// The type tag prevents an index from one arena's element type being used with
/// an arena of a different type. There is no generational counter in this draft,
/// so an index obtained before a [`TypedBump::reset`] must not be reused after.
pub struct Idx<T>(usize, PhantomData<fn() -> T>);

impl<T> Idx<T> {
    /// The raw insertion position this handle refers to.
    #[must_use]
    pub fn position(self) -> usize {
        self.0
    }
}

impl<T> Clone for Idx<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Idx<T> {}

impl<T> PartialEq for Idx<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<T> Eq for Idx<T> {}

impl<T> core::hash::Hash for Idx<T> {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl<T> core::fmt::Debug for Idx<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("Idx").field(&self.0).finish()
    }
}

/// A bump allocator bound to a single [`ArenaType`].
///
/// Every allocation is a `T`, so the backing [`Bump`] is configured with
/// `MIN_ALIGN == align_of::<T>()`. A side `Vec` records one pointer per
/// allocation, giving O(1) indexing by insertion order regardless of how
/// bumpalo spreads allocations across chunks.
pub struct TypedBump<T: ArenaType>
where
    [(); T::ALIGN]:,
{
    bump: Bump<{ T::ALIGN }>,
    index: Vec<NonNull<T>>,
    _t: PhantomData<T>,
}

impl<T: ArenaType> TypedBump<T>
where
    [(); T::ALIGN]:,
{
    /// Compile-time guard: bumpalo caps `MIN_ALIGN` at 16, and we defer support
    /// for anything larger. Referencing this in a constructor forces the check
    /// at monomorphization for the concrete `T`.
    const ALIGN_OK: () = assert!(T::ALIGN <= 16, "over-aligned types are unsupported");

    /// Create an empty allocator.
    #[must_use]
    pub fn new() -> Self {
        let () = Self::ALIGN_OK;
        Self {
            bump: Bump::with_min_align(),
            index: Vec::new(),
            _t: PhantomData,
        }
    }

    /// Create an allocator with at least `bytes` of preallocated capacity.
    #[must_use]
    pub fn with_capacity(bytes: usize) -> Self {
        let () = Self::ALIGN_OK;
        Self {
            bump: Bump::with_min_align_and_capacity(bytes),
            index: Vec::new(),
            _t: PhantomData,
        }
    }

    /// Allocate `val` and return a handle to it.
    pub fn alloc(&mut self, val: T) -> Idx<T> {
        // The `&mut T` borrow of `self.bump` ends once we capture the address as
        // a `NonNull`, so pushing into `self.index` does not conflict.
        let slot = NonNull::from(self.bump.alloc(val));
        let position = self.index.len();
        self.index.push(slot);
        Idx(position, PhantomData)
    }

    /// Borrow the value behind `idx`, if it is in range.
    #[must_use]
    pub fn get(&self, idx: Idx<T>) -> Option<&T> {
        // SAFETY: the pointer was produced by `self.bump.alloc`, the backing
        // memory is owned by `self.bump` and outlives this borrow, and `reset`
        // clears `index` in lockstep so no stale pointer is reachable.
        self.index.get(idx.0).map(|slot| unsafe { slot.as_ref() })
    }

    /// Mutably borrow the value behind `idx`, if it is in range.
    #[must_use]
    pub fn get_mut(&mut self, idx: Idx<T>) -> Option<&mut T> {
        // SAFETY: as `get`, and `&mut self` guarantees unique access.
        self.index
            .get_mut(idx.0)
            .map(|slot| unsafe { slot.as_mut() })
    }

    /// Discard every allocation, returning the allocator to an empty state.
    ///
    /// Does not run destructors (bumpalo never does); sound because [`ArenaType`]
    /// values are flat plain-old-data. Invalidates all previously issued indices.
    pub fn reset(&mut self) {
        self.bump.reset();
        self.index.clear();
    }

    /// Number of values currently allocated.
    #[must_use]
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// Whether no values are currently allocated.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Bytes bumpalo has handed out across all chunks. See [`Bump::allocated_bytes`].
    #[must_use]
    pub fn allocated_bytes(&self) -> usize {
        self.bump.allocated_bytes()
    }

    /// Remaining capacity in the current chunk. See [`Bump::chunk_capacity`].
    #[must_use]
    pub fn chunk_capacity(&self) -> usize {
        self.bump.chunk_capacity()
    }
}

impl<T: ArenaType> Default for TypedBump<T>
where
    [(); T::ALIGN]:,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[repr(C)]
    #[derive(Clone, Copy, Debug, PartialEq)]
    struct Point {
        x: i32,
        y: i32,
    }

    // SAFETY: `Point` is `#[repr(C)]` plain-old-data with a fixed, padding-free
    // layout, so it is sound to treat as a portable byte image.
    unsafe impl rkyv::Portable for Point {}

    #[test]
    fn alloc_and_index() {
        let mut arena: TypedBump<Point> = TypedBump::new();

        let a = arena.alloc(Point { x: 1, y: 2 });
        let b = arena.alloc(Point { x: 3, y: 4 });
        let c = arena.alloc(Point { x: 5, y: 6 });

        assert_eq!(arena.get(a), Some(&Point { x: 1, y: 2 }));
        assert_eq!(arena.get(b), Some(&Point { x: 3, y: 4 }));
        assert_eq!(arena.get(c), Some(&Point { x: 5, y: 6 }));
        assert_eq!(arena.len(), 3);
    }

    #[test]
    fn allocations_are_type_aligned() {
        let mut arena: TypedBump<Point> = TypedBump::new();
        let align = core::mem::align_of::<Point>();

        for i in 0..16 {
            let idx = arena.alloc(Point { x: i, y: -i });
            let ptr = arena.get(idx).unwrap() as *const Point;
            assert_eq!(ptr as usize % align, 0, "allocation {i} was misaligned");
        }
    }

    #[test]
    fn introspection_reflects_allocation() {
        // `new()` starts with no chunk allocated; `allocated_bytes()` reports the
        // total size of chunks taken from the OS, so it only grows once we force
        // a chunk by allocating. (Note: `with_capacity` would preallocate a chunk
        // and report nonzero immediately.)
        let mut arena: TypedBump<Point> = TypedBump::new();
        assert_eq!(arena.allocated_bytes(), 0);

        arena.alloc(Point { x: 7, y: 8 });
        assert!(arena.allocated_bytes() > 0);
        assert!(arena.chunk_capacity() > 0);
    }

    #[test]
    fn reset_clears_and_restarts_indices() {
        let mut arena: TypedBump<Point> = TypedBump::new();
        arena.alloc(Point { x: 1, y: 1 });
        arena.alloc(Point { x: 2, y: 2 });
        assert_eq!(arena.len(), 2);

        arena.reset();
        assert_eq!(arena.len(), 0);
        assert!(arena.is_empty());

        let fresh = arena.alloc(Point { x: 9, y: 9 });
        assert_eq!(fresh.position(), 0);
        assert_eq!(arena.get(fresh), Some(&Point { x: 9, y: 9 }));
    }
}
