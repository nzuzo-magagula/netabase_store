#![cfg_attr(feature = "const-index", feature(generic_const_exprs))]
#![no_std]
// Every operation inside an `unsafe fn` must be in an explicit `unsafe` block,
// so each unsafe step is individually justified rather than blanket-licensed.
#![deny(unsafe_op_in_unsafe_fn)]

pub mod fixed;

use core::sync::atomic::{AtomicU32, Ordering};
use core::{fmt, hash, marker::PhantomData, mem, mem::MaybeUninit, ptr::NonNull};
use rkyv::api::low::{LowSerializer, to_bytes_in_with_alloc};
use rkyv::ser::{allocator::SubAllocator, writer::Buffer};
use rkyv::traits::NoUndef;
use rkyv::{Archive, Portable, Serialize};

// ── Error type ───────────────────────────────────────────────────────────────

/// Errors that can arise when interacting with a [`TypedArena`].
#[non_exhaustive]
#[derive(Debug, PartialEq, Eq)]
pub enum ArenaError {
    /// No more slots are available; the arena is at capacity.
    Full,
    /// The requested index is beyond the currently allocated range.
    OutOfRange { idx: usize, len: usize },
    /// The backing memory is misaligned for `T::Archived`.
    Misaligned,
    /// A size or offset computation overflowed `usize`.
    Overflow,
    /// The backing buffer is smaller than the requested capacity requires.
    TooSmall { needed: usize, got: usize },
    /// A slot pointer falls outside the arena's backing buffer.
    OutOfBounds,
    /// Serialization of the value into its archived form failed.
    SerializationFailed,
    /// The token was minted before the most recent [`TypedArena::reset`]; the
    /// slot it referred to no longer holds that value.
    StaleToken,
    /// The token was minted by a different arena than the one it was used on.
    ForeignArena,
}

impl fmt::Display for ArenaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Full => write!(f, "arena is at capacity"),
            Self::OutOfRange { idx, len } => {
                write!(f, "slot index {idx} is out of allocated range (len={len})")
            }
            Self::Misaligned => write!(f, "backing memory is misaligned"),
            Self::Overflow => write!(f, "size arithmetic overflowed usize"),
            Self::TooSmall { needed, got } => {
                write!(f, "buffer too small: needed {needed}B, got {got}B")
            }
            Self::OutOfBounds => write!(f, "slot pointer lies outside arena bounds"),
            Self::SerializationFailed => {
                write!(f, "serialization of value into archived form failed")
            }
            Self::StaleToken => {
                write!(f, "slot token predates the arena's most recent reset")
            }
            Self::ForeignArena => {
                write!(f, "slot token was minted by a different arena")
            }
        }
    }
}

// ── ArenaSerializer ───────────────────────────────────────────────────────────

/// The no-alloc serializer used by [`TypedArena::alloc`].
pub type ArenaSerializer<'ser, 'alloc> =
    LowSerializer<Buffer<'ser>, SubAllocator<'alloc>, rkyv::rancor::Error>;

// ── ArenaType trait ──────────────────────────────────────────────────────────

/// A type that can be stored in a [`TypedArena`].
///
/// `Self::Archived: NoUndef` is required because the arena exposes byte views
/// of live slots (lifecycle policies, in-place overwrite). `Portable` alone
/// does not rule out uninitialized padding bytes; `NoUndef` does.
///
/// The bound is written as an associated-type bound in supertrait position so
/// it is elaborated at every `T: ArenaType` use site — generic code never has
/// to restate it.
#[cfg(not(feature = "pod"))]
pub trait ArenaType:
    Archive<Archived: Portable + NoUndef + Sized>
    + for<'ser, 'alloc> Serialize<ArenaSerializer<'ser, 'alloc>>
{
    const SLOT_SIZE: usize = mem::size_of::<Self::Archived>();
    const SLOT_ALIGN: usize = mem::align_of::<Self::Archived>();
}

#[cfg(not(feature = "pod"))]
impl<T> ArenaType for T where
    T: Archive<Archived: Portable + NoUndef + Sized>
        + for<'ser, 'alloc> Serialize<ArenaSerializer<'ser, 'alloc>>
{
}

/// A type that can be stored in a [`TypedArena`] (pod feature: requires `bytemuck::Pod`,
/// a strict superset of the `NoUndef` guarantee).
#[cfg(feature = "pod")]
pub trait ArenaType:
    Archive<Archived: Portable + NoUndef + Sized + bytemuck::Pod + bytemuck::Zeroable>
    + for<'ser, 'alloc> Serialize<ArenaSerializer<'ser, 'alloc>>
{
    const SLOT_SIZE: usize = mem::size_of::<Self::Archived>();
    const SLOT_ALIGN: usize = mem::align_of::<Self::Archived>();
}

#[cfg(feature = "pod")]
impl<T> ArenaType for T where
    T: Archive<Archived: Portable + NoUndef + Sized + bytemuck::Pod + bytemuck::Zeroable>
        + for<'ser, 'alloc> Serialize<ArenaSerializer<'ser, 'alloc>>
{
}

// ── AsBytesMut ───────────────────────────────────────────────────────────────

/// Provides a mutable byte view over `Self` for use with rkyv's [`Buffer`] writer.
///
/// # Safety
/// Every byte of `Self` must be part of a valid, initialized value — no uninit padding.
pub(crate) unsafe trait AsBytesMut: Sized {
    fn as_bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: guaranteed by the trait's safety contract above.
        unsafe {
            core::slice::from_raw_parts_mut(self as *mut Self as *mut u8, mem::size_of::<Self>())
        }
    }
}

// SAFETY: NoUndef is rkyv's guarantee that every byte of the value is always
// well-defined — no padding, no uninitialized bytes — even after typed writes
// through `&mut Self`. (Portable alone guarantees only stable layout.)
unsafe impl<T: Portable + NoUndef + Sized> AsBytesMut for T {}

// ── Arena lifecycle policies ──────────────────────────────────────────────────

/// Called by [`TypedArena::reset`] over the currently-allocated byte region.
///
/// # Safety
/// Must not write beyond the supplied slice, cause UB, or violate aliasing invariants.
pub unsafe trait ResetPolicy {
    fn on_reset(bytes: &mut [u8]);
}

/// Called by `Drop::drop` over the currently-allocated byte region.
///
/// # Safety
/// Same requirements as [`ResetPolicy`].
pub unsafe trait DropPolicy {
    fn on_drop(bytes: &mut [u8]);
}

/// Pairs a [`ResetPolicy`] with a [`DropPolicy`] to form a complete arena configuration.
///
/// # Safety
/// The associated policies must satisfy their own safety requirements.
pub unsafe trait ArenaConfig {
    type ResetPolicy: ResetPolicy;
    type DropPolicy: DropPolicy;
}

// ── Stock policy ZSTs ─────────────────────────────────────────────────────────

/// Policy: take no action. Both hooks are no-ops the compiler eliminates entirely.
pub struct Retain;

// SAFETY: No-op; never accesses memory.
unsafe impl ResetPolicy for Retain {
    fn on_reset(_: &mut [u8]) {}
}
unsafe impl DropPolicy for Retain {
    fn on_drop(_: &mut [u8]) {}
}

/// Policy: fill the allocated region with zeros.
pub struct Zeroize;

// SAFETY: `fill(0)` operates only within the supplied slice bounds.
unsafe impl ResetPolicy for Zeroize {
    fn on_reset(b: &mut [u8]) {
        b.fill(0);
    }
}
unsafe impl DropPolicy for Zeroize {
    fn on_drop(b: &mut [u8]) {
        b.fill(0);
    }
}

// ── Stock arena configurations ────────────────────────────────────────────────

/// No-op reset and drop hooks. Zero overhead; suitable for general use.
pub struct DefaultConfig;

// SAFETY: Both constituent policies are safe.
unsafe impl ArenaConfig for DefaultConfig {
    type ResetPolicy = Retain;
    type DropPolicy = Retain;
}

/// Zeroing reset and drop hooks. Clears allocated bytes on both `reset()` and scope-exit.
pub struct SecureConfig;

// SAFETY: Both constituent policies are safe.
unsafe impl ArenaConfig for SecureConfig {
    type ResetPolicy = Zeroize;
    type DropPolicy = Zeroize;
}

// ── Proof value types ─────────────────────────────────────────────────────────

/// **Compile-time proof** that slot `N` is within an arena's capacity.
/// Requires `feature = "const-index"` (nightly).
///
/// The `'arena` lifetime binds this token to the arena's backing buffer,
/// preventing it from outliving the arena that minted it.
#[derive(Debug, Clone, Copy)]
pub struct SlotIdx<'arena, const N: usize>(PhantomData<&'arena ()>);

/// **Runtime proof** that a slot index was validated at the time of creation.
///
/// Minted only by `checked_idx` and `alloc` on an arena. Every accessor
/// re-validates the token on use, in this order:
///
/// 1. **Brand** — each arena (including every scope) mints a unique brand at
///    construction. A token used on any arena other than the one that minted
///    it fails with [`ArenaError::ForeignArena`].
/// 2. **Generation** — every [`TypedArena::reset`] bumps the arena's
///    generation, so tokens minted before the reset fail with
///    [`ArenaError::StaleToken`] instead of silently aliasing the slot's new
///    occupant.
/// 3. **Range** — the index must be below the currently allocated length, or
///    the accessor fails with [`ArenaError::OutOfRange`].
///
/// The `'arena` lifetime additionally binds the token to the arena's backing
/// buffer so it cannot be stored past the arena's destruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DynSlotIdx<'arena> {
    pub(crate) index: u32,
    pub(crate) generation: u32,
    pub(crate) brand: u32,
    _marker: PhantomData<&'arena ()>,
}

/// Mint a process-unique arena brand.
///
/// Brands distinguish arenas from one another at token-validation time. The
/// counter starts at 1 (0 is reserved as "never a valid brand") and wrapping
/// would require 2^32 - 1 arena constructions within one process lifetime.
fn mint_brand() -> u32 {
    static BRAND_COUNTER: AtomicU32 = AtomicU32::new(1);
    BRAND_COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Proof that a byte offset was produced by [`ArenaGeometry::slot_offset`]
/// from a valid index without arithmetic overflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SlotOffset(pub(crate) usize);

/// Proof that a byte count was produced by [`ArenaGeometry::span`] or
/// [`ArenaGeometry::live_bytes`] without arithmetic overflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ByteSpan(pub(crate) usize);

/// The start address of an arena buffer as a typed raw pointer.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BufferPtr(pub(crate) *const u8);

impl BufferPtr {
    #[must_use]
    pub(crate) fn as_raw(self) -> *const u8 {
        self.0
    }
}

// ── Proof-token ergonomics ────────────────────────────────────────────────────

impl<'arena> DynSlotIdx<'arena> {
    /// Construct from a pre-validated index with the minting arena's identity.
    /// Index is checked in debug builds; zero-cost in release.
    pub(crate) fn from_trusted(index: usize, capacity: usize, brand: u32, generation: u32) -> Self {
        debug_assert!(
            index < capacity,
            "DynSlotIdx::from_trusted: index={index} is not < capacity={capacity}"
        );
        Self {
            index: index as u32,
            generation,
            brand,
            _marker: PhantomData,
        }
    }

    /// The raw slot index this token carries.
    #[must_use]
    pub fn index(&self) -> usize {
        self.index as usize
    }
}

impl<'arena> fmt::Display for DynSlotIdx<'arena> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "idx:{}", self.index)
    }
}
impl<'arena> From<DynSlotIdx<'arena>> for usize {
    fn from(v: DynSlotIdx<'arena>) -> usize {
        v.index as usize
    }
}
impl<'arena> PartialEq<usize> for DynSlotIdx<'arena> {
    fn eq(&self, n: &usize) -> bool {
        self.index as usize == *n
    }
}

impl fmt::Display for SlotOffset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "offset:{}B", self.0)
    }
}
impl From<SlotOffset> for usize {
    fn from(v: SlotOffset) -> usize {
        v.0
    }
}

impl fmt::Display for ByteSpan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "span:{}B", self.0)
    }
}
impl From<ByteSpan> for usize {
    fn from(v: ByteSpan) -> usize {
        v.0
    }
}

impl<'arena, const N: usize> fmt::Display for SlotIdx<'arena, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "const_idx:{N}")
    }
}
impl<'arena, const N: usize> From<SlotIdx<'arena, N>> for usize {
    fn from(_: SlotIdx<'arena, N>) -> usize {
        N
    }
}

impl PartialEq for BufferPtr {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl Eq for BufferPtr {}
impl fmt::Display for BufferPtr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BufferPtr({:p})", self.0)
    }
}

// ── Compile-time index gate ───────────────────────────────────────────────────

/// Helper for const-generic boolean where-clause bounds.
pub struct ConstCheck<const B: bool>;

/// Marker trait implemented only for `ConstCheck<true>`.
pub trait IsTrue {}
impl IsTrue for ConstCheck<true> {}

// ── ArenaGeometry trait ───────────────────────────────────────────────────────

/// A byte range that understands `T`'s slot geometry.
pub(crate) trait ArenaGeometry<T: ArenaType> {
    fn start_ptr(&self) -> BufferPtr;
    fn byte_len(&self) -> usize;

    /// Byte offset to slot `idx` (checked for overflow).
    #[allow(dead_code)]
    fn slot_offset(&self, idx: usize) -> Result<SlotOffset, ArenaError> {
        idx.checked_mul(T::SLOT_SIZE)
            .map(SlotOffset)
            .ok_or(ArenaError::Overflow)
    }

    /// Byte offset to slot N (compile-time constant, no runtime multiply).
    #[cfg(feature = "const-index")]
    #[must_use]
    fn slot_offset_const<const N: usize>(&self) -> SlotOffset {
        SlotOffset(N * T::SLOT_SIZE)
    }

    /// Total bytes for `n` slots (checked for overflow).
    #[allow(dead_code)]
    fn span(&self, n: usize) -> Result<ByteSpan, ArenaError> {
        n.checked_mul(T::SLOT_SIZE)
            .map(ByteSpan)
            .ok_or(ArenaError::Overflow)
    }

    /// Bytes occupied by `n` already-validated slots (n ≤ COUNT — no overflow).
    #[allow(dead_code)]
    fn live_bytes(&self, n: usize) -> ByteSpan {
        debug_assert!(
            n.checked_mul(T::SLOT_SIZE).is_some(),
            "live_bytes: n={n} overflows n * SLOT_SIZE={}",
            T::SLOT_SIZE
        );
        ByteSpan(n * T::SLOT_SIZE)
    }

