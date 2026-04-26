// @review [x]
use crate::traits::behavioural::TransactionHooks;
use crate::traits::structural::schema::definitions::NetabaseDefinition;
use crate::traits::structural::schema::models::keys::NetabaseModelKeys;
use crate::traits::structural::schema::models::keys::subscription::SubscriptionOwner;
use crate::traits::structural::schema::repositories::NetabaseRepository;

#[cfg(feature = "std")]
pub mod blob;
pub mod keys;

// Query(#model/id-3e8): Q[Tr(NetabaseModelWithKeys)] && Q[Tr(NetabaseModel)], "Why are these separate? Should they not be the same?"
// TODO(#model_keys/id-6e9c): C[Tr(ModelKeysEnum)], "Define a model-level enum aggregating primary/secondary/relational/blob/subscription keys for grouped-table mode."
// TODO(#model_keys/id-6ea1): V[Tr(NetabaseModelWithKeys)], "Clarify the separation from NetabaseModel so definition/repository routing doesn't require table access."

/// The base trait for models that have keys. Every model owns at least an empty subscription set,
/// which the derive macro always generates.
pub trait NetabaseModelWithKeys<R: NetabaseRepository, D: NetabaseDefinition<R>>:
    SubscriptionOwner<R> + TransactionHooks + Sized
{
    // Verify(#addresses/id-57c): V[Tr(NetabaseModelAddress)], "What makes this different from other addresses?"
    type Address: crate::traits::structural::database::tables::core::NetabaseModelAddress<R, D, Self>;
    type Keys: NetabaseModelKeys<R, D, Self>;

    fn primary_key(&self) -> <Self::Keys as NetabaseModelKeys<R, D, Self>>::PrimaryKey;
    fn get_primary_address() -> Self::Address;
}

/// The full model trait, extending NetabaseModelWithKeys to include tables.
pub trait NetabaseModel<R: NetabaseRepository, D: NetabaseDefinition<R>>:
    NetabaseModelWithKeys<R, D>
{
    // Resolved (#model_tables/id-m001, #tables/id-a30): `TABLES` is intentionally a zero-sized
    // typed dispatch token, not a live table collection. The macro generates
    // `type Tables = {Model}Tables<R, D, (), Mode>` where `Mode` is `ShardedDispatch` or
    // `LinearDispatch` (see `tables::core::ModelTableDispatch`), so the physical layout is visible
    // in the type. `TABLES = {Model}Tables(PhantomData)` carries no handles; the `ModelTables`
    // factory/orchestrate methods take a live `NetabaseTransaction` and open handles per operation
    // (redb/fjall handles are transaction-scoped). The owner struct thus owns the dispatch logic
    // and layout decision, which is the right shape for the intended no-parse RAM↔persistent
    // (ECS-allocator) translation.
    type Tables;

    const TABLES: Self::Tables;
}
