use crate::errors::NetabaseError;
use crate::keys::ordered::OrderedKeyEncoding;
use crate::traits::structural::database::NetabaseStore;
use crate::traits::structural::database::tables::auxiliary::AuxiliaryTables;
use crate::traits::structural::database::tables::auxiliary::relational::RelationalTables;
use crate::traits::structural::database::tables::auxiliary::secondary::SecondaryTables;
use crate::traits::structural::database::tables::auxiliary::subscription::SubscriptionTables;
use crate::traits::structural::database::tables::codec::StoreValue;
use crate::traits::structural::database::transactions::NetabaseTransaction;
use crate::traits::structural::database::transactions::model::{ModelReadOps, ModelWriteOps};
use crate::traits::structural::database::transactions::repository::{
    RepositoryReadTx, RepositoryWriteTx,
};
use crate::traits::structural::schema::definitions::NetabaseDefinition;
pub use crate::traits::structural::schema::models::NetabaseModelWithKeys;
use crate::traits::structural::schema::models::keys::{
    NetabaseDefinitionKeys, NetabaseModelKeys, NetabaseRepositoryKeys,
};
use crate::traits::structural::schema::repositories::NetabaseRepository;
use rkyv::rancor::Fallible;
use rkyv::{Place, Serialize};

pub trait TableOwner {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableStorageMode {
    /// One physical table per auxiliary abstraction, grouped semantically by the model.
    Sharded,
    /// One physical table per auxiliary *category* (e.g. `{Model}_Secondary`), keyed by the
    /// category's enum (`{Model}SecondaryKeys`). Point ops and range/prefix scans both work.
    Linear,
}
// Ordering note: every key encodes through `OrderedKeyEncoding` (leading
// variant tag byte for enums), so backend byte order *is* the typed order.
// A `range()` bounded by one variant is a contiguous prefix scan in both
// Sharded and Linear modes — no per-comparison decoding anywhere.

pub trait NodeStorageMode {
    const MODE: TableStorageMode;
}

/// Compile-time selector for how a model's auxiliary tables are physically laid out.
/// The layout is driven by the model's [`NodeStorageMode::MODE`]:
///
/// - [`ShardedDispatch`] — one physical table per secondary/relational field
///   (`{Model}_Secondary_{field}`, …), grouped by the model.
/// - [`LinearDispatch`] — one physical table per auxiliary category, keyed by the
///   category's enum (`{Model}_Secondary` holding `{Model}SecondaryKeys`, …). The
///   enum's ordered encoding leads with the variant tag, so redb/fjall/arena byte
///   order groups entries by field variant and a `range()` bounded by one variant
///   is a contiguous prefix scan over that category.
///
/// In both cases operations pass through the owned `{Model}{Secondary,Relational,Blob,
/// Subscription}Tables` structs; only the physical table names differ.
pub trait ModelTableDispatch {
    const MODE: TableStorageMode;
}

/// One physical table per auxiliary abstraction (the implemented dispatch).
pub struct ShardedDispatch;
impl ModelTableDispatch for ShardedDispatch {
    const MODE: TableStorageMode = TableStorageMode::Sharded;
}

/// Single enum-keyed physical table per category, with working point ops and contiguous
/// range/prefix scans over a category (ordering is the typed order via the key encoding).
pub struct LinearDispatch;
impl ModelTableDispatch for LinearDispatch {
    const MODE: TableStorageMode = TableStorageMode::Linear;
}

pub trait EnumStorageMode<
    R: NetabaseRepository,
    D: NetabaseDefinition<R>,
    M: NetabaseModelWithKeys<R, D>,
>: NodeStorageMode
{
}
pub trait ModelStorageMode<R: NetabaseRepository, D: NetabaseDefinition<R>>:
    NodeStorageMode
{
}
pub trait RepositoryStorageMode<R: NetabaseRepository>: NodeStorageMode {}

/// Physical table collection for a repository. Owns the orchestration logic that routes
/// operations down through its definition children. Repository::TABLES is the root of the
/// physical table tree.
pub trait RepositoryTables<R: NetabaseRepository>: Sized {
    type Config: TableConfig;