    /// Whether the slot `[raw, raw + SLOT_SIZE)` lies entirely within this buffer.
    fn contains_slot(&self, raw: *const u8) -> Result<bool, ArenaError> {
        let start = self.start_ptr().as_raw();
        let end = start.wrapping_add(self.byte_len());
        let slot_end = raw.wrapping_add(T::SLOT_SIZE);
        if slot_end < raw {
            return Err(ArenaError::Overflow);
        }
        Ok(raw >= start && slot_end <= end)
    }
}

// ── ArenaSlotPtr ──────────────────────────────────────────────────────────────

/// A validated, aligned, non-null raw pointer to a `T::Archived` slot.
///
/// ### Invariants (proven at construction)
/// - `ptr` is non-null
/// - `ptr` is aligned to `T::SLOT_ALIGN`
/// - The `*u8 → *T::Archived` cast occurred exactly here
///
/// The `'arena` lifetime ties this pointer to the arena's immutable borrow.
/// While any `ArenaSlotPtr<'arena, T>` is live, the borrow checker blocks
/// `reset()`, `alloc()`, and `overwrite()` (all `&mut self`) on the same arena.
/// The pointer cannot be stored past the arena's backing buffer lifetime.
#[derive(Debug)]
pub struct ArenaSlotPtr<'arena, T: ArenaType> {
    ptr: NonNull<T::Archived>,
    _marker: PhantomData<&'arena T::Archived>,
}

impl<'arena, T: ArenaType> ArenaSlotPtr<'arena, T> {
    pub(crate) fn from_const(raw: *const u8) -> Result<Self, ArenaError> {
        if raw.align_offset(T::SLOT_ALIGN) != 0 {
            return Err(ArenaError::Misaligned);
        }
        NonNull::new(raw as *mut u8)
            .map(|p| Self { ptr: p.cast(), _marker: PhantomData })
            .ok_or(ArenaError::Misaligned)
    }

    pub(crate) fn from_const_bounded(
        raw: *const u8,
        buf: &'arena impl ArenaGeometry<T>,
    ) -> Result<Self, ArenaError> {
        if !buf.contains_slot(raw)? {
            return Err(ArenaError::OutOfBounds);
        }
        Self::from_const(raw)
    }

    #[must_use]
    pub fn as_ptr(&self) -> *const T::Archived {
        self.ptr.as_ptr()
    }

    // Write access through this token is deliberately absent: ArenaSlotPtr is
    // Copy and obtainable from &arena, so a *mut here would let shared holders
    // mint aliasing write pointers. Use `TypedArena::slot_mut_ptr` (which
    // requires `&mut` on the owning arena) to obtain a write pointer.
}

impl<'arena, T: ArenaType> Copy for ArenaSlotPtr<'arena, T> {}
impl<'arena, T: ArenaType> Clone for ArenaSlotPtr<'arena, T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<'arena, T: ArenaType> PartialEq for ArenaSlotPtr<'arena, T> {
    fn eq(&self, other: &Self) -> bool {
        self.ptr == other.ptr
    }
}
impl<'arena, T: ArenaType> Eq for ArenaSlotPtr<'arena, T> {}
impl<'arena, T: ArenaType> hash::Hash for ArenaSlotPtr<'arena, T> {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.ptr.as_ptr().hash(state);
    }
}

// ── ArenaBuf ─────────────────────────────────────────────────────────────────

/// A byte buffer viewed through `T`'s slot geometry. Validation-only helper.
pub(crate) struct ArenaBuf<'a, T: ArenaType> {
    data: &'a mut [u8],
    _type: PhantomData<T>,
}

impl<'a, T: ArenaType> ArenaBuf<'a, T> {
    const _NON_ZST: () = assert!(
        mem::size_of::<T::Archived>() > 0,
        "ArenaBuf: T::Archived cannot be zero-sized — all slots would alias"
    );

    pub(crate) fn new(data: &'a mut [u8], count: usize) -> Result<Self, ArenaError> {
        let needed = count
            .checked_mul(T::SLOT_SIZE)
            .ok_or(ArenaError::Overflow)?;
        if data.as_ptr().align_offset(T::SLOT_ALIGN) != 0 {
            return Err(ArenaError::Misaligned);
        }
        if data.len() < needed {
            return Err(ArenaError::TooSmall {
                needed,
                got: data.len(),
            });
        }
        Ok(Self {
            data,
            _type: PhantomData,
        })
    }
}

impl<'a, T: ArenaType> ArenaGeometry<T> for ArenaBuf<'a, T> {
    fn start_ptr(&self) -> BufferPtr {
        BufferPtr(self.data.as_ptr())
    }
    fn byte_len(&self) -> usize {
        self.data.len()
    }
}

// ── AllocatedSlots ────────────────────────────────────────────────────────────

/// The **initialized** prefix of a typed arena's backing array.
///
/// Stores a raw pointer to the invariant base of the array plus a length.
/// The pointer never changes — only `len` moves. All accessors reconstruct
/// local slices on-demand via `from_raw_parts`; nothing aliased is stored.
pub(crate) struct AllocatedSlots<'a, T: ArenaType> {
    /// Stable start of the full COUNT-element backing array. Never mutated.
    ptr: NonNull<T::Archived>,
    /// Number of initialized slots in `[0..len)`.
    len: usize,
    _marker: PhantomData<&'a mut T::Archived>,
}

// SAFETY: same conditions as &mut [T::Archived]: safe to send/share iff T::Archived is.
unsafe impl<'a, T: ArenaType> Send for AllocatedSlots<'a, T> where T::Archived: Send {}
unsafe impl<'a, T: ArenaType> Sync for AllocatedSlots<'a, T> where T::Archived: Sync {}

impl<'a, T: ArenaType> AllocatedSlots<'a, T> {
    pub(crate) fn len(&self) -> usize {
        self.len
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(crate) fn get(&self, idx: usize) -> &T::Archived {
        debug_assert!(idx < self.len, "AllocatedSlots::get: idx={idx} >= len={}", self.len);
        // SAFETY: caller ensures idx < self.len; slot is initialized.
        unsafe { &*self.ptr.as_ptr().add(idx) }
    }

    pub(crate) fn get_mut(&mut self, idx: usize) -> &mut T::Archived {
        debug_assert!(idx < self.len, "AllocatedSlots::get_mut: idx={idx} >= len={}", self.len);
        // SAFETY: caller ensures idx < self.len; slot is initialized.
        unsafe { &mut *self.ptr.as_ptr().add(idx) }
    }

    pub(crate) fn as_slice(&self) -> &[T::Archived] {
        // SAFETY: ptr..ptr+len is the initialized prefix; T::Archived: Portable (no uninit padding).
        unsafe { core::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }

    pub(crate) fn first(&self) -> Option<&T::Archived> {
        if self.len == 0 {
            None
        } else {
            Some(self.get(0))
        }
    }

    pub(crate) fn last(&self) -> Option<&T::Archived> {
        if self.len == 0 {
            None
        } else {
            Some(self.get(self.len - 1))
        }
    }

    pub(crate) fn first_mut(&mut self) -> Option<&mut T::Archived> {
        if self.len == 0 {
            None
        } else {
            Some(self.get_mut(0))
        }
    }

    pub(crate) fn last_mut(&mut self) -> Option<&mut T::Archived> {
        if self.len == 0 {
            None
        } else {
            let last = self.len - 1;
            Some(self.get_mut(last))
        }
    }

    pub(crate) fn iter_mut(&mut self) -> core::slice::IterMut<'_, T::Archived> {
        // SAFETY: ptr..ptr+len is the initialized prefix. The local slice's lifetime
        // is tied to &mut self, and IterMut stores raw pointers — not the slice itself.
        unsafe { core::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }.iter_mut()
    }

    /// Mutable byte view of the initialized region, for lifecycle policy hooks.
    pub(crate) fn as_bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: Portable guarantees no uninit padding; all len slots are initialized.
        unsafe {
            core::slice::from_raw_parts_mut(self.ptr.as_ptr() as *mut u8, self.len * T::SLOT_SIZE)
        }
    }

    pub(crate) fn start_ptr(&self) -> BufferPtr {
        BufferPtr(self.ptr.as_ptr() as *const u8)
    }

    /// Extend the initialized region by one slot.
    ///
    /// # Safety
    /// The first element of `unallocated` must have been fully written with valid
    /// `T::Archived` bytes before this call. `self` and `unallocated` must be the
    /// two adjacent halves of the same COUNT-element backing array.
    pub(crate) unsafe fn advance(&mut self, unallocated: &mut UnallocatedSlots<'a, T>) {
        self.len += 1;
        // wrapping_add is safe to call without an unsafe block; NonNull::new checks for null.
        // The backing buffer fits entirely in the address space (validated at construction),
        // so this can never actually wrap — the expect fires only on a bug.
        let next = unallocated.ptr.as_ptr().wrapping_add(1);
        unallocated.ptr =
            NonNull::new(next).expect("advance: pointer overflow — buffer at end of address space");
        unallocated.len -= 1;
    }

    /// Move the split point to 0: all slots return to `unallocated`.
    ///
    /// # Safety
    /// `self.ptr` must be the stable base of the original COUNT-element backing array.
    /// Lifecycle policy must have been applied before this call. `total` must equal COUNT.
    pub(crate) unsafe fn reset_to_start(
        &mut self,
        unallocated: &mut UnallocatedSlots<'a, T>,
        total: usize,
    ) {
        // Cast T::Archived ptr to MaybeUninit — same layout; slots become logically uninit.
        unallocated.ptr = self.ptr.cast::<MaybeUninit<T::Archived>>();
        unallocated.len = total;
        self.len = 0;
    }
}

// ── UnallocatedSlots ──────────────────────────────────────────────────────────

/// The **uninitialized** suffix of a typed arena's backing array.
///
/// Uses `MaybeUninit<T::Archived>` so that no reference to a typed, uninitialized
/// value is ever formed — byte access goes through raw pointer casts only.
pub(crate) struct UnallocatedSlots<'a, T: ArenaType> {
    /// First uninitialized slot.
    ptr: NonNull<MaybeUninit<T::Archived>>,
    /// Number of remaining uninitialized slots.
    len: usize,
    _marker: PhantomData<&'a mut MaybeUninit<T::Archived>>,
}

// SAFETY: same conditions as &mut [MaybeUninit<T::Archived>].
unsafe impl<'a, T: ArenaType> Send for UnallocatedSlots<'a, T> where T::Archived: Send {}
unsafe impl<'a, T: ArenaType> Sync for UnallocatedSlots<'a, T> where T::Archived: Sync {}

impl<'a, T: ArenaType> UnallocatedSlots<'a, T> {
    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Mutable byte view of the next slot for in-place serialization.
    ///
    /// Caller must write valid `T::Archived` bytes before calling [`AllocatedSlots::advance`].
    ///
    /// # Panics
    /// Panics in debug if `self.len == 0` — check [`is_empty`][Self::is_empty] first.
    pub(crate) fn next_slot_bytes(&mut self) -> &mut [u8] {
        debug_assert!(
            !self.is_empty(),
            "next_slot_bytes called on empty unallocated region"
        );
        // SAFETY: self.len > 0; cast MaybeUninit ptr to *mut u8 for byte-level write.
        // We never form &mut T::Archived over an uninit slot — the byte slice is the only
        // access until advance() promotes this slot to AllocatedSlots.
        unsafe { core::slice::from_raw_parts_mut(self.ptr.as_ptr() as *mut u8, T::SLOT_SIZE) }
    }

    /// Byte pointer one past the end of the unallocated suffix.
    #[allow(dead_code)]
    pub(crate) fn buf_end_ptr(&self) -> *const u8 {
        // wrapping_add is safe (no unsafe block required). The backing buffer fits in the
        // address space (validated at construction), so this never actually wraps.
        self.ptr.as_ptr().wrapping_add(self.len) as *const u8
    }
}

// ── TypedArena ────────────────────────────────────────────────────────────────

/// A fixed-capacity, bump-allocating arena over a borrowed byte slice.
///
/// Each slot holds one `T::Archived` value. `T::Archived: Portable` guarantees
/// canonical (little-endian) byte layout across architectures.
pub struct TypedArena<'a, T: ArenaType, const COUNT: usize, C: ArenaConfig = DefaultConfig> {
    /// Initialized slots `[0..len]`. `allocated.ptr` is the stable start of the buffer.
    allocated: AllocatedSlots<'a, T>,
    /// Uninitialized slots `[len..COUNT]`. Empty when the arena is at capacity.
    unallocated: UnallocatedSlots<'a, T>,
    /// Process-unique identity of this arena; embedded in every minted token.
    brand: u32,
    /// Bumped on every [`TypedArena::reset`]; invalidates outstanding tokens.
    generation: u32,
    _marker: PhantomData<C>,
}

// SAFETY: TypedArena exclusively owns its backing region for 'a — same conditions as &mut [T::Archived].
unsafe impl<'a, T: ArenaType, const COUNT: usize, C: ArenaConfig> Send
    for TypedArena<'a, T, COUNT, C>
where
    T::Archived: Send,
{
}

unsafe impl<'a, T: ArenaType, const COUNT: usize, C: ArenaConfig> Sync
    for TypedArena<'a, T, COUNT, C>
where
    T::Archived: Sync,
{
}

impl<'a, T: ArenaType, const COUNT: usize, C: ArenaConfig> ArenaGeometry<T>
    for TypedArena<'a, T, COUNT, C>
{
    fn start_ptr(&self) -> BufferPtr {
        self.allocated.start_ptr()
    }
    fn byte_len(&self) -> usize {
        // COUNT * SLOT_SIZE cannot overflow: validated at construction.
        COUNT * T::SLOT_SIZE
    }
}

impl<'a, T: ArenaType, const COUNT: usize, C: ArenaConfig> TypedArena<'a, T, COUNT, C> {
    /// Wrap `data` in a `TypedArena`.
    ///
    /// # Errors
    /// - [`ArenaError::Overflow`] — `COUNT * SLOT_SIZE` overflows `usize`
    /// - [`ArenaError::Misaligned`] — `data` is not aligned to `T::Archived`
    /// - [`ArenaError::TooSmall`] — `data` is shorter than `COUNT * SLOT_SIZE` bytes
    pub fn new(data: &'a mut [u8]) -> Result<Self, ArenaError> {
        // Token indices are u32; reject capacities that cannot be represented.
        if COUNT > u32::MAX as usize {
            return Err(ArenaError::Overflow);
        }
        // ArenaBuf::new is the single validation site (alignment, overflow, size).
        // Validate through a temporary reborrow so `data`'s own provenance tag
        // stays live: extracting the pointer first and then moving `data` into
        // ArenaBuf would invalidate that pointer under Stacked Borrows.
        ArenaBuf::<T>::new(&mut *data, COUNT)?;
        // Extract the base pointer from `data` itself — this tag carries
        // provenance over the whole buffer for 'a.
        let ptr = data.as_mut_ptr();
        // ArenaBuf::new confirmed alignment and size. NonNull::new checks for null;
        // a valid non-empty Rust slice pointer is always non-null, so this never fails
        // in practice — but we propagate the error rather than assert it statically.
        let base = NonNull::new(ptr as *mut T::Archived).ok_or(ArenaError::Misaligned)?;
        Ok(Self {
            allocated: AllocatedSlots {
                ptr: base,
                len: 0,
                _marker: PhantomData,
            },
            unallocated: UnallocatedSlots {
                ptr: base.cast(),
                len: COUNT,
                _marker: PhantomData,
            },
            brand: mint_brand(),
            generation: 0,
            _marker: PhantomData,
        })
    }

