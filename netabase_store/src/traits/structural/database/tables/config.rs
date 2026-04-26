//! Typestate insert configuration.
//!
//! Which auxiliary table categories an insert touches is a **compile-time
//! property**: the policy is a ZST type parameter carrying `const bool`s, so
//! generated orchestration code branches on constants and the dead paths are
//! eliminated at monomorphization. In particular, the content hash for
//! subscriptions is only computed when `P::SUBSCRIPTIONS` is true.
//!
//! ```ignore
//! // All categories, no subscriptions (the default):
//! let cfg = InsertConfig::new();
//! // Subscribe two topics and skip blob writes — all reflected in the type:
//! let cfg = InsertConfig::new()
//!     .subscriptions(&[topic_a, topic_b])
//!     .exclude_blob();
//! ```

use core::marker::PhantomData;

/// Compile-time insert policy: which categories are written.
pub trait InsertPolicy {
    /// Write subscription topics (and therefore compute the content hash).
    const SUBSCRIPTIONS: bool;
    /// Write secondary index tables.
    const SECONDARY: bool;
    /// Write relational tables.
    const RELATIONAL: bool;
    /// Write blob tables.
    const BLOB: bool;
    /// Run custom-table side effects.
    const CUSTOM: bool;
}

/// The default policy: every category except subscriptions.
pub struct InsertAll;
impl InsertPolicy for InsertAll {
    const SUBSCRIPTIONS: bool = false;
    const SECONDARY: bool = true;
    const RELATIONAL: bool = true;
    const BLOB: bool = true;
    const CUSTOM: bool = true;
}

/// Enable subscription writes on top of `P`.
pub struct WithSubscriptions<P>(PhantomData<P>);
impl<P: InsertPolicy> InsertPolicy for WithSubscriptions<P> {
    const SUBSCRIPTIONS: bool = true;
    const SECONDARY: bool = P::SECONDARY;
    const RELATIONAL: bool = P::RELATIONAL;
    const BLOB: bool = P::BLOB;
    const CUSTOM: bool = P::CUSTOM;
}

/// Disable secondary-index writes on top of `P`.
pub struct NoSecondary<P>(PhantomData<P>);
impl<P: InsertPolicy> InsertPolicy for NoSecondary<P> {
    const SUBSCRIPTIONS: bool = P::SUBSCRIPTIONS;
    const SECONDARY: bool = false;
    const RELATIONAL: bool = P::RELATIONAL;
    const BLOB: bool = P::BLOB;
    const CUSTOM: bool = P::CUSTOM;
}

/// Disable relational writes on top of `P`.
pub struct NoRelational<P>(PhantomData<P>);
impl<P: InsertPolicy> InsertPolicy for NoRelational<P> {
    const SUBSCRIPTIONS: bool = P::SUBSCRIPTIONS;
    const SECONDARY: bool = P::SECONDARY;
    const RELATIONAL: bool = false;
    const BLOB: bool = P::BLOB;
    const CUSTOM: bool = P::CUSTOM;
}

/// Disable blob writes on top of `P`.
pub struct NoBlob<P>(PhantomData<P>);
impl<P: InsertPolicy> InsertPolicy for NoBlob<P> {
    const SUBSCRIPTIONS: bool = P::SUBSCRIPTIONS;
    const SECONDARY: bool = P::SECONDARY;
    const RELATIONAL: bool = P::RELATIONAL;
    const BLOB: bool = false;
    const CUSTOM: bool = P::CUSTOM;
}

/// Disable custom-table side effects on top of `P`.
pub struct NoCustom<P>(PhantomData<P>);
impl<P: InsertPolicy> InsertPolicy for NoCustom<P> {
    const SUBSCRIPTIONS: bool = P::SUBSCRIPTIONS;
    const SECONDARY: bool = P::SECONDARY;
    const RELATIONAL: bool = P::RELATIONAL;
    const BLOB: bool = P::BLOB;
    const CUSTOM: bool = false;
}

/// Insert configuration: a borrowed subscription topic list plus a
/// compile-time [`InsertPolicy`]. No heap allocation, no runtime flags.
pub struct InsertConfig<'s, S, P: InsertPolicy = InsertAll> {
    /// Topics to register this insert under. Only consulted when
    /// `P::SUBSCRIPTIONS` is true.
    pub subscriptions: &'s [S],
    _policy: PhantomData<P>,
}

impl<S> InsertConfig<'static, S, InsertAll> {
    /// All categories written, no subscriptions.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            subscriptions: &[],
            _policy: PhantomData,
        }
    }
}

impl<S> Default for InsertConfig<'static, S, InsertAll> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'s, S, P: InsertPolicy> InsertConfig<'s, S, P> {
    /// Register the insert under `topics` (flips `SUBSCRIPTIONS` on in the
    /// type).
    #[must_use]
    pub fn subscriptions<'t>(self, topics: &'t [S]) -> InsertConfig<'t, S, WithSubscriptions<P>> {
        InsertConfig {
            subscriptions: topics,
            _policy: PhantomData,
        }
    }

    #[must_use]
    pub fn exclude_secondary(self) -> InsertConfig<'s, S, NoSecondary<P>> {
        InsertConfig {
            subscriptions: self.subscriptions,
            _policy: PhantomData,
        }
    }

    #[must_use]
    pub fn exclude_relational(self) -> InsertConfig<'s, S, NoRelational<P>> {
        InsertConfig {
            subscriptions: self.subscriptions,
            _policy: PhantomData,
        }
    }

    #[must_use]
    pub fn exclude_blob(self) -> InsertConfig<'s, S, NoBlob<P>> {
        InsertConfig {
            subscriptions: self.subscriptions,
            _policy: PhantomData,
        }
    }

    #[must_use]
    pub fn exclude_custom(self) -> InsertConfig<'s, S, NoCustom<P>> {
        InsertConfig {
            subscriptions: self.subscriptions,
            _policy: PhantomData,
        }
    }

    /// Rewrap for a child scope with a different subscription key type
    /// (routing descends the schema tree with per-level key enums), keeping
    /// the same compile-time policy.
    #[must_use]
    pub fn rekey<'t, S2>(&self, topics: &'t [S2]) -> InsertConfig<'t, S2, P> {
        InsertConfig {
            subscriptions: topics,
            _policy: PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_consts_fold_as_expected() {
        type P1 = InsertAll;
        assert!(!P1::SUBSCRIPTIONS);
        assert!(P1::SECONDARY && P1::RELATIONAL && P1::BLOB && P1::CUSTOM);

        type P2 = WithSubscriptions<NoBlob<InsertAll>>;
        assert!(P2::SUBSCRIPTIONS);
        assert!(!P2::BLOB);
        assert!(P2::SECONDARY && P2::RELATIONAL && P2::CUSTOM);

        type P3 = NoCustom<NoSecondary<InsertAll>>;
        assert!(!P3::CUSTOM && !P3::SECONDARY);
        assert!(P3::RELATIONAL && P3::BLOB);
    }

    #[test]
    fn builder_carries_topics() {
        let topics = [1u8, 2];
        let cfg = InsertConfig::new().subscriptions(&topics).exclude_blob();
        assert_eq!(cfg.subscriptions, &[1, 2]);
        fn policy_of<S, P: InsertPolicy>(_: &InsertConfig<'_, S, P>) -> (bool, bool) {
            (P::SUBSCRIPTIONS, P::BLOB)
        }
        assert_eq!(policy_of(&cfg), (true, false));
    }
}