    fn orchestrate_insert<'db, DB: NetabaseStore<R>, P: super::config::InsertPolicy>(
        &self,
        txn: &mut impl RepositoryWriteTx<'db, R, DB>,
        item: R,
        config: &super::config::InsertConfig<
            '_,
            <R::Keys as NetabaseRepositoryKeys<R>>::SubscriptionKeys,
            P,
        >,
    ) -> Result<(), NetabaseError>
    where
        R: 'db;

    fn orchestrate_get<'db, DB: NetabaseStore<R>>(
        &self,
        txn: &impl RepositoryReadTx<'db, R, DB>,
        key: <R::Keys as NetabaseRepositoryKeys<R>>::PrimaryKey,
    ) -> Result<Option<R>, NetabaseError>
    where
        R: 'db;

    fn orchestrate_delete<'db, DB: NetabaseStore<R>>(
        &self,
        address: R::Address,
        key: <R::Keys as NetabaseRepositoryKeys<R>>::PrimaryKey,
        txn: &mut impl RepositoryWriteTx<'db, R, DB>,
    ) -> Result<(), NetabaseError>
    where
        R: 'db;
}

/// Physical table collection for a definition. Owns the orchestration logic that routes
/// operations down through its model children. Definition::TABLES is the locus for all
/// table access within a definition, reachable from the repository's table tree.
pub trait DefinitionTables<R: NetabaseRepository, D: NetabaseDefinition<R>>: Sized {
    type Config: TableConfig;

    fn orchestrate_insert<'db, DB: NetabaseStore<R>, P: super::config::InsertPolicy>(
        &self,
        txn: &mut impl RepositoryWriteTx<'db, R, DB>,
        item: D,
        config: &super::config::InsertConfig<
            '_,
            <D::Keys as NetabaseDefinitionKeys<R, D>>::SubscriptionKeys,
            P,
        >,
    ) -> Result<(), NetabaseError>
    where
        R: 'db;

    fn orchestrate_get<'db, DB: NetabaseStore<R>>(
        &self,
        txn: &impl RepositoryReadTx<'db, R, DB>,
        key: <D::Keys as NetabaseDefinitionKeys<R, D>>::PrimaryKey,
    ) -> Result<Option<D>, NetabaseError>
    where
        R: 'db;

    fn orchestrate_delete<'db, DB: NetabaseStore<R>>(
        &self,
        address: D::Address,
        key: <D::Keys as NetabaseDefinitionKeys<R, D>>::PrimaryKey,
        txn: &mut impl RepositoryWriteTx<'db, R, DB>,
    ) -> Result<(), NetabaseError>
    where
        R: 'db;

    #[cfg(feature = "std")]
    fn orchestrate_fetch_blob_indices<'db, DB: NetabaseStore<R>>(
        &self,
        txn: &impl RepositoryReadTx<'db, R, DB>,
        key: <D::Keys as NetabaseDefinitionKeys<R, D>>::PrimaryKey,
    ) -> Result<Vec<<D::Keys as NetabaseDefinitionKeys<R, D>>::BlobKeys>, NetabaseError>
    where
        R: 'db;

    #[cfg(feature = "std")]
    fn orchestrate_read_blob_chunks<'db, DB: NetabaseStore<R>>(
        &self,
        txn: &impl RepositoryReadTx<'db, R, DB>,
        indices: Vec<<D::Keys as NetabaseDefinitionKeys<R, D>>::BlobKeys>,
    ) -> Result<Vec<Vec<u8>>, NetabaseError>
    where
        R: 'db;
}

/// A table key: order-preserving byte encoding plus the value semantics the
/// routing layer needs. The encoding *is* the storage order on every backend.
pub trait TableKey: OrderedKeyEncoding + Ord + Clone + 'static {}
impl<T: OrderedKeyEncoding + Ord + Clone + 'static> TableKey for T {}

/// A table value: fixed-width canonical rkyv bytes (see
/// [`codec`](super::codec)).
pub trait TableValue: StoreValue + 'static {}
impl<T: StoreValue + 'static> TableValue for T {}

pub use super::codec::{
    access_value, deserialize_value, read_value, serialize_value_into, value_size,
};
pub use super::config::{InsertConfig, InsertPolicy};

/// A 32-byte content hash (blake3) of a value's canonical byte form.
///
/// Stored as a table value: archives to exactly its 32 bytes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModelHash(pub [u8; 32]);