    /// Validate a token's brand, generation, and range; return the raw index.
    fn check_token(&self, idx: DynSlotIdx<'_>) -> Result<usize, ArenaError> {
        if idx.brand != self.brand {
            return Err(ArenaError::ForeignArena);
        }
        if idx.generation != self.generation {
            return Err(ArenaError::StaleToken);
        }
        let i = idx.index as usize;
        let len = self.allocated.len();
        if i >= len {
            return Err(ArenaError::OutOfRange { idx: i, len });
        }
        Ok(i)
    }

    /// Validate that `raw` is a properly-aligned, non-null pointer to a slot
    /// that lies entirely within this arena's backing buffer.
    pub fn validate_slot_ptr(&self, raw: *const u8) -> Result<ArenaSlotPtr<'_, T>, ArenaError> {
        ArenaSlotPtr::from_const_bounded(raw, self)
    }

    /// Validate `raw` and return a **write** pointer to its slot. Requires
    /// `&mut self` so the write capability is witnessed by exclusive access to
    /// the arena — a shared [`ArenaSlotPtr`] alone never grants mutation, and
    /// no shared borrows of the arena can be live across this call.
    ///
    /// Alignment, null, and bounds are validated exactly as in
    /// [`validate_slot_ptr`][Self::validate_slot_ptr]; a pointer into a
    /// different arena is rejected with [`ArenaError::OutOfBounds`].
    ///
    /// # Safety (of dereferencing the result)
    /// The returned pointer is valid for writes only while no other reference
    /// into the arena is live and only until the arena is reset or dropped.
    pub fn slot_mut_ptr(&mut self, raw: *const u8) -> Result<*mut T::Archived, ArenaError> {
        let validated = ArenaSlotPtr::<T>::from_const_bounded(raw, &*self)?;
        Ok(validated.as_ptr() as *mut T::Archived)
    }

    // ── Compile-time index path (nightly, feature = "const-index") ────────

    #[cfg(feature = "const-index")]
    #[must_use]
    pub fn const_idx<const N: usize>(&self) -> SlotIdx<'a, N>
    where
        ConstCheck<{ N < COUNT }>: IsTrue,
    {
        // The token is bound to the backing buffer's lifetime 'a (like
        // DynSlotIdx), not to this &self borrow — otherwise it could never be
        // passed to the &mut accessors of the same arena.
        SlotIdx::<N>(PhantomData)
    }

    #[cfg(feature = "const-index")]
    #[must_use]
    pub fn get_const<const N: usize>(&self, _: SlotIdx<'_, N>) -> Result<&T::Archived, ArenaError>
    where
        ConstCheck<{ N < COUNT }>: IsTrue,
    {
        let len = self.allocated.len();
        if N >= len {
            return Err(ArenaError::OutOfRange { idx: N, len });
        }
        Ok(self.allocated.get(N))
    }

    #[cfg(feature = "const-index")]
    #[must_use]
    pub fn get_const_mut<const N: usize>(
        &mut self,
        _: SlotIdx<'_, N>,
    ) -> Result<&mut T::Archived, ArenaError>
    where
        ConstCheck<{ N < COUNT }>: IsTrue,
    {
        let len = self.allocated.len();
        if N >= len {
            return Err(ArenaError::OutOfRange { idx: N, len });
        }
        Ok(self.allocated.get_mut(N))
    }

    #[cfg(feature = "const-index")]
    pub fn overwrite_const<const N: usize>(
        &mut self,
        _: SlotIdx<'_, N>,
        val: T,
    ) -> Result<(), ArenaError>
    where
        ConstCheck<{ N < COUNT }>: IsTrue,
    {
        let len = self.allocated.len();
        if N >= len {
            return Err(ArenaError::OutOfRange { idx: N, len });
        }
        let slot_bytes = self.allocated.get_mut(N).as_bytes_mut();
        let writer = Buffer::from(slot_bytes);
        to_bytes_in_with_alloc(&val, writer, SubAllocator::empty())
            .map_err(|_| ArenaError::SerializationFailed)?;
        Ok(())
    }

    // ── Runtime index path ────────────────────────────────────────────────

    /// Validate `n` against the currently allocated length and produce a [`DynSlotIdx`].
    pub fn checked_idx(&self, n: usize) -> Result<DynSlotIdx<'a>, ArenaError> {
        let len = self.allocated.len();
        if n >= len {
            return Err(ArenaError::OutOfRange { idx: n, len });
        }
        Ok(DynSlotIdx::from_trusted(n, COUNT, self.brand, self.generation))
    }

    /// Return a shared reference to the slot identified by `idx`.
    pub fn get(&self, idx: DynSlotIdx<'a>) -> Result<&T::Archived, ArenaError> {
        let i = self.check_token(idx)?;
        Ok(self.allocated.get(i))
    }

    /// Return an exclusive reference to the slot identified by `idx`.
    pub fn get_mut(&mut self, idx: DynSlotIdx<'a>) -> Result<&mut T::Archived, ArenaError> {
        let i = self.check_token(idx)?;
        Ok(self.allocated.get_mut(i))
    }

    // ── Allocation and reset ──────────────────────────────────────────────

    /// Bump-allocate the next slot by serializing `val` directly into it.
    ///
    /// Serialization happens before the boundary advances: on failure the arena's
    /// state is unchanged.
    #[must_use = "dropping the index loses the only handle to the newly allocated slot"]
    pub fn alloc(&mut self, val: T) -> Result<DynSlotIdx<'a>, ArenaError> {
        if self.unallocated.is_empty() {
            return Err(ArenaError::Full);
        }
        let len = self.allocated.len();
        {
            let slot_bytes = self.unallocated.next_slot_bytes();
            let writer = Buffer::from(slot_bytes);
            to_bytes_in_with_alloc(&val, writer, SubAllocator::empty())
                .map_err(|_| ArenaError::SerializationFailed)?;
        }
        // SAFETY: serialization succeeded; first unallocated slot holds valid T::Archived bytes.
        unsafe { self.allocated.advance(&mut self.unallocated) };
        Ok(DynSlotIdx::from_trusted(len, COUNT, self.brand, self.generation))
    }

    /// Replace the value at `idx` by serializing `val` directly into the slot's bytes.
    ///
    /// For `T::Archived: Portable`, `SerializationFailed` cannot occur.
    pub fn overwrite(&mut self, idx: DynSlotIdx<'a>, val: T) -> Result<(), ArenaError> {
        let i = self.check_token(idx)?;
        let slot_bytes = self.allocated.get_mut(i).as_bytes_mut();
        let writer = Buffer::from(slot_bytes);
        to_bytes_in_with_alloc(&val, writer, SubAllocator::empty())
            .map_err(|_| ArenaError::SerializationFailed)?;
        Ok(())
    }

    /// View all allocated slots as a contiguous slice of `T::Archived`.
    #[must_use]
    pub fn as_slice(&self) -> &[T::Archived] {
        self.allocated.as_slice()
    }

    /// Reset the bump pointer to 0, running the configured [`ResetPolicy`] over
    /// the previously-allocated region first. Increments the arena generation,
    /// invalidating all outstanding [`DynSlotIdx`] tokens: any token minted
    /// before this call fails with [`ArenaError::StaleToken`] on use.
    pub fn reset(&mut self) {
        if !self.allocated.is_empty() {
            C::ResetPolicy::on_reset(self.allocated.as_bytes_mut());
        }
        self.generation = self.generation.wrapping_add(1);
        // Pass COUNT to restore full capacity — even if slots were carved via
        // split_unallocated_bytes, those borrows have expired before reset() can be called.
        // SAFETY: allocated.ptr is the stable base of the COUNT-element array.
        unsafe { self.allocated.reset_to_start(&mut self.unallocated, COUNT) };
    }

    /// Iterate over all allocated slots in order.
    pub fn iter(&self) -> core::slice::Iter<'_, T::Archived> {
        self.as_slice().iter()
    }

    pub fn len(&self) -> usize {
        self.allocated.len()
    }
    pub fn is_empty(&self) -> bool {
        self.allocated.is_empty()
    }
    pub const fn capacity(&self) -> usize {
        COUNT
    }

    /// Shared reference to the first allocated slot, or `None` if the arena is empty.
    pub fn first(&self) -> Option<&T::Archived> {
        self.allocated.first()
    }

    /// Exclusive reference to the first allocated slot, or `None` if empty.
    pub fn first_mut(&mut self) -> Option<&mut T::Archived> {
        self.allocated.first_mut()
    }

    /// Shared reference to the last allocated slot, or `None` if empty.
    pub fn last(&self) -> Option<&T::Archived> {
        self.allocated.last()
    }

    /// Exclusive reference to the last allocated slot, or `None` if empty.
    pub fn last_mut(&mut self) -> Option<&mut T::Archived> {
        self.allocated.last_mut()
    }

    /// Raw byte pointer to the start of the backing buffer.
    ///
    /// # Safety
    /// The returned pointer is valid for `'a` (the backing buffer's lifetime).
    /// Dereferencing it, or constructing any reference from it, is only safe
    /// while the arena and its backing `&'a mut [u8]` are live. Do not store
    /// the pointer past the arena's scope.
    #[must_use]
    pub fn buf_start(&self) -> *const u8 {
        self.allocated.start_ptr().0
    }

    /// Raw byte pointer one-past-end of the full backing buffer.
    ///
    /// Stable: always equals `buf_start + COUNT * SLOT_SIZE`, even after `split_unallocated_bytes`.
    ///
    /// # Safety
    /// Same invariants as [`buf_start`][Self::buf_start]: valid only while the
    /// backing buffer is live. Do not dereference or construct references from
    /// the returned pointer past the arena's scope.
    #[must_use]
    pub fn buf_end(&self) -> *const u8 {
        // byte_len() = COUNT * SLOT_SIZE is validated to not overflow at construction.
        // wrapping_add requires no unsafe block; the buffer fits in address space so it
        // never actually wraps — the result is always the true one-past-end.
        (self.allocated.ptr.as_ptr() as *const u8).wrapping_add(self.byte_len())
    }

    /// Carve `slot_count` slots from the unallocated region and return their backing bytes.
    ///
    /// The returned `&'b mut [u8]` is tied to the exclusive borrow `'b` of `self`, so the
    /// Rust borrow checker statically enforces that any child arena built from it cannot
    /// outlive this arena. While the slice is live, no further mutation of `self` is possible.
    ///
    /// # Errors
    /// - [`ArenaError::Full`] — fewer than `slot_count` unallocated slots remain
    /// - [`ArenaError::Overflow`] — `slot_count * T::SLOT_SIZE` overflows `usize`
    pub fn split_unallocated_bytes(
        &mut self,
        slot_count: usize,
    ) -> Result<&mut [u8], ArenaError> {
        let byte_count = slot_count
            .checked_mul(T::SLOT_SIZE)
            .ok_or(ArenaError::Overflow)?;
        if slot_count > self.unallocated.len {
            return Err(ArenaError::Full);
        }
        // carved_ptr comes from a NonNull, so it is always non-null.
        let carved_ptr = self.unallocated.ptr.as_ptr() as *mut u8;
        // wrapping_add + NonNull::new: slot_count <= unallocated.len ensures this stays
        // within the validated buffer and never wraps; the ok_or catches the impossible null case.
        let new_uninit = self.unallocated.ptr.as_ptr().wrapping_add(slot_count);
        self.unallocated.ptr = NonNull::new(new_uninit).ok_or(ArenaError::Overflow)?;
        self.unallocated.len -= slot_count;
        // SAFETY: carved_ptr is non-null, aligned (derived from NonNull<MaybeUninit<T::Archived>>
        // cast to *mut u8), and byte_count bytes lie within the backing buffer for 'a ⊇ 'b.
        // The range was removed from unallocated, so the caller holds the only reference.
        Ok(unsafe { core::slice::from_raw_parts_mut(carved_ptr, byte_count) })
    }

    /// Carve a typed child scope of `CHILD_COUNT` slots from the unallocated region.
    ///
    /// While the returned [`ScopedArena`] is alive, `self` is exclusively borrowed.
    /// On drop, the configured `DropPolicy` runs over the scope's allocated slots and
    /// those slots are returned to `self` — restoring its unallocated capacity.
    ///
    /// # Errors
    /// - [`ArenaError::Full`] — fewer than `CHILD_COUNT` unallocated slots remain
    pub fn scope<'b, const CHILD_COUNT: usize>(
        &'b mut self,
    ) -> Result<ScopedArena<'b, T, CHILD_COUNT, C>, ArenaError> {
        if CHILD_COUNT > self.unallocated.len {
            return Err(ArenaError::Full);
        }
        // cast() is a safe type-only reinterpretation of the same non-null address.
        let child_base: NonNull<T::Archived> = self.unallocated.ptr.cast::<T::Archived>();
        // wrapping_add + NonNull::new: CHILD_COUNT <= unallocated.len ensures this stays in
        // the validated buffer and never wraps; ok_or catches the impossible null case.
        let new_uninit = self.unallocated.ptr.as_ptr().wrapping_add(CHILD_COUNT);
        let new_uninit = NonNull::new(new_uninit).ok_or(ArenaError::Overflow)?;
        let new_len = self.unallocated.len - CHILD_COUNT;
        // Derive the raw back-pointers LAST and apply the carve *through them*:
        // a later write through `self` would invalidate these tags under
        // Stacked Borrows, making the child's restoring Drop UB.
        let parent_uninit_ptr: *mut NonNull<MaybeUninit<T::Archived>> =
            &mut self.unallocated.ptr as *mut _;
        let parent_uninit_len: *mut usize = &mut self.unallocated.len as *mut _;
        // SAFETY: both raw pointers were just derived from exclusive borrows of
        // disjoint fields of self, which stays exclusively borrowed for 'b.
        unsafe {
            *parent_uninit_ptr = new_uninit;
            *parent_uninit_len = new_len;
        }
        Ok(ScopedArena {
            allocated: AllocatedSlots {
                ptr: child_base,
                len: 0,
                _marker: PhantomData,
            },
            unallocated: UnallocatedSlots {
                ptr: child_base.cast(),
                len: CHILD_COUNT,
                _marker: PhantomData,
            },
            parent_uninit_ptr,
            parent_uninit_len,
            taken: CHILD_COUNT,
            brand: mint_brand(),
            generation: 0,
            _config: PhantomData,
        })
    }
}

