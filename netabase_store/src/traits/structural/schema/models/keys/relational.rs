//! Relational keys: lazily-resolved links between models.
//!
//! [`Relation`] **always archives as the target model's primary key** — the
//! related model itself lives in its own table and is rehydrated on read.
//! The `Item` variant (std-only) is an in-memory convenience for inserting a
//! nested model in one call; the orchestrator splits it out, and the stored
//! canonical byte form is identical to the `Key` form.

use core::fmt;
use core::hash;

use crate::traits::structural::schema::models::keys::NetabaseModelKeys;
use crate::traits::structural::{
    database::tables::TableKey,
    schema::{
        definitions::NetabaseDefinition,
        models::{NetabaseModel, NetabaseModelWithKeys},
        repositories::NetabaseRepository,
    },
};
use rkyv::rancor::Fallible;
use rkyv::{Archive, Place, Serialize};

/// The primary-key type of the relation's target model.
pub type RelationKey<ToR, ToD, ToM> =
    <<ToM as NetabaseModelWithKeys<ToR, ToD>>::Keys as NetabaseModelKeys<ToR, ToD, ToM>>::PrimaryKey;

/// A link to a model, possibly in another definition or repository.
///
/// Logically this is always "the target's primary key"; comparison, hashing,
/// and the archived byte form all go through the key.
pub enum Relation<ToR, ToD, ToM>
where
    ToR: NetabaseRepository,
    ToD: NetabaseDefinition<ToR>,
    ToM: NetabaseModel<ToR, ToD>,
{
    /// The unresolved link: just the target's primary key.
    Key(RelationKey<ToR, ToD, ToM>),
    /// A not-yet-inserted target carried inline (host-side convenience).
    #[cfg(feature = "std")]
    Item(Box<ToM>),
}

impl<ToR, ToD, ToM> Relation<ToR, ToD, ToM>
where
    ToR: NetabaseRepository,
    ToD: NetabaseDefinition<ToR>,
    ToM: NetabaseModel<ToR, ToD>,
{
    /// The effective target key, whichever variant this is.
    pub fn key(&self) -> RelationKey<ToR, ToD, ToM> {
        match self {
            Self::Key(k) => k.clone(),
            #[cfg(feature = "std")]
            Self::Item(m) => m.primary_key(),
        }
    }
}

impl<ToR, ToD, ToM> Clone for Relation<ToR, ToD, ToM>
where
    ToR: NetabaseRepository,
    ToD: NetabaseDefinition<ToR>,
    ToM: NetabaseModel<ToR, ToD> + Clone,
{
    fn clone(&self) -> Self {
        match self {
            Self::Key(k) => Self::Key(k.clone()),
            #[cfg(feature = "std")]
            Self::Item(m) => Self::Item(m.clone()),
        }
    }
}

impl<ToR, ToD, ToM> fmt::Debug for Relation<ToR, ToD, ToM>
where
    ToR: NetabaseRepository,
    ToD: NetabaseDefinition<ToR>,
    ToM: NetabaseModel<ToR, ToD>,
    RelationKey<ToR, ToD, ToM>: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Key(k) => f.debug_tuple("Relation::Key").field(k).finish(),
            #[cfg(feature = "std")]
            Self::Item(m) => f
                .debug_tuple("Relation::Item")
                .field(&m.primary_key())
                .finish(),
        }
    }
}

// Equality/order/hash are on the effective key, so `Key(k)` and `Item(m)`
// with `m.primary_key() == k` are equal — matching the stored byte form.
impl<ToR, ToD, ToM> PartialEq for Relation<ToR, ToD, ToM>
where
    ToR: NetabaseRepository,
    ToD: NetabaseDefinition<ToR>,
    ToM: NetabaseModel<ToR, ToD>,
{
    fn eq(&self, other: &Self) -> bool {
        self.key() == other.key()
    }
}
impl<ToR, ToD, ToM> Eq for Relation<ToR, ToD, ToM>
where
    ToR: NetabaseRepository,
    ToD: NetabaseDefinition<ToR>,
    ToM: NetabaseModel<ToR, ToD>,
{
}
impl<ToR, ToD, ToM> PartialOrd for Relation<ToR, ToD, ToM>
where
    ToR: NetabaseRepository,
    ToD: NetabaseDefinition<ToR>,
    ToM: NetabaseModel<ToR, ToD>,
{
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl<ToR, ToD, ToM> Ord for Relation<ToR, ToD, ToM>
where
    ToR: NetabaseRepository,
    ToD: NetabaseDefinition<ToR>,
    ToM: NetabaseModel<ToR, ToD>,
{
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.key().cmp(&other.key())
    }
}
impl<ToR, ToD, ToM> hash::Hash for Relation<ToR, ToD, ToM>
where
    ToR: NetabaseRepository,
    ToD: NetabaseDefinition<ToR>,
    ToM: NetabaseModel<ToR, ToD>,
    RelationKey<ToR, ToD, ToM>: hash::Hash,
{
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.key().hash(state);
    }
}

impl<ToR, ToD, ToM> Default for Relation<ToR, ToD, ToM>
where
    ToR: NetabaseRepository,
    ToD: NetabaseDefinition<ToR>,
    ToM: NetabaseModel<ToR, ToD>,
    RelationKey<ToR, ToD, ToM>: Default,
{
    fn default() -> Self {
        Self::Key(Default::default())
    }
}

// ── rkyv: the canonical byte form is the target's primary key ───────────────

impl<ToR, ToD, ToM> Archive for Relation<ToR, ToD, ToM>
where
    ToR: NetabaseRepository,
    ToD: NetabaseDefinition<ToR>,
    ToM: NetabaseModel<ToR, ToD>,
    RelationKey<ToR, ToD, ToM>: Archive<Resolver = ()>,
{
    type Archived = <RelationKey<ToR, ToD, ToM> as Archive>::Archived;
    type Resolver = ();

    fn resolve(&self, _: (), out: Place<Self::Archived>) {
        self.key().resolve((), out);
    }
}

impl<S, ToR, ToD, ToM> Serialize<S> for Relation<ToR, ToD, ToM>
where
    S: Fallible + ?Sized,
    ToR: NetabaseRepository,
    ToD: NetabaseDefinition<ToR>,
    ToM: NetabaseModel<ToR, ToD>,
    RelationKey<ToR, ToD, ToM>: Archive<Resolver = ()> + Serialize<S>,
{
    fn serialize(&self, serializer: &mut S) -> Result<(), S::Error> {
        self.key().serialize(serializer)
    }
}

// NOTE: no generic `Deserialize<Relation, _> for PK::Archived` impl — the
// archived key type is an uncovered associated type, so such a blanket impl
// would overlap foreign impls. Generated code deserializes the concrete key
// and wraps it in `Relation::Key`.

pub trait RelationalKeysEnum<
    R: NetabaseRepository,
    D: NetabaseDefinition<R>,
    M: NetabaseModelWithKeys<R, D>,
>
{
}

pub trait RelationalKey<
    R: NetabaseRepository,
    D: NetabaseDefinition<R>,
    M: NetabaseModelWithKeys<R, D>,
>: TableKey
{
    type ParentEnum: RelationalKeysEnum<R, D, M>;
    type ToModel<TR: NetabaseRepository, TD: NetabaseDefinition<TR>>: NetabaseModel<TR, TD>;
}