impl rkyv::Archive for ModelHash {
    type Archived = [u8; 32];
    type Resolver = ();
    fn resolve(&self, _: (), out: Place<Self::Archived>) {
        out.write(self.0);
    }
}

impl<S: Fallible + ?Sized> Serialize<S> for ModelHash {
    fn serialize(&self, _: &mut S) -> Result<(), S::Error> {
        Ok(())
    }
}

impl<D: Fallible + ?Sized> rkyv::Deserialize<ModelHash, D> for [u8; 32] {
    fn deserialize(&self, _: &mut D) -> Result<ModelHash, D::Error> {
        Ok(ModelHash(*self))
    }
}

pub trait NetabaseHasher {
    fn hash(data: &[u8]) -> ModelHash;
}

pub struct Blake3Hasher;
impl NetabaseHasher for Blake3Hasher {
    fn hash(data: &[u8]) -> ModelHash {
        let hash = blake3::hash(data);
        ModelHash(*hash.as_bytes())
    }
}

pub trait TableConfig {
    fn table_name(&self) -> impl AsRef<str>;
}

impl TableConfig for () {
    #[allow(refining_impl_trait)]
    fn table_name(&self) -> &'static str {
        "default"
    }
}

impl TableConfig for &'static str {
    #[allow(refining_impl_trait)]
    fn table_name(&self) -> &'static str {
        self
    }
}

pub trait NetabaseModelAddress<
    R: NetabaseRepository,
    D: NetabaseDefinition<R>,
    M: NetabaseModelWithKeys<R, D>,
>
{
}
pub trait NetabaseDefinitionAddress<R: NetabaseRepository, D: NetabaseDefinition<R>> {}
pub trait NetabaseRepositoryAddress<R: NetabaseRepository> {}

/// Read operations over one table.
///
/// Reads are **zero-copy**: `get` returns a guard dereferencing to the
/// value's validated archived form inside the backend's storage. Owned reads
/// are a convenience layered on top.
pub trait TableReadOps<K: TableKey, V: TableValue> {
    /// Borrow of one stored value's validated archived form.
    type Guard<'g>: core::ops::Deref<Target = V::Archived>
    where
        Self: 'g;

    /// Ordered iteration over (decoded key, value guard) pairs.
    type Iter<'g>: Iterator<Item = Result<(K, Self::Guard<'g>), NetabaseError>>
    where
        Self: 'g;

    /// Point lookup.
    fn get<'g>(&'g self, key: &K) -> Result<Option<Self::Guard<'g>>, NetabaseError>;

    /// All values stored under `key` (multimap tables may yield several; a
    /// plain table yields zero or one).
    fn get_all<'g>(&'g self, key: &K) -> Result<Self::Iter<'g>, NetabaseError>;

    /// Ordered range scan. Ordering equals the keys' typed `Ord` (the order
    /// law of [`OrderedKeyEncoding`]).
    fn range<'g>(
        &'g self,
        range: impl core::ops::RangeBounds<K>,
    ) -> Result<Self::Iter<'g>, NetabaseError>;

    /// Owned point lookup.
    fn get_value(&self, key: &K) -> Result<Option<V>, NetabaseError>
    where
        V::Archived: rkyv::Deserialize<V, rkyv::api::low::LowDeserializer<rkyv::rancor::Error>>,
    {
        match self.get(key)? {
            Some(guard) => Ok(Some(super::codec::deserialize_value::<V>(&guard)?)),
            None => Ok(None),
        }
    }
}

/// Write operations over one table.
pub trait TableWriteOps<K: TableKey, V: TableValue>: TableReadOps<K, V> {
    /// Insert or replace (plain table) / add a pair (multimap table).
    fn insert(&mut self, key: &K, value: &V) -> Result<(), NetabaseError>;

    /// Remove a key (all its values on a multimap). Returns whether anything
    /// was removed.
    fn remove(&mut self, key: &K) -> Result<bool, NetabaseError>;

    /// Remove one specific (key, value) pair from a multimap table.
    fn remove_value(&mut self, key: &K, value: &V) -> Result<bool, NetabaseError> {
        let _ = (key, value);
        Err(NetabaseError::Unsupported(
            crate::errors::OpKind::Multimap,
        ))
    }
}

pub trait StoreTable<O: TableOwner, K: TableKey, V: TableValue> {
    type TableConfig: TableConfig;
}

