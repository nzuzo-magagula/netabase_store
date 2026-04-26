// Holding an ArenaSlotPtr from validate_slot_ptr should block arena.reset().
// ArenaSlotPtr<'_, T> borrows the arena immutably for '_; reset() takes &mut self.

use netabase_arena::{TypedArena, DefaultConfig};
use rkyv::{Archive, Serialize};

#[repr(align(8))]
struct Buf([u8; 64]);

#[derive(Archive, Serialize)]
#[rkyv(derive(Clone, Copy))]
struct Point { x: u32, y: u32 }

// SAFETY: #[repr(C)], two ArchivedU32 fields — no padding, all bytes defined,
// all-zero is a valid value, and the type is Copy.
unsafe impl rkyv::traits::NoUndef for ArchivedPoint {}
unsafe impl bytemuck::Zeroable for ArchivedPoint {}
unsafe impl bytemuck::Pod for ArchivedPoint {}

fn main() {
    let mut buf = Buf([0u8; 64]);
    let mut arena = TypedArena::<Point, 4, DefaultConfig>::new(&mut buf.0).unwrap();
    let ptr = arena.validate_slot_ptr(arena.buf_start()).unwrap();
    arena.reset(); //~ ERROR cannot borrow `arena` as mutable
    let _ = ptr;
}
