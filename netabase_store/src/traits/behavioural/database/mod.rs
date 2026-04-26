// @review [ ]
/// Compatibility facade for transaction traits.
///
/// The structural layer is the single source of truth for transaction trait
/// definitions. Behavioural keeps this re-export so downstream imports stay
/// stable.
pub use crate::traits::structural::database::transactions;