pub trait ModelTable<
    R: NetabaseRepository,
    D: NetabaseDefinition<R>,
    M: NetabaseModelWithKeys<R, D>,
>
{
    type Key: TableKey;
    type Value: TableValue;
}

pub trait ModelTables<
    'db,
    R: NetabaseRepository + 'db,
    D: NetabaseDefinition<R>,
    M: NetabaseModelWithKeys<R, D>,
    DB: NetabaseStore<R>,
>: ModelReadOps<'db, R, D, M, DB> + ModelWriteOps<'db, R, D, M, DB>
{
    type Primary: PrimaryTable<R, D, M>;
    fn primary<'a>(tnx: &impl NetabaseTransaction<'a, R, DB>) -> Self::Primary;
    type Secondary: SecondaryTables<R, D, M>;
    fn secondary<'a>(txn: &impl NetabaseTransaction<'a, R, DB>) -> Self::Secondary;
    type Relational: RelationalTables<R, D, M>;
    fn relational<'a>(txn: &impl NetabaseTransaction<'a, R, DB>) -> Self::Relational;
    type Auxiliary: AuxiliaryTables<R, D, M>;
    fn all<'a>(txn: &impl NetabaseTransaction<'a, R, DB>) -> Self::Auxiliary;
    type Subscription: SubscriptionTables<R, D, M>;
    fn subscription<'a>(txn: &impl NetabaseTransaction<'a, R, DB>) -> Self::Subscription;

    type Custom: crate::traits::structural::database::tables::auxiliary::custom::CustomTables<R, D, M>;
    fn custom<'a>(txn: &impl NetabaseTransaction<'a, R, DB>) -> Self::Custom;

    /// The subscription key type for this model. Populated by InsertConfig to
    /// control which subscription tables receive writes during orchestrate_insert.
    type SubscriptionKey: Clone;

    // Orchestration methods
    fn orchestrate_insert<P: InsertPolicy>(
        &mut self,
        txn: &mut impl RepositoryWriteTx<'db, R, DB>,
        model: M,
        config: &InsertConfig<'_, Self::SubscriptionKey, P>,
    ) -> Result<(), NetabaseError>;
    fn orchestrate_delete(
        &mut self,
        txn: &mut impl RepositoryWriteTx<'db, R, DB>,
        key: <M::Keys as NetabaseModelKeys<R, D, M>>::PrimaryKey,
    ) -> Result<(), NetabaseError>;
    fn orchestrate_get(
        &mut self,
        txn: &impl RepositoryReadTx<'db, R, DB>,
        key: <M::Keys as NetabaseModelKeys<R, D, M>>::PrimaryKey,
    ) -> Result<Option<M>, NetabaseError>;

    #[cfg(feature = "std")]
    fn orchestrate_fetch_blob_indices(
        &mut self,
        txn: &impl RepositoryReadTx<'db, R, DB>,
        key: <M::Keys as NetabaseModelKeys<R, D, M>>::PrimaryKey,
    ) -> Result<Vec<<M::Keys as NetabaseModelKeys<R, D, M>>::BlobKeys>, NetabaseError>;

    #[cfg(feature = "std")]
    fn orchestrate_read_blob_chunks(
        &mut self,
        txn: &impl RepositoryReadTx<'db, R, DB>,
        indices: Vec<<M::Keys as NetabaseModelKeys<R, D, M>>::BlobKeys>,
    ) -> Result<Vec<Vec<u8>>, NetabaseError>;
}

pub trait PrimaryTable<
    R: NetabaseRepository,
    D: NetabaseDefinition<R>,
    M: NetabaseModelWithKeys<R, D>,
>: ModelTable<R, D, M>
{
    type TableConfig: TableConfig;
}

pub struct DefinitionTablesConfig<M> {
    pub models: M,
}

impl<M: TableConfig> TableConfig for DefinitionTablesConfig<M> {
    fn table_name(&self) -> impl AsRef<str> {
        self.models.table_name()
    }
}

pub struct RepositoryTablesConfig<D> {
    pub definitions: D,
}

impl<D: TableConfig> TableConfig for RepositoryTablesConfig<D> {
    fn table_name(&self) -> impl core::convert::AsRef<str> {
        self.definitions.table_name()
    }
}
