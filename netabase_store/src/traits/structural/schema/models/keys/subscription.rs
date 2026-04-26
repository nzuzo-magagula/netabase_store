// @review [~]
use crate::traits::structural::{
    database::tables::{
        TableKey, TableOwner,
        core::{Blake3Hasher, ModelHash, NetabaseHasher},
    },
    schema::repositories::NetabaseRepository,
};
// Subscription keying & hierarchy (resolves #subscription/id-1f4, id-1f6, id-s001, id-s002):
//
// Subscriptions are a *container-level* concept declared at three levels via the
// `subscriptions(...)` attribute on a model/definition/repository. Each level generates a pair
// of sibling enums:
//   - `{X}SubscriptionKeys` — the routing enum. It carries this level's own subscription topics
//     plus one nested variant per child (`Model(...)` for a definition, `Definition(...)` for a
//     repository). It implements `NetabaseSerialize`/`NetabaseDeserialize` with a leading u32
//     variant tag, so a stored subscription key round-trips back to the exact variant — mirroring
//     the primary-key enums. This is the `SubscriptionKeysEnum` for the level.
//   - `{X}SubscriptionRegistry` — an enum with one variant per registered child, each carrying
//     that child's content [`ModelHash`] (see [`SubscriptionRegistry`]). The set of registry
//     entries is compared between nodes via [`SubscriptionRegistry::merkle_root`] to detect
//     divergence. A definition's entry tags a model's hash by model; a repository's entry tags a
//     definition's (leaf) hash by definition.
//
// `SubscriptionOwner<R>` is required *uniformly* as a supertrait of `NetabaseModelWithKeys`,
// `NetabaseDefinition`, and `NetabaseRepository` — model-level subscriptions are not opt-in; the
// derive macro always generates the (possibly empty) subscription enums. (Resolves the former
// asymmetry described in id-s001.)
//
// `SubscriptionTables::orchestrate_insert` takes a [`ModelHash`] because subscriptions store the
// model's content hash (not the model body) as the value written to a subscription table — this
// is what enables content-based change detection / merkle comparison. The hash is generated
// inside the model orchestrator from the serialized model, not supplied by the user. Secondary and
// relational tables don't take it because they index keys, not content. (Resolves id-s002.)
pub trait SubscriptionOwner<R: NetabaseRepository>: TableOwner {
    type SubscriptionsEnum: SubscriptionKeysEnum<R, Self>;
}

pub trait SubscriptionKeysEnum<R: NetabaseRepository, O: SubscriptionOwner<R> + ?Sized> {}

pub trait SubscriptionKey<R: NetabaseRepository, O: SubscriptionOwner<R> + ?Sized>:
    TableKey
{
}

/// A subscription registry: an enum with one variant per registered child (one per
/// model for a definition, one per definition for a repository), each carrying that
/// member's content [`ModelHash`]. This is the "enum of model hashes, enumerated by
/// child" — the structure two nodes compare (via [`SubscriptionRegistry::merkle_root`])
/// to detect divergence over a decentralised network.
///
/// A model "subscribes" by being inserted through the definition-wrapped enum, which
/// can produce its registry entry. Full network reconciliation is deferred; this trait
/// defines the registry structure and hashing surface only.
pub trait SubscriptionRegistry: Sized {
    /// The content hash carried by this single registration.
    fn member_hash(&self) -> ModelHash;

    /// Collect the per-member hashes from a set of registrations.
    #[cfg(feature = "std")]
    fn registered_members(members: &[Self]) -> Vec<ModelHash> {
        members.iter().map(|m| m.member_hash()).collect()
    }

    /// Order-independent root hash over a set of registrations, used for merkle
    /// comparison between nodes. (Sorts member hashes so the root is independent of
    /// insertion order; a full incremental merkle tree is deferred.)
    #[cfg(feature = "std")]
    fn merkle_root(members: &[Self]) -> ModelHash {
        let mut hashes: Vec<[u8; 32]> = members.iter().map(|m| m.member_hash().0).collect();
        hashes.sort_unstable();
        let mut buf = Vec::with_capacity(hashes.len() * 32);
        for h in hashes {
            buf.extend_from_slice(&h);
        }
        Blake3Hasher::hash(&buf)
    }
}