// ── Drop ──────────────────────────────────────────────────────────────────────

impl<'a, T: ArenaType, const COUNT: usize, C: ArenaConfig> Drop for TypedArena<'a, T, COUNT, C> {
    fn drop(&mut self) {
        if !self.allocated.is_empty() {
            C::DropPolicy::on_drop(self.allocated.as_bytes_mut());
        }
    }
}

// ── Trait implementations ─────────────────────────────────────────────────────

impl<T: ArenaType, const COUNT: usize, C: ArenaConfig> fmt::Display
    for TypedArena<'_, T, COUNT, C>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TypedArena[{}/{}]", self.len(), COUNT)
    }
}

impl<'arena, 'a, T: ArenaType, const COUNT: usize, C: ArenaConfig> IntoIterator
    for &'arena TypedArena<'a, T, COUNT, C>
{
    type Item = &'arena T::Archived;
    type IntoIter = core::slice::Iter<'arena, T::Archived>;

    fn into_iter(self) -> Self::IntoIter {
        self.as_slice().iter()
    }
}

impl<'arena, 'a, T: ArenaType, const COUNT: usize, C: ArenaConfig> IntoIterator
    for &'arena mut TypedArena<'a, T, COUNT, C>
{
    type Item = &'arena mut T::Archived;
    type IntoIter = core::slice::IterMut<'arena, T::Archived>;

    fn into_iter(self) -> Self::IntoIter {
        self.allocated.iter_mut()
    }
}

// Index/IndexMut are deliberately not implemented: they would panic on a
// stale or foreign token, and panics are not an acceptable failure mode in
// the execution path. Use the fallible `get`/`get_mut` instead.

// ── ScopedArena ───────────────────────────────────────────────────────────────

/// A temporary, scope-bound arena carved from a parent arena's unallocated region.
///
/// Borrows `COUNT` slots from the parent (either a [`TypedArena`] or another `ScopedArena`)
/// at construction. While alive the parent is exclusively borrowed and unavailable.
///
/// On drop:
/// 1. `C::DropPolicy` is applied to any allocated slots.
/// 2. The `COUNT` slots are returned to the parent — restoring its unallocated capacity.
///
/// Call [`ScopedArena::scope`] to create recursively nested child scopes.
pub struct ScopedArena<'parent, T: ArenaType, const COUNT: usize, C: ArenaConfig = DefaultConfig> {
    allocated: AllocatedSlots<'parent, T>,
    unallocated: UnallocatedSlots<'parent, T>,
    /// Raw pointer to the parent's `unallocated.ptr` for restoration on drop.
    parent_uninit_ptr: *mut NonNull<MaybeUninit<T::Archived>>,
    /// Raw pointer to the parent's `unallocated.len` for restoration on drop.
    parent_uninit_len: *mut usize,
    /// Number of slots borrowed from the parent; always fully returned on drop.
    taken: usize,
    /// Process-unique identity of this scope; embedded in every minted token.
    brand: u32,
    /// Bumped on every [`ScopedArena::reset`]; invalidates outstanding tokens.
    generation: u32,
    _config: PhantomData<C>,
}

// SAFETY: raw back-pointers are only accessed in Drop, while the parent is exclusively borrowed
// for 'parent. T::Archived conditions are the same as for AllocatedSlots/UnallocatedSlots.
unsafe impl<'parent, T: ArenaType, const COUNT: usize, C: ArenaConfig> Send
    for ScopedArena<'parent, T, COUNT, C>
where
    T::Archived: Send,
{
}

unsafe impl<'parent, T: ArenaType, const COUNT: usize, C: ArenaConfig> Sync
    for ScopedArena<'parent, T, COUNT, C>
where
    T::Archived: Sync,
{
}

impl<'parent, T: ArenaType, const COUNT: usize, C: ArenaConfig> ArenaGeometry<T>
    for ScopedArena<'parent, T, COUNT, C>
{
    fn start_ptr(&self) -> BufferPtr {
        self.allocated.start_ptr()
    }
    fn byte_len(&self) -> usize {
        COUNT * T::SLOT_SIZE
    }
}

impl<'parent, T: ArenaType, const COUNT: usize, C: ArenaConfig> ScopedArena<'parent, T, COUNT, C> {
    // ── Index path ────────────────────────────────────────────────────────

    /// Validate a token's brand, generation, and range; return the raw index.
    fn check_token(&self, idx: DynSlotIdx<'_>) -> Result<usize, ArenaError> {
        if idx.brand != self.brand {
            return Err(ArenaError::ForeignArena);
        }
        if idx.generation != self.generation {
            return Err(ArenaError::StaleToken);
        }
        let i = idx.index as usize;
        let len = self.allocated.len();
        if i >= len {
            return Err(ArenaError::OutOfRange { idx: i, len });
        }
        Ok(i)
    }

    /// Validate `n` against the currently allocated length and produce a [`DynSlotIdx`].
    pub fn checked_idx(&self, n: usize) -> Result<DynSlotIdx<'parent>, ArenaError> {
        let len = self.allocated.len();
        if n >= len {
            return Err(ArenaError::OutOfRange { idx: n, len });
        }
        Ok(DynSlotIdx::from_trusted(n, COUNT, self.brand, self.generation))
    }

    /// Return a shared reference to the slot identified by `idx`.
    pub fn get(&self, idx: DynSlotIdx<'parent>) -> Result<&T::Archived, ArenaError> {
        let i = self.check_token(idx)?;
        Ok(self.allocated.get(i))
    }

    /// Return an exclusive reference to the slot identified by `idx`.
    pub fn get_mut(&mut self, idx: DynSlotIdx<'parent>) -> Result<&mut T::Archived, ArenaError> {
        let i = self.check_token(idx)?;
        Ok(self.allocated.get_mut(i))
    }

    // ── Allocation and reset ──────────────────────────────────────────────

    /// Bump-allocate the next slot by serializing `val` directly into it.
    #[must_use = "dropping the index loses the only handle to the newly allocated slot"]
    pub fn alloc(&mut self, val: T) -> Result<DynSlotIdx<'parent>, ArenaError> {
        if self.unallocated.is_empty() {
            return Err(ArenaError::Full);
        }
        let len = self.allocated.len();
        {
            let slot_bytes = self.unallocated.next_slot_bytes();
            let writer = Buffer::from(slot_bytes);
            to_bytes_in_with_alloc(&val, writer, SubAllocator::empty())
                .map_err(|_| ArenaError::SerializationFailed)?;
        }
        // SAFETY: serialization succeeded; first unallocated slot holds valid T::Archived bytes.
        unsafe { self.allocated.advance(&mut self.unallocated) };
        Ok(DynSlotIdx::from_trusted(len, COUNT, self.brand, self.generation))
    }

    /// Replace the value at `idx` by serializing `val` directly into the slot's bytes.
    pub fn overwrite(&mut self, idx: DynSlotIdx<'parent>, val: T) -> Result<(), ArenaError> {
        let i = self.check_token(idx)?;
        let slot_bytes = self.allocated.get_mut(i).as_bytes_mut();
        let writer = Buffer::from(slot_bytes);
        to_bytes_in_with_alloc(&val, writer, SubAllocator::empty())
            .map_err(|_| ArenaError::SerializationFailed)?;
        Ok(())
    }

    /// Reset the bump pointer to 0, running the configured [`ResetPolicy`] first.
    /// Increments the scope's generation, invalidating all outstanding tokens.
    ///
    /// Does **not** return slots to the parent; parent capacity is restored only on drop.
    pub fn reset(&mut self) {
        if !self.allocated.is_empty() {
            C::ResetPolicy::on_reset(self.allocated.as_bytes_mut());
        }
        self.generation = self.generation.wrapping_add(1);
        // SAFETY: allocated.ptr is the stable base of the COUNT-element region.
        unsafe { self.allocated.reset_to_start(&mut self.unallocated, COUNT) };
    }

    // ── Accessors ─────────────────────────────────────────────────────────

    /// View all allocated slots as a contiguous slice.
    #[must_use]
    pub fn as_slice(&self) -> &[T::Archived] {
        self.allocated.as_slice()
    }

    /// Iterate over all allocated slots in order.
    pub fn iter(&self) -> core::slice::Iter<'_, T::Archived> {
        self.as_slice().iter()
    }

    pub fn len(&self) -> usize {
        self.allocated.len()
    }
    pub fn is_empty(&self) -> bool {
        self.allocated.is_empty()
    }
    pub const fn capacity(&self) -> usize {
        COUNT
    }

    pub fn first(&self) -> Option<&T::Archived> {
        self.allocated.first()
    }
    pub fn first_mut(&mut self) -> Option<&mut T::Archived> {
        self.allocated.first_mut()
    }
    pub fn last(&self) -> Option<&T::Archived> {
        self.allocated.last()
    }
    pub fn last_mut(&mut self) -> Option<&mut T::Archived> {
        self.allocated.last_mut()
    }

    /// Raw byte pointer to the start of this scope's backing region.
    ///
    /// # Safety
    /// Valid only while the scope's parent arena and its backing buffer are live.
    /// Dereferencing or constructing any reference from this pointer past that
    /// scope is undefined behaviour.
    #[must_use]
    pub fn buf_start(&self) -> *const u8 {
        self.allocated.start_ptr().0
    }

    /// Raw byte pointer one-past-end of this scope's backing region.
    ///
    /// # Safety
    /// Same invariants as [`buf_start`][Self::buf_start].
    #[must_use]
    pub fn buf_end(&self) -> *const u8 {
        // byte_len() = COUNT * SLOT_SIZE, validated at construction. wrapping_add
        // requires no unsafe block and never wraps because the buffer fits in address space.
        (self.allocated.ptr.as_ptr() as *const u8).wrapping_add(self.byte_len())
    }

    /// Validate `raw` as an aligned, in-bounds slot pointer within this scope.
    pub fn validate_slot_ptr(&self, raw: *const u8) -> Result<ArenaSlotPtr<'_, T>, ArenaError> {
        ArenaSlotPtr::from_const_bounded(raw, self)
    }

    // ── Recursive child scope ─────────────────────────────────────────────

    /// Carve a child scope of `CHILD_COUNT` slots from this scope's unallocated region.
    ///
    /// Mirrors [`TypedArena::scope`]: parent is exclusively borrowed while child is alive;
    /// child's slots are returned on drop.
    ///
    /// # Errors
    /// - [`ArenaError::Full`] — fewer than `CHILD_COUNT` unallocated slots remain
    pub fn scope<'child, const CHILD_COUNT: usize>(
        &'child mut self,
    ) -> Result<ScopedArena<'child, T, CHILD_COUNT, C>, ArenaError> {
        if CHILD_COUNT > self.unallocated.len {
            return Err(ArenaError::Full);
        }
        let child_base: NonNull<T::Archived> = self.unallocated.ptr.cast::<T::Archived>();
        let new_uninit = self.unallocated.ptr.as_ptr().wrapping_add(CHILD_COUNT);
        let new_uninit = NonNull::new(new_uninit).ok_or(ArenaError::Overflow)?;
        let new_len = self.unallocated.len - CHILD_COUNT;
        // Derive the raw back-pointers LAST and apply the carve *through them*
        // (a write through `self` would invalidate the tags for the child's Drop).
        let parent_uninit_ptr: *mut NonNull<MaybeUninit<T::Archived>> =
            &mut self.unallocated.ptr as *mut _;
        let parent_uninit_len: *mut usize = &mut self.unallocated.len as *mut _;
        // SAFETY: just derived from exclusive borrows of disjoint fields of self,
        // which stays exclusively borrowed for the child's lifetime.
        unsafe {
            *parent_uninit_ptr = new_uninit;
            *parent_uninit_len = new_len;
        }
        Ok(ScopedArena {
            allocated: AllocatedSlots {
                ptr: child_base,
                len: 0,
                _marker: PhantomData,
            },
            unallocated: UnallocatedSlots {
                ptr: child_base.cast(),
                len: CHILD_COUNT,
                _marker: PhantomData,
            },
            parent_uninit_ptr,
            parent_uninit_len,
            taken: CHILD_COUNT,
            brand: mint_brand(),
            generation: 0,
            _config: PhantomData,
        })
    }
}

// ── ScopedArena Drop ──────────────────────────────────────────────────────────

impl<'parent, T: ArenaType, const COUNT: usize, C: ArenaConfig> Drop
    for ScopedArena<'parent, T, COUNT, C>
{
    fn drop(&mut self) {
        if !self.allocated.is_empty() {
            C::DropPolicy::on_drop(self.allocated.as_bytes_mut());
        }
        // Reverse the carve: walk parent's ptr back by `taken` and restore `taken` to len.
        // SAFETY: 'parent outlives self; parent is exclusively borrowed while self is alive,
        // so no other code touches parent.unallocated. `taken` exactly cancels the forward
        // advance applied at construction — wrapping_sub + NonNull::new catches any bug.
        unsafe {
            let prev = (*self.parent_uninit_ptr).as_ptr().wrapping_sub(self.taken);
            *self.parent_uninit_ptr = NonNull::new(prev)
                .expect("ScopedArena drop: parent pointer wrapped — this is a bug");
            *self.parent_uninit_len += self.taken;
        }
    }
}

// ── ScopedArena trait implementations ────────────────────────────────────────

impl<T: ArenaType, const COUNT: usize, C: ArenaConfig> fmt::Debug for ScopedArena<'_, T, COUNT, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScopedArena")
            .field("len", &self.len())
            .field("capacity", &COUNT)
            .finish_non_exhaustive()
    }
}

impl<T: ArenaType, const COUNT: usize, C: ArenaConfig> fmt::Display
    for ScopedArena<'_, T, COUNT, C>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ScopedArena[{}/{}]", self.len(), COUNT)
    }
}

impl<'scope, 'parent, T: ArenaType, const COUNT: usize, C: ArenaConfig> IntoIterator
    for &'scope ScopedArena<'parent, T, COUNT, C>
{
    type Item = &'scope T::Archived;
    type IntoIter = core::slice::Iter<'scope, T::Archived>;

    fn into_iter(self) -> Self::IntoIter {
        self.as_slice().iter()
    }
}

impl<'scope, 'parent, T: ArenaType, const COUNT: usize, C: ArenaConfig> IntoIterator
    for &'scope mut ScopedArena<'parent, T, COUNT, C>
{
    type Item = &'scope mut T::Archived;
    type IntoIter = core::slice::IterMut<'scope, T::Archived>;

    fn into_iter(self) -> Self::IntoIter {
        self.allocated.iter_mut()
    }
}

