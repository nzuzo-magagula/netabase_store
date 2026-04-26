// A DynSlotIdx<'arena> cannot outlive its arena's backing-buffer lifetime.
// Attempting to use it after the arena and its buffer are dropped is a compile error.

use netabase_arena::{TypedArena, DefaultConfig, DynSlotIdx};
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
    let escaped_idx: DynSlotIdx;
    {
        let mut buf = Buf([0u8; 64]);
        let mut arena = TypedArena::<Point, 4, DefaultConfig>::new(&mut buf.0).unwrap();
        arena.alloc(Point { x: 1, y: 2 }).unwrap();
        escaped_idx = arena.checked_idx(0).unwrap(); //~ ERROR cannot infer an appropriate lifetime
    }
    let _ = escaped_idx;
}
