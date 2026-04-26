use core::fmt;

/// A value did not fit within a fixed capacity of `N`.
///
/// Returned by the fallible constructors and `try_*` methods of the fixed
/// types. Capacity exhaustion is an explicit, recoverable condition at the
/// edge of the system — never a panic and never a hidden reallocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapacityError {
    /// The fixed capacity that would have been exceeded.
    pub capacity: usize,
    /// The length the operation would have required.
    pub needed: usize,
}

impl fmt::Display for CapacityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "fixed capacity exceeded: needed {} but capacity is {}",
            self.needed, self.capacity
        )
    }
}

impl core::error::Error for CapacityError {}