// Index/IndexMut deliberately omitted (panicking accessors); use get/get_mut.

// ── DynScopedArena ────────────────────────────────────────────────────────────

/// A temporary, scope-bound arena with a **runtime** capacity, carved from a parent's
/// unallocated region.
///
/// Constructed via [`TypedArena::dyn_scope`], [`ScopedArena::dyn_scope`], or
/// [`DynScopedArena::dyn_scope`]. The requested count is clamped to the parent's
/// available slots — construction is therefore infallible.
///
/// On drop, the configured `DropPolicy` is applied and all borrowed slots are returned
/// to the parent. Supports recursive nesting via [`DynScopedArena::dyn_scope`] and
/// [`DynScopedArena::scope`].
pub struct DynScopedArena<'parent, T: ArenaType, C: ArenaConfig = DefaultConfig> {
    allocated: AllocatedSlots<'parent, T>,
    unallocated: UnallocatedSlots<'parent, T>,
    /// Raw pointer to the parent's `unallocated.ptr` for restoration on drop.
    parent_uninit_ptr: *mut NonNull<MaybeUninit<T::Archived>>,
    /// Raw pointer to the parent's `unallocated.len` for restoration on drop.
    parent_uninit_len: *mut usize,
    /// Slots borrowed from the parent; always fully returned on drop.
    taken: usize,
    /// Runtime capacity (≤ parent's available slots at construction time).
    capacity: usize,
    /// Process-unique identity of this scope; embedded in every minted token.
    brand: u32,
    /// Bumped on every [`DynScopedArena::reset`]; invalidates outstanding tokens.
    generation: u32,
    _config: PhantomData<C>,
}

unsafe impl<'parent, T: ArenaType, C: ArenaConfig> Send for DynScopedArena<'parent, T, C> where
    T::Archived: Send
{
}

unsafe impl<'parent, T: ArenaType, C: ArenaConfig> Sync for DynScopedArena<'parent, T, C> where
    T::Archived: Sync
{
}

impl<'parent, T: ArenaType, C: ArenaConfig> ArenaGeometry<T> for DynScopedArena<'parent, T, C> {
    fn start_ptr(&self) -> BufferPtr {
        self.allocated.start_ptr()
    }
    fn byte_len(&self) -> usize {
        self.capacity * T::SLOT_SIZE
    }
}

impl<'parent, T: ArenaType, C: ArenaConfig> DynScopedArena<'parent, T, C> {
    // ── Internal constructor ──────────────────────────────────────────────

    /// Carve `count.min(available)` slots from the `unallocated` region of any parent.
    ///
    /// This is the single construction site shared by `TypedArena::dyn_scope`,
    /// `ScopedArena::dyn_scope`, and `DynScopedArena::dyn_scope`.
    ///
    /// # Safety
    /// `parent_uninit_ptr` and `parent_uninit_len` must point to the parent's
    /// `unallocated.ptr` and `unallocated.len` fields respectively, and the parent
    /// must be exclusively borrowed for `'parent`.
    unsafe fn from_parent(
        parent_uninit_ptr: *mut NonNull<MaybeUninit<T::Archived>>,
        parent_uninit_len: *mut usize,
        count: usize,
    ) -> Self {
        // SAFETY: caller guarantees parent_uninit_ptr / _len are valid for 'parent.
        let available = unsafe { *parent_uninit_len };
        let actual = count.min(available);

        let child_base: NonNull<T::Archived> =
            unsafe { (*parent_uninit_ptr).cast::<T::Archived>() };

        if actual > 0 {
            // wrapping_add: actual <= available <= parent buffer size, so it never wraps.
            let new_uninit = unsafe { (*parent_uninit_ptr).as_ptr().wrapping_add(actual) };
            // Null is impossible here (we're within a validated buffer), but we check anyway.
            unsafe {
                *parent_uninit_ptr = NonNull::new(new_uninit)
                    .expect("DynScopedArena: parent pointer overflow — this is a bug");
                *parent_uninit_len -= actual;
            }
        }

        DynScopedArena {
            allocated: AllocatedSlots {
                ptr: child_base,
                len: 0,
                _marker: PhantomData,
            },
            unallocated: UnallocatedSlots {
                ptr: child_base.cast(),
                len: actual,
                _marker: PhantomData,
            },
            parent_uninit_ptr,
            parent_uninit_len,
            taken: actual,
            capacity: actual,
            brand: mint_brand(),
            generation: 0,
            _config: PhantomData,
        }
    }

    // ── Index path ────────────────────────────────────────────────────────

    /// Validate a token's brand, generation, and range; return the raw index.
    fn check_token(&self, idx: DynSlotIdx<'_>) -> Result<usize, ArenaError> {
        if idx.brand != self.brand {
            return Err(ArenaError::ForeignArena);
        }
        if idx.generation != self.generation {
            return Err(ArenaError::StaleToken);
        }
        let i = idx.index as usize;
        let len = self.allocated.len();
        if i >= len {
            return Err(ArenaError::OutOfRange { idx: i, len });
        }
        Ok(i)
    }

    /// Validate `n` against the currently allocated length and produce a [`DynSlotIdx`].
    pub fn checked_idx(&self, n: usize) -> Result<DynSlotIdx<'parent>, ArenaError> {
        let len = self.allocated.len();
        if n >= len {
            return Err(ArenaError::OutOfRange { idx: n, len });
        }
        Ok(DynSlotIdx::from_trusted(
            n,
            self.capacity,
            self.brand,
            self.generation,
        ))
    }

    /// Return a shared reference to the slot identified by `idx`.
    pub fn get(&self, idx: DynSlotIdx<'parent>) -> Result<&T::Archived, ArenaError> {
        let i = self.check_token(idx)?;
        Ok(self.allocated.get(i))
    }

    /// Return an exclusive reference to the slot identified by `idx`.
    pub fn get_mut(&mut self, idx: DynSlotIdx<'parent>) -> Result<&mut T::Archived, ArenaError> {
        let i = self.check_token(idx)?;
        Ok(self.allocated.get_mut(i))
    }

    // ── Allocation and reset ──────────────────────────────────────────────

    /// Bump-allocate the next slot by serializing `val` directly into it.
    #[must_use = "dropping the index loses the only handle to the newly allocated slot"]
    pub fn alloc(&mut self, val: T) -> Result<DynSlotIdx<'parent>, ArenaError> {
        if self.unallocated.is_empty() {
            return Err(ArenaError::Full);
        }
        let len = self.allocated.len();
        {
            let slot_bytes = self.unallocated.next_slot_bytes();
            let writer = Buffer::from(slot_bytes);
            to_bytes_in_with_alloc(&val, writer, SubAllocator::empty())
                .map_err(|_| ArenaError::SerializationFailed)?;
        }
        unsafe { self.allocated.advance(&mut self.unallocated) };
        Ok(DynSlotIdx::from_trusted(
            len,
            self.capacity,
            self.brand,
            self.generation,
        ))
    }

    /// Replace the value at `idx` by serializing `val` directly into the slot's bytes.
    pub fn overwrite(&mut self, idx: DynSlotIdx<'parent>, val: T) -> Result<(), ArenaError> {
        let i = self.check_token(idx)?;
        let slot_bytes = self.allocated.get_mut(i).as_bytes_mut();
        let writer = Buffer::from(slot_bytes);
        to_bytes_in_with_alloc(&val, writer, SubAllocator::empty())
            .map_err(|_| ArenaError::SerializationFailed)?;
        Ok(())
    }

    /// Reset the bump pointer to 0, running the configured [`ResetPolicy`] first.
    /// Increments the scope's generation, invalidating all outstanding tokens.
    ///
    /// Does **not** return slots to the parent; that happens only on drop.
    pub fn reset(&mut self) {
        if !self.allocated.is_empty() {
            C::ResetPolicy::on_reset(self.allocated.as_bytes_mut());
        }
        self.generation = self.generation.wrapping_add(1);
        unsafe {
            self.allocated
                .reset_to_start(&mut self.unallocated, self.capacity)
        };
    }

    // ── Accessors ─────────────────────────────────────────────────────────

    #[must_use]
    pub fn as_slice(&self) -> &[T::Archived] {
        self.allocated.as_slice()
    }

    pub fn iter(&self) -> core::slice::Iter<'_, T::Archived> {
        self.as_slice().iter()
    }

    pub fn len(&self) -> usize {
        self.allocated.len()
    }
    pub fn is_empty(&self) -> bool {
        self.allocated.is_empty()
    }
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn first(&self) -> Option<&T::Archived> {
        self.allocated.first()
    }
    pub fn first_mut(&mut self) -> Option<&mut T::Archived> {
        self.allocated.first_mut()
    }
    pub fn last(&self) -> Option<&T::Archived> {
        self.allocated.last()
    }
    pub fn last_mut(&mut self) -> Option<&mut T::Archived> {
        self.allocated.last_mut()
    }

    /// Raw byte pointer to the start of this scope's backing region.
    ///
    /// # Safety
    /// Valid only while the scope's parent arena and its backing buffer are live.
    /// Dereferencing or constructing any reference from this pointer past that
    /// scope is undefined behaviour.
    #[must_use]
    pub fn buf_start(&self) -> *const u8 {
        self.allocated.start_ptr().0
    }

    /// Raw byte pointer one-past-end of this scope's backing region.
    ///
    /// # Safety
    /// Same invariants as [`buf_start`][Self::buf_start].
    #[must_use]
    pub fn buf_end(&self) -> *const u8 {
        (self.allocated.ptr.as_ptr() as *const u8).wrapping_add(self.byte_len())
    }

    pub fn validate_slot_ptr(&self, raw: *const u8) -> Result<ArenaSlotPtr<'_, T>, ArenaError> {
        ArenaSlotPtr::from_const_bounded(raw, self)
    }

    // ── Recursive child scopes ────────────────────────────────────────────

    /// Carve a compile-time-sized child scope from this arena's unallocated region.
    ///
    /// # Errors
    /// - [`ArenaError::Full`] — fewer than `CHILD_COUNT` unallocated slots remain
    pub fn scope<'child, const CHILD_COUNT: usize>(
        &'child mut self,
    ) -> Result<ScopedArena<'child, T, CHILD_COUNT, C>, ArenaError> {
        if CHILD_COUNT > self.unallocated.len {
            return Err(ArenaError::Full);
        }
        let child_base: NonNull<T::Archived> = self.unallocated.ptr.cast::<T::Archived>();
        let new_uninit = self.unallocated.ptr.as_ptr().wrapping_add(CHILD_COUNT);
        let new_uninit = NonNull::new(new_uninit).ok_or(ArenaError::Overflow)?;
        let new_len = self.unallocated.len - CHILD_COUNT;
        // Derive the raw back-pointers LAST and apply the carve *through them*
        // (a write through `self` would invalidate the tags for the child's Drop).
        let parent_uninit_ptr: *mut NonNull<MaybeUninit<T::Archived>> =
            &mut self.unallocated.ptr as *mut _;
        let parent_uninit_len: *mut usize = &mut self.unallocated.len as *mut _;
        // SAFETY: just derived from exclusive borrows of disjoint fields of self,
        // which stays exclusively borrowed for the child's lifetime.
        unsafe {
            *parent_uninit_ptr = new_uninit;
            *parent_uninit_len = new_len;
        }
        Ok(ScopedArena {
            allocated: AllocatedSlots {
                ptr: child_base,
                len: 0,
                _marker: PhantomData,
            },
            unallocated: UnallocatedSlots {
                ptr: child_base.cast(),
                len: CHILD_COUNT,
                _marker: PhantomData,
            },
            parent_uninit_ptr,
            parent_uninit_len,
            taken: CHILD_COUNT,
            brand: mint_brand(),
            generation: 0,
            _config: PhantomData,
        })
    }

    /// Carve a runtime-sized child scope, capped at available unallocated slots.
    ///
    /// Construction is infallible: if `count` exceeds available slots, the scope's
    /// capacity is silently reduced to fit. Check [`DynScopedArena::capacity`] for
    /// the actual size granted.
    pub fn dyn_scope<'child>(&'child mut self, count: usize) -> DynScopedArena<'child, T, C> {
        // SAFETY: self is the parent; we pass pointers to our own unallocated fields.
        // Self is exclusively borrowed for 'child while the returned scope is alive.
        unsafe {
            DynScopedArena::from_parent(
                &mut self.unallocated.ptr as *mut _,
                &mut self.unallocated.len as *mut _,
                count,
            )
        }
    }
}

// ── DynScopedArena Drop ───────────────────────────────────────────────────────

impl<'parent, T: ArenaType, C: ArenaConfig> Drop for DynScopedArena<'parent, T, C> {
    fn drop(&mut self) {
        if !self.allocated.is_empty() {
            C::DropPolicy::on_drop(self.allocated.as_bytes_mut());
        }
        // Reverse the carve. Same pattern as ScopedArena::drop.
        // SAFETY: 'parent outlives self; parent is exclusively borrowed while self is alive.
        unsafe {
            let prev = (*self.parent_uninit_ptr).as_ptr().wrapping_sub(self.taken);
            *self.parent_uninit_ptr = NonNull::new(prev)
                .expect("DynScopedArena drop: parent pointer wrapped — this is a bug");
            *self.parent_uninit_len += self.taken;
        }
    }
}

// ── DynScopedArena trait implementations ─────────────────────────────────────

impl<T: ArenaType, C: ArenaConfig> fmt::Debug for DynScopedArena<'_, T, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DynScopedArena")
            .field("len", &self.len())
            .field("capacity", &self.capacity)
            .finish_non_exhaustive()
    }
}

impl<T: ArenaType, C: ArenaConfig> fmt::Display for DynScopedArena<'_, T, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DynScopedArena[{}/{}]", self.len(), self.capacity)
    }
}

impl<'scope, 'parent, T: ArenaType, C: ArenaConfig> IntoIterator
    for &'scope DynScopedArena<'parent, T, C>
{
    type Item = &'scope T::Archived;
    type IntoIter = core::slice::Iter<'scope, T::Archived>;

    fn into_iter(self) -> Self::IntoIter {
        self.as_slice().iter()
    }
}

impl<'scope, 'parent, T: ArenaType, C: ArenaConfig> IntoIterator
    for &'scope mut DynScopedArena<'parent, T, C>
{
    type Item = &'scope mut T::Archived;
    type IntoIter = core::slice::IterMut<'scope, T::Archived>;

    fn into_iter(self) -> Self::IntoIter {
        self.allocated.iter_mut()
    }
}

// Index/IndexMut deliberately omitted (panicking accessors); use get/get_mut.

// ── dyn_scope constructors on TypedArena and ScopedArena ─────────────────────

impl<'a, T: ArenaType, const COUNT: usize, C: ArenaConfig> TypedArena<'a, T, COUNT, C> {
    /// Carve a runtime-sized child scope, capped at available unallocated slots.
    ///
    /// Construction is infallible: if `count` exceeds available slots, the scope's
    /// capacity is silently reduced to fit. Check [`DynScopedArena::capacity`] for
    /// the actual size granted.
    pub fn dyn_scope<'b>(&'b mut self, count: usize) -> DynScopedArena<'b, T, C> {
        // SAFETY: self is the parent; pointers into our unallocated fields; exclusively borrowed.
        unsafe {
            DynScopedArena::from_parent(
                &mut self.unallocated.ptr as *mut _,
                &mut self.unallocated.len as *mut _,
                count,
            )
        }
    }
}

impl<'parent, T: ArenaType, const COUNT: usize, C: ArenaConfig> ScopedArena<'parent, T, COUNT, C> {
    /// Carve a runtime-sized child scope, capped at available unallocated slots.
    ///
    /// Construction is infallible: if `count` exceeds available slots, the scope's
    /// capacity is silently reduced to fit. Check [`DynScopedArena::capacity`] for
    /// the actual size granted.
    pub fn dyn_scope<'child>(&'child mut self, count: usize) -> DynScopedArena<'child, T, C> {
        unsafe {
            DynScopedArena::from_parent(
                &mut self.unallocated.ptr as *mut _,
                &mut self.unallocated.len as *mut _,
                count,
            )
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use rkyv::{Archive, Deserialize, Serialize};
    use std::format;
    use std::string::ToString;
    use std::vec;
    use std::vec::Vec;

    #[repr(align(8))]
    struct AlignedBuf<const N: usize>([u8; N]);

    #[derive(Archive, Serialize, Deserialize, Debug, PartialEq)]
    #[rkyv(derive(Debug, PartialEq, Clone, Copy))]
    struct Point {
        x: u32,
        y: u32,
    }

    // SAFETY: ArchivedPoint is #[repr(C)] with two ArchivedU32 (u32_le) fields:
    // size 8, align 4, zero padding. Every byte is always initialized.
    unsafe impl NoUndef for ArchivedPoint {}

    // SAFETY: same layout argument as NoUndef above; all-zero bytes are a
    // valid value (x == 0, y == 0) and the type is Copy with no padding.
    #[cfg(feature = "pod")]
    unsafe impl bytemuck::Zeroable for ArchivedPoint {}
    #[cfg(feature = "pod")]
    unsafe impl bytemuck::Pod for ArchivedPoint {}

    const ARENA_CAP: usize = 8;

    // ── Construction guards ───────────────────────────────────────────────

    #[test]
    fn new_rejects_misaligned_buffer() {
        if Point::SLOT_ALIGN <= 1 {
            return;
        }
        let mut raw = AlignedBuf::<65>([0u8; 65]);
        let misaligned = &mut raw.0[1..];
        assert_eq!(
            TypedArena::<Point, ARENA_CAP>::new(misaligned).err(),
            Some(ArenaError::Misaligned),
        );
    }

    #[test]
    fn new_rejects_undersized_buffer() {
        let mut buf = AlignedBuf::<1>([0u8; 1]);
        let needed = ARENA_CAP * Point::SLOT_SIZE;
        assert_eq!(
            TypedArena::<Point, ARENA_CAP>::new(&mut buf.0).err(),
            Some(ArenaError::TooSmall { needed, got: 1 }),
        );
    }

    #[test]
    fn new_rejects_overflow_in_total_size() {
        let mut buf = [0u8; 64];
        let count_that_overflows = usize::MAX / Point::SLOT_SIZE + 1;
        assert_eq!(
            ArenaBuf::<Point>::new(&mut buf, count_that_overflows).err(),
            Some(ArenaError::Overflow),
        );
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "live_bytes")]
    fn live_bytes_debug_assert_fires_on_overflow_input() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let arena_buf = ArenaBuf::<Point>::new(&mut buf.0, 4).unwrap();
        let _ = arena_buf.live_bytes(usize::MAX / Point::SLOT_SIZE + 1);
    }

    // ── ArenaSlotPtr guards ───────────────────────────────────────────────

    #[test]
    fn slot_ptr_rejects_misaligned() {
        // With rkyv's unaligned layout SLOT_ALIGN is 1 and no pointer can be
        // misaligned; the check only bites for over-aligned archived types.
        if Point::SLOT_ALIGN <= 1 {
            return;
        }
        let buf = [0u8; 64];
        let misaligned = unsafe { buf.as_ptr().add(1) };
        assert_eq!(
            ArenaSlotPtr::<Point>::from_const(misaligned).unwrap_err(),
            ArenaError::Misaligned,
        );
    }

    #[test]
    fn slot_ptr_rejects_null() {
        let null: *const u8 = core::ptr::null();
        assert_eq!(
            ArenaSlotPtr::<Point>::from_const(null).unwrap_err(),
            ArenaError::Misaligned,
        );
    }

    #[test]
    fn bounded_ptr_rejects_pointer_past_arena_end() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        let past_end = arena.buf_end();
        assert_eq!(
            arena.validate_slot_ptr(past_end).unwrap_err(),
            ArenaError::OutOfBounds,
        );
    }

    #[test]
    fn bounded_ptr_accepts_pointer_inside_arena() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        let first_slot = arena.buf_start();
        assert!(arena.validate_slot_ptr(first_slot).is_ok());
    }

    // ── Runtime index path ────────────────────────────────────────────────

    #[test]
    fn alloc_and_get_roundtrip() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();

        let idx = arena.alloc(Point { x: 10, y: 20 }).unwrap();
        let archived = arena.get(idx).unwrap();
        assert_eq!(archived.x.to_native(), 10);
        assert_eq!(archived.y.to_native(), 20);
    }

    #[test]
    fn alloc_fills_to_capacity_then_returns_full_error() {
        let mut buf = AlignedBuf::<32>([0u8; 32]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        for i in 0..4 {
            assert!(arena.alloc(Point { x: i, y: (2 * i) }).is_ok());
        }
        assert_eq!(
            arena.alloc(Point { x: 0, y: 0 }).unwrap_err(),
            ArenaError::Full
        );
        assert_eq!(arena.len(), 4);
    }

    #[test]
    fn reset_allows_reuse() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        for _ in 0..4 {
            arena.alloc(Point { x: 0, y: 0 }).unwrap();
        }
        arena.reset();
        assert!(arena.is_empty());
        assert!(arena.alloc(Point { x: 0, y: 0 }).is_ok());
    }

    #[test]
    fn secure_config_zeroes_on_reset() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        {
            let mut arena = TypedArena::<Point, 4, SecureConfig>::new(&mut buf.0).unwrap();
            arena
                .alloc(Point {
                    x: 0xDEAD,
                    y: 0xBEEF,
                })
                .unwrap();
            arena.reset();
            assert!(arena.is_empty());
        }
        assert!(buf.0[..Point::SLOT_SIZE].iter().all(|&b| b == 0));
    }

    #[test]
    fn secure_config_zeroes_on_drop_without_reset() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        {
            let mut arena = TypedArena::<Point, 4, SecureConfig>::new(&mut buf.0).unwrap();
            arena
                .alloc(Point {
                    x: 0xDEAD,
                    y: 0xBEEF,
                })
                .unwrap();
        }
        assert!(buf.0[..Point::SLOT_SIZE].iter().all(|&b| b == 0));
    }

    #[test]
    fn custom_config_compiles_and_runs() {
        struct MyPolicy;
        unsafe impl ResetPolicy for MyPolicy {
            fn on_reset(_: &mut [u8]) {}
        }
        unsafe impl DropPolicy for MyPolicy {
            fn on_drop(_: &mut [u8]) {}
        }
        struct MyConfig;
        unsafe impl ArenaConfig for MyConfig {
            type ResetPolicy = MyPolicy;
            type DropPolicy = MyPolicy;
        }

        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4, MyConfig>::new(&mut buf.0).unwrap();
        arena.alloc(Point { x: 1, y: 2 }).unwrap();
        arena.reset();
        assert!(arena.is_empty());
    }

    #[test]
    fn as_slice_covers_allocated_slots() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        arena.alloc(Point { x: 0, y: 0 }).unwrap();
        arena.alloc(Point { x: 0, y: 0 }).unwrap();
        assert_eq!(arena.as_slice().len(), 2);
    }

    #[test]
    fn checked_idx_returns_out_of_range_on_empty_arena() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        assert_eq!(
            arena.checked_idx(0).unwrap_err(),
            ArenaError::OutOfRange { idx: 0, len: 0 },
        );
    }

    #[test]
    fn checked_idx_returns_out_of_range_for_large_index() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        assert_eq!(
            arena.checked_idx(5).unwrap_err(),
            ArenaError::OutOfRange { idx: 5, len: 0 },
        );
    }

    #[test]
    fn stale_dyn_idx_rejected_after_reset() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        let _ = arena.alloc(Point { x: 0, y: 0 }).unwrap();
        let idx = arena.checked_idx(0).unwrap();
        arena.reset(); // bumps the generation
        assert_eq!(arena.get(idx).unwrap_err(), ArenaError::StaleToken);
    }

    #[test]
    fn stale_dyn_idx_rejected_even_after_realloc() {
        // The ABA case: after reset + re-alloc the slot holds a new value;
        // a pre-reset token must NOT silently alias it.
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        let old = arena.alloc(Point { x: 1, y: 1 }).unwrap();
        arena.reset();
        let _new = arena.alloc(Point { x: 2, y: 2 }).unwrap();
        assert_eq!(arena.get(old).unwrap_err(), ArenaError::StaleToken);
        assert_eq!(
            arena.overwrite(old, Point { x: 9, y: 9 }).unwrap_err(),
            ArenaError::StaleToken
        );
    }

    #[test]
    fn foreign_token_rejected_across_same_lifetime_arenas() {
        // Two arenas whose backing buffers have the same lifetime: the brand
        // check must reject tokens minted by the other arena.
        let mut buf_a = AlignedBuf::<64>([0u8; 64]);
        let mut buf_b = AlignedBuf::<64>([0u8; 64]);
        let mut arena_a = TypedArena::<Point, 4>::new(&mut buf_a.0).unwrap();
        let mut arena_b = TypedArena::<Point, 4>::new(&mut buf_b.0).unwrap();
        let idx_a = arena_a.alloc(Point { x: 1, y: 2 }).unwrap();
        let _idx_b = arena_b.alloc(Point { x: 3, y: 4 }).unwrap();
        assert_eq!(arena_b.get(idx_a).unwrap_err(), ArenaError::ForeignArena);
        assert_eq!(
            arena_b.overwrite(idx_a, Point { x: 0, y: 0 }).unwrap_err(),
            ArenaError::ForeignArena
        );
    }

    // ── Ergonomic trait implementations ──────────────────────────────────

    #[test]
    fn error_display_messages() {
        assert_eq!(ArenaError::Full.to_string(), "arena is at capacity");
        assert_eq!(
            ArenaError::OutOfRange { idx: 3, len: 2 }.to_string(),
            "slot index 3 is out of allocated range (len=2)"
        );
        assert_eq!(
            ArenaError::Misaligned.to_string(),
            "backing memory is misaligned"
        );
        assert_eq!(
            ArenaError::Overflow.to_string(),
            "size arithmetic overflowed usize"
        );
        assert_eq!(
            ArenaError::TooSmall { needed: 64, got: 1 }.to_string(),
            "buffer too small: needed 64B, got 1B",
        );
        assert_eq!(
            ArenaError::OutOfBounds.to_string(),
            "slot pointer lies outside arena bounds"
        );
        assert_eq!(
            ArenaError::SerializationFailed.to_string(),
            "serialization of value into archived form failed"
        );
        assert_eq!(
            ArenaError::StaleToken.to_string(),
            "slot token predates the arena's most recent reset"
        );
        assert_eq!(
            ArenaError::ForeignArena.to_string(),
            "slot token was minted by a different arena"
        );
    }

    #[test]
    fn proof_token_extraction() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        arena.alloc(Point { x: 0, y: 0 }).unwrap();

        let idx = arena.checked_idx(0).unwrap();
        assert_eq!(usize::from(idx), 0);
        assert!(idx == 0usize);
        assert_eq!(idx.to_string(), "idx:0");

        let offset: SlotOffset = arena.slot_offset(usize::from(idx)).unwrap();
        assert_eq!(usize::from(offset), 0);
        assert_eq!(offset.to_string(), "offset:0B");

        let span: ByteSpan = arena.live_bytes(1);
        assert_eq!(usize::from(span), Point::SLOT_SIZE);
        assert_eq!(span.to_string(), format!("span:{}B", Point::SLOT_SIZE));
    }

    #[test]
    fn arena_iter_yields_all_slots() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        arena.alloc(Point { x: 0, y: 0 }).unwrap();
        arena.alloc(Point { x: 0, y: 0 }).unwrap();
        arena.alloc(Point { x: 0, y: 0 }).unwrap();

        assert_eq!(arena.iter().count(), 3);

        let mut n = 0;
        for _slot in &arena {
            n += 1;
        }
        assert_eq!(n, 3);
    }

    #[test]
    fn arena_display() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        assert_eq!(arena.to_string(), "TypedArena[0/4]");
        arena.alloc(Point { x: 0, y: 0 }).unwrap();
        assert_eq!(arena.to_string(), "TypedArena[1/4]");
    }

    #[test]
    fn arena_slot_ptr_copy_and_equality() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        arena.alloc(Point { x: 0, y: 0 }).unwrap();

        let ptr1 = ArenaSlotPtr::<Point>::from_const(arena.buf_start()).unwrap();
        let ptr2 = ptr1;
        assert_eq!(ptr1, ptr2);
    }

    // ── Convenience accessors ─────────────────────────────────────────────

    #[test]
    fn first_and_last_return_none_on_empty_arena() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        assert!(arena.first().is_none());
        assert!(arena.last().is_none());
    }

    #[test]
    fn first_and_last_return_some_after_alloc() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        arena.alloc(Point { x: 0, y: 0 }).unwrap();
        arena.alloc(Point { x: 0, y: 0 }).unwrap();

        assert!(arena.first().is_some());
        assert!(arena.last().is_some());
        assert!(!core::ptr::eq(
            arena.first().unwrap(),
            arena.last().unwrap()
        ));
    }

    #[test]
    fn first_mut_and_last_mut_return_none_on_empty_arena() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        assert!(arena.first_mut().is_none());
        assert!(arena.last_mut().is_none());
    }

    #[test]
    fn buf_start_and_buf_end_span_backing_buffer() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        let start = arena.buf_start();
        let end = arena.buf_end();
        assert_eq!(end as usize - start as usize, arena.byte_len());
        assert_eq!(start, arena.start_ptr().0);
    }

    // ── Compile-time index path (nightly only) ────────────────────────────

    #[cfg(feature = "const-index")]
    #[test]
    fn const_idx_compile_time_access() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        arena.alloc(Point { x: 0, y: 0 }).unwrap();

        let idx = arena.const_idx::<0>();
        let archived = arena.get_const(idx).unwrap();
        assert_eq!(archived.x.to_native(), 0);
    }

    // ── Construction edge cases ───────────────────────────────────────────

    #[test]
    fn new_exact_size_buffer_works() {
        let needed = 4 * Point::SLOT_SIZE;
        let mut raw = vec![0u8; needed + 8];
        let offset = raw.as_ptr().align_offset(Point::SLOT_ALIGN);
        let aligned = &mut raw[offset..offset + needed];
        assert!(TypedArena::<Point, 4>::new(aligned).is_ok());
    }

    // ── Allocation correctness ────────────────────────────────────────────

    #[test]
    fn alloc_failure_does_not_increment_len() {
        let mut buf = AlignedBuf::<32>([0u8; 32]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        for i in 0..4u32 {
            arena.alloc(Point { x: i, y: i }).unwrap();
        }
        assert_eq!(arena.len(), 4);
        let _ = arena.alloc(Point { x: 99, y: 99 }).unwrap_err();
        assert_eq!(arena.len(), 4);
    }

    #[test]
    fn multiple_reset_and_realloc_cycles() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        for cycle in 0u32..3 {
            for i in 0..4u32 {
                arena
                    .alloc(Point {
                        x: cycle * 10 + i,
                        y: 0,
                    })
                    .unwrap();
            }
            assert_eq!(arena.len(), 4);
            arena.reset();
            assert_eq!(arena.len(), 0);
        }
        for _ in 0..4 {
            arena.alloc(Point { x: 0, y: 0 }).unwrap();
        }
        assert_eq!(arena.len(), 4);
    }

    #[test]
    fn get_returns_out_of_range_for_capacity_boundary() {
        let mut buf = AlignedBuf::<32>([0u8; 32]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        for i in 0..4u32 {
            arena.alloc(Point { x: i, y: i }).unwrap();
        }
        // Forge an in-brand, in-generation token for index 4: out of range
        // for a full 4-slot arena (only the range check can reject it).
        let forged = DynSlotIdx {
            index: 4,
            generation: arena.generation,
            brand: arena.brand,
            _marker: PhantomData,
        };
        assert_eq!(
            arena.get(forged).unwrap_err(),
            ArenaError::OutOfRange { idx: 4, len: 4 },
        );
    }

    // ── Mutation ──────────────────────────────────────────────────────────

    #[test]
    fn get_mut_mutation_visible_via_get() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        let idx = arena.alloc(Point { x: 1, y: 2 }).unwrap();

        assert!(core::ptr::eq(
            arena.get(idx).unwrap() as *const _,
            arena.get_mut(idx).unwrap() as *const _,
        ));

        {
            let slot = arena.get_mut(idx).unwrap();
            let raw: *mut [u8; 8] = slot as *mut _ as *mut [u8; 8];
            unsafe { *raw = [42, 0, 0, 0, 99, 0, 0, 0] };
        }

        let slot = arena.get(idx).unwrap();
        assert_eq!(slot.x.to_native(), 42);
        assert_eq!(slot.y.to_native(), 99);
    }

    #[test]
    fn first_mut_and_last_mut_return_some_and_are_writable() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        arena.alloc(Point { x: 10, y: 20 }).unwrap();
        arena.alloc(Point { x: 30, y: 40 }).unwrap();

        assert!(arena.first_mut().is_some());
        assert!(arena.last_mut().is_some());
        assert!(!core::ptr::eq(
            arena.first_mut().unwrap() as *const _,
            arena.last_mut().unwrap() as *const _,
        ));

        {
            let first = arena.first_mut().unwrap();
            let raw: *mut [u8; 4] = first as *mut _ as *mut [u8; 4];
            unsafe { *raw = [111, 0, 0, 0] };
        }
        assert_eq!(arena.first().unwrap().x.to_native(), 111);
    }

    // ── Token-based access (Index/IndexMut deliberately not implemented) ──

    #[test]
    fn token_read_via_get() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        let idx = arena.alloc(Point { x: 7, y: 8 }).unwrap();
        let slot = arena.get(idx).unwrap();
        assert_eq!(slot.x.to_native(), 7);
        assert_eq!(slot.y.to_native(), 8);
    }

    // ── Reset / Drop ──────────────────────────────────────────────────────

    #[test]
    fn reset_on_empty_arena_is_no_op() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        arena.reset();
        assert!(arena.is_empty());
        assert_eq!(arena.len(), 0);
    }

    #[test]
    fn as_slice_after_reset_is_empty() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        arena.alloc(Point { x: 1, y: 2 }).unwrap();
        arena.reset();
        assert_eq!(arena.as_slice().len(), 0);
    }

    #[test]
    fn buf_start_stable_across_alloc_and_reset() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        let before = arena.buf_start();
        arena.alloc(Point { x: 0, y: 0 }).unwrap();
        let after_alloc = arena.buf_start();
        arena.reset();
        let after_reset = arena.buf_start();
        assert_eq!(before, after_alloc);
        assert_eq!(before, after_reset);
    }

    #[test]
    fn drop_on_empty_arena_does_not_panic() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let _arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
    }

    #[test]
    fn secure_config_zeroes_all_allocated_slots_on_reset() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        {
            let mut arena = TypedArena::<Point, 4, SecureConfig>::new(&mut buf.0).unwrap();
            arena
                .alloc(Point {
                    x: 0xDEAD,
                    y: 0xBEEF,
                })
                .unwrap();
            arena
                .alloc(Point {
                    x: 0x1234,
                    y: 0x5678,
                })
                .unwrap();
            arena
                .alloc(Point {
                    x: 0xAAAA,
                    y: 0xBBBB,
                })
                .unwrap();
            arena.reset();
        }
        let zeroed_region = &buf.0[..3 * Point::SLOT_SIZE];
        assert!(zeroed_region.iter().all(|&b| b == 0));
    }

    #[test]
    fn secure_config_zeroes_all_allocated_slots_on_drop() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        {
            let mut arena = TypedArena::<Point, 4, SecureConfig>::new(&mut buf.0).unwrap();
            arena
                .alloc(Point {
                    x: 0xDEAD,
                    y: 0xBEEF,
                })
                .unwrap();
            arena
                .alloc(Point {
                    x: 0x1234,
                    y: 0x5678,
                })
                .unwrap();
            arena
                .alloc(Point {
                    x: 0xAAAA,
                    y: 0xBBBB,
                })
                .unwrap();
        }
        let zeroed_region = &buf.0[..3 * Point::SLOT_SIZE];
        assert!(zeroed_region.iter().all(|&b| b == 0));
    }

    // ── Iterator ─────────────────────────────────────────────────────────

    #[test]
    fn iter_on_empty_arena_yields_zero_items() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        assert_eq!(arena.iter().count(), 0);
    }

    #[test]
    fn iter_on_full_arena_yields_count_items() {
        let mut buf = AlignedBuf::<32>([0u8; 32]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        for i in 0..4u32 {
            arena.alloc(Point { x: i, y: i }).unwrap();
        }
        assert_eq!(arena.iter().count(), 4);
    }

    #[test]
    fn into_iter_mut_visits_all_slots() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        arena.alloc(Point { x: 0, y: 0 }).unwrap();
        arena.alloc(Point { x: 0, y: 0 }).unwrap();
        arena.alloc(Point { x: 0, y: 0 }).unwrap();

        let mut visited = 0usize;
        for slot in &mut arena {
            let raw: *mut [u8; 4] = slot as *mut _ as *mut [u8; 4];
            let i = visited as u32;
            unsafe { *raw = i.to_le_bytes() };
            visited += 1;
        }
        assert_eq!(visited, 3);

        for (i, slot) in arena.iter().enumerate() {
            assert_eq!(slot.x.to_native(), i as u32);
        }
    }

    #[test]
    fn into_iter_shared_can_be_used_twice() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        arena.alloc(Point { x: 1, y: 2 }).unwrap();
        arena.alloc(Point { x: 3, y: 4 }).unwrap();

        let first_pass: Vec<_> = (&arena).into_iter().collect();
        let second_pass: Vec<_> = (&arena).into_iter().collect();
        assert_eq!(first_pass.len(), second_pass.len());
        assert_eq!(first_pass[0].x.to_native(), second_pass[0].x.to_native());
    }

    // ── Bounds validation / ArenaSlotPtr ─────────────────────────────────

    #[test]
    fn validate_slot_ptr_accepts_first_slot() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        assert!(arena.validate_slot_ptr(arena.buf_start()).is_ok());
    }

    #[test]
    fn validate_slot_ptr_rejects_past_end() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        assert_eq!(
            arena.validate_slot_ptr(arena.buf_end()).unwrap_err(),
            ArenaError::OutOfBounds,
        );
    }

    #[test]
    fn validate_slot_ptr_rejects_misaligned() {
        if Point::SLOT_ALIGN <= 1 {
            return;
        }
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        let misaligned = unsafe { arena.buf_start().add(1) };
        assert_eq!(
            arena.validate_slot_ptr(misaligned).unwrap_err(),
            ArenaError::Misaligned,
        );
    }

    #[test]
    fn slot_mut_ptr_requires_exclusive_arena_access() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        arena.alloc(Point { x: 5, y: 6 }).unwrap();

        let raw = arena.buf_start();
        let write_ptr = arena.slot_mut_ptr(raw).unwrap();
        assert_eq!(
            write_ptr as *const _,
            arena.buf_start() as *const <Point as Archive>::Archived
        );
    }

    #[test]
    fn slot_mut_ptr_rejects_pointer_from_other_arena() {
        let mut buf_a = AlignedBuf::<64>([0u8; 64]);
        let mut buf_b = AlignedBuf::<64>([0u8; 64]);
        let mut arena_a = TypedArena::<Point, 4>::new(&mut buf_a.0).unwrap();
        let arena_b = TypedArena::<Point, 4>::new(&mut buf_b.0).unwrap();
        let foreign_raw = arena_b.buf_start();
        assert_eq!(
            arena_a.slot_mut_ptr(foreign_raw).unwrap_err(),
            ArenaError::OutOfBounds,
        );
    }

    #[test]
    fn contains_slot_overflow_on_wrapping_pointer() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        let wrapping: *const u8 = usize::MAX as *const u8;
        assert_eq!(
            arena.contains_slot(wrapping).unwrap_err(),
            ArenaError::Overflow,
        );
    }

    #[test]
    fn slot_offset_overflow_returns_error() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        let huge = usize::MAX / Point::SLOT_SIZE + 1;
        assert_eq!(arena.slot_offset(huge).unwrap_err(), ArenaError::Overflow);
    }

    #[test]
    fn span_overflow_returns_error() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        assert_eq!(
            arena.span(usize::MAX / Point::SLOT_SIZE + 1).unwrap_err(),
            ArenaError::Overflow,
        );
    }

    // ── Overwrite ─────────────────────────────────────────────────────────

    #[test]
    fn overwrite_replaces_slot_value() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        let idx = arena.alloc(Point { x: 1, y: 2 }).unwrap();

        arena.overwrite(idx, Point { x: 99, y: 88 }).unwrap();

        let slot = arena.get(idx).unwrap();
        assert_eq!(slot.x.to_native(), 99);
        assert_eq!(slot.y.to_native(), 88);
    }

    #[test]
    fn overwrite_returns_out_of_range_for_unallocated_idx() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        // Forge an in-brand token for index 0: out of range while len == 0.
        let forged = DynSlotIdx {
            index: 0,
            generation: arena.generation,
            brand: arena.brand,
            _marker: PhantomData,
        };
        assert_eq!(
            arena.overwrite(forged, Point { x: 0, y: 0 }).unwrap_err(),
            ArenaError::OutOfRange { idx: 0, len: 0 },
        );
    }

    #[test]
    fn overwrite_stale_idx_returns_stale_token() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        let idx = arena.alloc(Point { x: 1, y: 2 }).unwrap();
        arena.reset(); // bumps the generation
        assert_eq!(
            arena.overwrite(idx, Point { x: 0, y: 0 }).unwrap_err(),
            ArenaError::StaleToken,
        );
    }

    #[test]
    fn overwrite_does_not_affect_adjacent_slots() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        let i0 = arena.alloc(Point { x: 10, y: 11 }).unwrap();
        let i1 = arena.alloc(Point { x: 20, y: 21 }).unwrap();
        let i2 = arena.alloc(Point { x: 30, y: 31 }).unwrap();

        arena.overwrite(i1, Point { x: 99, y: 99 }).unwrap();

        assert_eq!(arena.get(i0).unwrap().x.to_native(), 10);
        assert_eq!(arena.get(i2).unwrap().x.to_native(), 30);
        assert_eq!(arena.get(i1).unwrap().x.to_native(), 99);
    }

    // ── Compile-time index path (nightly only) ────────────────────────────

    #[cfg(feature = "const-index")]
    #[test]
    fn overwrite_const_replaces_slot_value() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        arena.alloc(Point { x: 1, y: 2 }).unwrap();

        let idx = arena.const_idx::<0>();
        arena.overwrite_const(idx, Point { x: 77, y: 88 }).unwrap();

        let result = arena.get_const(arena.const_idx::<0>()).unwrap();
        assert_eq!(result.x.to_native(), 77);
        assert_eq!(result.y.to_native(), 88);
    }

    // ── split_unallocated_bytes ───────────────────────────────────────────

    #[test]
    fn split_unallocated_bytes_basic() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        arena.alloc(Point { x: 1, y: 2 }).unwrap();
        arena.alloc(Point { x: 3, y: 4 }).unwrap();

        let child_bytes = arena.split_unallocated_bytes(2).unwrap();
        assert_eq!(child_bytes.len(), 2 * Point::SLOT_SIZE);
    }

    #[test]
    fn split_unallocated_bytes_returns_full_when_insufficient() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        assert_eq!(
            arena.split_unallocated_bytes(5).unwrap_err(),
            ArenaError::Full
        );
    }

    #[test]
    fn split_unallocated_bytes_shrinks_parent_then_reset_restores_full_capacity() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        {
            let _child = arena.split_unallocated_bytes(2).unwrap();
            // While _child is live, parent's unallocated region is smaller.
            // Borrow checker prevents calling arena.alloc() here.
        }
        // After _child drops, reset() restores full COUNT capacity.
        arena.reset();
        for _ in 0..4 {
            arena.alloc(Point { x: 0, y: 0 }).unwrap();
        }
        assert_eq!(arena.len(), 4);
    }

    #[test]
    fn split_unallocated_bytes_adjacent_to_allocated_region() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        arena.alloc(Point { x: 0, y: 0 }).unwrap(); // slot 0 allocated

        // Capture the expected start of the carved region before borrowing the arena.
        let expected_ptr = unsafe { arena.buf_start().add(Point::SLOT_SIZE) };
        // The carved region starts immediately after the allocated slot.
        let child_bytes = arena.split_unallocated_bytes(1).unwrap();
        assert_eq!(child_bytes.as_ptr(), expected_ptr);
    }

    // ── ScopedArena ───────────────────────────────────────────────────────

    #[test]
    fn scope_alloc_and_get_roundtrip() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 8>::new(&mut buf.0).unwrap();
        let mut scope = arena.scope::<4>().unwrap();

        let idx = scope.alloc(Point { x: 7, y: 42 }).unwrap();
        let slot = scope.get(idx).unwrap();
        assert_eq!(slot.x.to_native(), 7);
        assert_eq!(slot.y.to_native(), 42);
    }

    #[test]
    fn scope_parent_unallocated_restored_after_scope_drop() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();
        {
            let _scope = arena.scope::<4>().unwrap();
            // While scope is live, parent has 0 unallocated slots.
        }
        // After scope drops, all 4 slots are returned.
        for i in 0..4u32 {
            arena.alloc(Point { x: i, y: i }).unwrap();
        }
        assert_eq!(arena.len(), 4);
    }

    #[test]
    fn scope_drop_applies_lifecycle_policy_secure_config() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        {
            let mut arena = TypedArena::<Point, 4, SecureConfig>::new(&mut buf.0).unwrap();
            let mut scope = arena.scope::<2>().unwrap();
            scope
                .alloc(Point {
                    x: 0xDEAD,
                    y: 0xBEEF,
                })
                .unwrap();
            // scope drops here — DropPolicy (Zeroize) should clear the allocated slot
        }
        // The first slot's bytes (inside the scope's region) must be zeroed.
        assert!(buf.0[..Point::SLOT_SIZE].iter().all(|&b| b == 0));
    }

    #[test]
    fn scope_reset_then_reuse() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 8>::new(&mut buf.0).unwrap();
        let mut scope = arena.scope::<4>().unwrap();

        scope.alloc(Point { x: 1, y: 2 }).unwrap();
        assert_eq!(scope.len(), 1);

        scope.reset();
        assert_eq!(scope.len(), 0);
        assert_eq!(scope.capacity(), 4);

        // After reset, can alloc again from the same scope.
        scope.alloc(Point { x: 3, y: 4 }).unwrap();
        assert_eq!(scope.len(), 1);
    }

    #[test]
    fn scope_returns_full_when_insufficient() {
        let mut buf = AlignedBuf::<16>([0u8; 16]);
        let mut arena = TypedArena::<Point, 2>::new(&mut buf.0).unwrap();
        assert_eq!(arena.scope::<3>().unwrap_err(), ArenaError::Full);
    }

    #[test]
    fn scope_recursive_nesting() {
        // Buffer big enough for 8 Points (64 bytes)
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 8>::new(&mut buf.0).unwrap();

        // Carve 4 slots into s1; parent now has 4 unallocated.
        let mut s1 = arena.scope::<4>().unwrap();

        // Carve 2 slots into s2 from s1; s1 now has 2 unallocated.
        let mut s2 = s1.scope::<2>().unwrap();
        s2.alloc(Point { x: 10, y: 20 }).unwrap();
        assert_eq!(s2.len(), 1);
        // s2 drops here: 2 slots returned to s1
        drop(s2);

        // s1 should have all 4 slots available again
        assert_eq!(s1.capacity(), 4);
        for i in 0..4u32 {
            s1.alloc(Point { x: i, y: i }).unwrap();
        }
        assert_eq!(s1.len(), 4);
        // s1 drops here: 4 slots returned to arena
        drop(s1);

        // arena can now use all 8 slots
        for i in 0..8u32 {
            arena.alloc(Point { x: i, y: i }).unwrap();
        }
        assert_eq!(arena.len(), 8);
    }

    #[test]
    fn scope_sequential_scopes_restore_same_region() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 4>::new(&mut buf.0).unwrap();

        let start1;
        {
            let scope = arena.scope::<2>().unwrap();
            start1 = scope.buf_start();
        }
        // Second sequential scope from the same parent.
        let scope2 = arena.scope::<2>().unwrap();
        let start2 = scope2.buf_start();
        drop(scope2);

        // Both scopes began at the same base address (same unallocated region).
        assert_eq!(start1, start2);
    }

    #[test]
    fn scope_display() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 8>::new(&mut buf.0).unwrap();
        let mut scope = arena.scope::<4>().unwrap();
        assert_eq!(scope.to_string(), "ScopedArena[0/4]");
        scope.alloc(Point { x: 0, y: 0 }).unwrap();
        assert_eq!(scope.to_string(), "ScopedArena[1/4]");
    }

    #[test]
    fn scope_token_read_via_get() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 8>::new(&mut buf.0).unwrap();
        let mut scope = arena.scope::<4>().unwrap();
        let idx = scope.alloc(Point { x: 55, y: 66 }).unwrap();
        assert_eq!(scope.get(idx).unwrap().x.to_native(), 55);
    }

    #[test]
    fn scope_rejects_parent_token() {
        // A parent token has the parent's brand; the scope must reject it
        // even though indices and lifetimes would otherwise line up.
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 8>::new(&mut buf.0).unwrap();
        let parent_idx = arena.alloc(Point { x: 1, y: 1 }).unwrap();
        let mut scope = arena.scope::<4>().unwrap();
        let _ = scope.alloc(Point { x: 2, y: 2 }).unwrap();
        assert_eq!(scope.get(parent_idx).unwrap_err(), ArenaError::ForeignArena);
    }

    // ── DynScopedArena tests ───────────────────────────────────────────────────

    #[test]
    fn dyn_scope_alloc_and_get_roundtrip() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 8>::new(&mut buf.0).unwrap();
        let mut dyn_scope = arena.dyn_scope(4);
        assert_eq!(dyn_scope.capacity(), 4);
        assert_eq!(dyn_scope.len(), 0);
        let idx = dyn_scope.alloc(Point { x: 10, y: 20 }).unwrap();
        assert_eq!(dyn_scope.len(), 1);
        let slot = dyn_scope.get(idx).unwrap();
        assert_eq!(slot.x.to_native(), 10);
        assert_eq!(slot.y.to_native(), 20);
    }

    #[test]
    fn dyn_scope_caps_at_available_space() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 8>::new(&mut buf.0).unwrap();
        // Request more than available: should cap at 8.
        let dyn_scope = arena.dyn_scope(100);
        assert_eq!(dyn_scope.capacity(), 8);
    }

    #[test]
    fn dyn_scope_zero_count_is_valid() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 8>::new(&mut buf.0).unwrap();
        let dyn_scope = arena.dyn_scope(0);
        assert_eq!(dyn_scope.capacity(), 0);
        assert_eq!(dyn_scope.len(), 0);
        assert!(dyn_scope.is_empty());
    }

    #[test]
    fn dyn_scope_restores_parent_on_drop() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 8>::new(&mut buf.0).unwrap();
        // Alloc 2 in the parent so parent unallocated = 6.
        arena.alloc(Point { x: 1, y: 2 }).unwrap();
        arena.alloc(Point { x: 3, y: 4 }).unwrap();
        {
            let mut dyn_scope = arena.dyn_scope(4);
            dyn_scope.alloc(Point { x: 9, y: 9 }).unwrap();
            // dyn_scope drops here, returning 4 slots to arena
        }
        // Parent had 6 unallocated; borrowed 4 (actual=4); now should have 6 again.
        for _ in 0..6 {
            arena.alloc(Point { x: 0, y: 0 }).unwrap();
        }
    }

    #[test]
    fn dyn_scope_full_returns_error() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 8>::new(&mut buf.0).unwrap();
        let mut dyn_scope = arena.dyn_scope(2);
        dyn_scope.alloc(Point { x: 1, y: 1 }).unwrap();
        dyn_scope.alloc(Point { x: 2, y: 2 }).unwrap();
        assert_eq!(dyn_scope.alloc(Point { x: 3, y: 3 }), Err(ArenaError::Full));
    }

    #[test]
    fn dyn_scope_get_out_of_range() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 8>::new(&mut buf.0).unwrap();
        let dyn_scope = arena.dyn_scope(4);
        let forged = DynSlotIdx {
            index: 0,
            generation: dyn_scope.generation,
            brand: dyn_scope.brand,
            _marker: PhantomData,
        };
        assert_eq!(
            dyn_scope.get(forged),
            Err(ArenaError::OutOfRange { idx: 0, len: 0 })
        );
    }

    #[test]
    fn dyn_scope_reset_then_reuse() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 8>::new(&mut buf.0).unwrap();
        let mut dyn_scope = arena.dyn_scope(4);
        dyn_scope.alloc(Point { x: 5, y: 6 }).unwrap();
        dyn_scope.alloc(Point { x: 7, y: 8 }).unwrap();
        assert_eq!(dyn_scope.len(), 2);
        dyn_scope.reset();
        assert_eq!(dyn_scope.len(), 0);
        assert_eq!(dyn_scope.capacity(), 4);
        let idx = dyn_scope.alloc(Point { x: 11, y: 12 }).unwrap();
        assert_eq!(dyn_scope.len(), 1);
        assert_eq!(dyn_scope.get(idx).unwrap().x.to_native(), 11);
    }

    #[test]
    fn dyn_scope_drop_applies_lifecycle_policy() {
        let mut buf = AlignedBuf::<128>([0u8; 128]);
        let mut arena = TypedArena::<Point, 16, SecureConfig>::new(&mut buf.0).unwrap();
        // Capture where the scope's slots will be.
        let scope_start: *const u8;
        {
            let mut dyn_scope = arena.dyn_scope(4);
            scope_start = dyn_scope.buf_start();
            dyn_scope
                .alloc(Point {
                    x: 0xDEAD_BEEF,
                    y: 0xCAFE_BABE,
                })
                .unwrap();
            // Drop zeroes via SecureConfig::DropPolicy.
        }
        let slot_bytes =
            unsafe { core::slice::from_raw_parts(scope_start, 4 * size_of::<ArchivedPoint>()) };
        assert!(
            slot_bytes.iter().all(|&b| b == 0),
            "SecureConfig should zero on drop"
        );
    }

    #[test]
    fn dyn_scope_recursive_nesting() {
        let mut buf = AlignedBuf::<128>([0u8; 128]);
        let mut arena = TypedArena::<Point, 16>::new(&mut buf.0).unwrap();
        {
            let mut outer = arena.dyn_scope(8);
            assert_eq!(outer.capacity(), 8);
            {
                let mut inner = outer.dyn_scope(4);
                assert_eq!(inner.capacity(), 4);
                inner.alloc(Point { x: 1, y: 2 }).unwrap();
                // inner drops, returning 4 to outer
            }
            assert_eq!(outer.len(), 0);
            // Outer should be able to alloc all 8 again after inner drop.
            for i in 0u32..8 {
                outer.alloc(Point { x: i, y: i }).unwrap();
            }
        }
        // All 16 parent slots restored after outer drops.
        for i in 0u32..16 {
            arena.alloc(Point { x: i, y: i }).unwrap();
        }
    }

    #[test]
    fn dyn_scope_from_scoped_arena_restores() {
        let mut buf = AlignedBuf::<128>([0u8; 128]);
        let mut arena = TypedArena::<Point, 16>::new(&mut buf.0).unwrap();
        let mut scope = arena.scope::<8>().unwrap();
        {
            let mut dyn_child = scope.dyn_scope(4);
            dyn_child.alloc(Point { x: 99, y: 100 }).unwrap();
            // dyn_child drops, returning 4 slots back to scope
        }
        // scope should now have all 8 unallocated again.
        for i in 0u32..8 {
            scope.alloc(Point { x: i, y: i }).unwrap();
        }
    }

    #[test]
    fn dyn_scope_const_child_scope() {
        let mut buf = AlignedBuf::<128>([0u8; 128]);
        let mut arena = TypedArena::<Point, 16>::new(&mut buf.0).unwrap();
        let mut dyn_scope = arena.dyn_scope(8);
        {
            let mut const_child = dyn_scope.scope::<4>().unwrap();
            let idx = const_child.alloc(Point { x: 42, y: 43 }).unwrap();
            assert_eq!(const_child.get(idx).unwrap().x.to_native(), 42);
            // const_child drops, returning 4 slots to dyn_scope
        }
        // dyn_scope has 8 slots again.
        for i in 0u32..8 {
            dyn_scope.alloc(Point { x: i, y: i }).unwrap();
        }
    }

    #[test]
    fn dyn_scope_display_format() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 8>::new(&mut buf.0).unwrap();
        let mut dyn_scope = arena.dyn_scope(4);
        assert_eq!(dyn_scope.to_string(), "DynScopedArena[0/4]");
        dyn_scope.alloc(Point { x: 1, y: 2 }).unwrap();
        assert_eq!(dyn_scope.to_string(), "DynScopedArena[1/4]");
    }

    #[test]
    fn dyn_scope_overwrite() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 8>::new(&mut buf.0).unwrap();
        let mut dyn_scope = arena.dyn_scope(4);
        let idx = dyn_scope.alloc(Point { x: 1, y: 2 }).unwrap();
        dyn_scope.overwrite(idx, Point { x: 99, y: 100 }).unwrap();
        assert_eq!(dyn_scope.get(idx).unwrap().x.to_native(), 99);
    }

    #[test]
    fn dyn_scope_iter() {
        let mut buf = AlignedBuf::<64>([0u8; 64]);
        let mut arena = TypedArena::<Point, 8>::new(&mut buf.0).unwrap();
        let mut dyn_scope = arena.dyn_scope(4);
        dyn_scope.alloc(Point { x: 1, y: 2 }).unwrap();
        dyn_scope.alloc(Point { x: 3, y: 4 }).unwrap();
        let xs: Vec<u32> = dyn_scope.iter().map(|p| p.x.to_native()).collect();
        assert_eq!(xs, vec![1u32, 3u32]);
    }

    #[test]
    fn complex_scoping() {
        let mut buf = AlignedBuf::<256>([0u8; 256]);
        let mut arena = TypedArena::<Point, 20>::new(&mut buf.0).unwrap();
        {
            let mut scoped = arena.scope::<4>().expect("Failed");
            for n in 0..4 {
                scoped.alloc(Point { x: n + 1, y: 2 * n }).unwrap();
            }
        }
    }
}
