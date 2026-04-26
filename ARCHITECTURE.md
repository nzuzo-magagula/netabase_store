# Netabase Technical Architecture

Netabase is a **compile-time-validated data abstraction** over a key-value store (redb, with
fjall/memory backends). It models data as a three-tier tree and generates all storage plumbing from
proc-macros, so the type system enforces the schema's structure and routing.

## Crates

- `netabase_macros` — the proc-macro pipeline. Three stages: **visitors** (extract attributes/fields
  from the AST) → **planners** (build an intermediate plan IR) → **generators** (emit the storage
  types, key enums, and trait impls). Entry points: `#[derive(NetabaseModel)]` / `#[netabase_model]`,
  `#[netabase_definition]`, `#[netabase_repository]`, `#[derive(NetabaseBlob)]`.
- `netabase_store` — the runtime: the trait hierarchy, key/codec traits, and the redb/fjall/memory
  backends. No generated code lives here.

## The three-tier tree: Model → Definition → Repository

- **Model** — a unit of data with a primary key. Owns a set of auxiliary tables (below). Trait:
  `NetabaseModel<R, D>` (extends `NetabaseModelWithKeys<R, D>`).
- **Definition** — a semantic grouping of related models; generated as an enum with one variant per
  model. Aggregates its models' keys. Trait: `NetabaseDefinition<R>`.
- **Repository** — the root; an enum with one variant per definition. Routes operations downward via
  static dispatch. Trait: `NetabaseRepository`.

Operations are routed top-down (`route_insert`/`route_get`/`route_delete` on the repository enum) so
the transaction scope is always correct and cross-definition/cross-repository relations are dispatched
only at the repository layer — never below a model.

## Auxiliary tables (per model)

Each model owns five auxiliary table abstractions, each with its own trait in
`tables/auxiliary/`:

- **Primary** — `K → V`; the main store, keyed by the model's primary key.
- **Secondary** — index tables for property lookups (multimap-friendly): query → primary keys.
- **Relational** — related-model lookups, keyed by route; toggles between single-KV and multimap.
- **Blob** — large data, chunked (whole-value or field-level strategy via `#[derive(NetabaseBlob)]`).
- **Subscription** — change tracking for decentralised sync (see below).

### The model-table owner struct & physical-layout toggle

Each model generates a zero-sized owner struct `{Model}Tables<R, D, DB, Mode>` that **owns the
dispatch logic and the physical-layout decision**, not live table handles (redb/fjall handles are
transaction-scoped and are opened per operation). The `Mode` type parameter
(`tables::core::ModelTableDispatch`) makes the layout visible in the type:

- **`ShardedDispatch`** (default) — one physical table per auxiliary abstraction, grouped by model
  (`{Model}_Secondary_{field}`, …). Implemented.
- **`LinearDispatch`** — one physical table per auxiliary *category*, keyed by the category's enum
  (`{Model}_Secondary` holding `{Model}SecondaryKeys`). Point ops **and** contiguous prefix/range
  scans both work: the ordered key encoding leads with the category's variant tag byte, so every
  backend orders entries by category and a range bounded by one variant is a contiguous prefix scan
  (the former "blocked on `Key::compare` byte order" concern is resolved by `OrderedKeyEncoding` —
  see the heap-free data-plane section below).

The mode is chosen at compile time from the `storage(...)` attribute and surfaced via
`AuxiliaryStorageMode` per-category consts; physical table names resolve through the generated
`{Model}TableName` / `{Model}SecondaryTable` / `{Model}RelationalTable` enums (single source of
truth).

**Storage-mode naming.** `sharded` is the default. `linear` is the canonical name for the
single-table-per-category mode; **`grouped` is an accepted legacy alias for `linear`** (both map to
`LinearDispatch`). The acceptance points are `model_generator.rs` (`is_linear`) and
`definition_generator.rs` / `repository_generator.rs`. Prefer `linear` in new schemas.

**Pure shard.** A model marked `#[netabase(pure)]` stores a skeleton in the Primary table with its
blob and relational fields stripped (the auxiliary tables are the source of truth); `get` rehydrates
them. This is surfaced by the `BLOB_DEDUP` / `RELATIONAL_DEDUP` consts on `AuxiliaryStorageMode` and
is implemented (see `pure_shard_tests.rs`). It is additive — it changes which fields the Primary
value carries without changing the operation surface.

## Subscriptions & the merkle registry

Subscriptions are a **container-level** concept declared with `subscriptions(...)` at the model,
definition, and repository levels (there is no field-level subscription). Each level generates a pair
of sibling enums:

- `{X}SubscriptionKeys` — the routing enum: this level's own topics plus one nested variant per child
  (`Model(...)` for a definition, `Definition(...)` for a repository). Variant-tagged
  serialize/deserialize lets a stored key round-trip back to its exact variant.
- `{X}SubscriptionRegistry` — an enum with one variant per registered child, each carrying that
  child's content `ModelHash`. The set of entries is compared between nodes via
  `SubscriptionRegistry::merkle_root` (order-independent) to detect divergence over the network.

`SubscriptionOwner<R>` is a uniform supertrait at all three levels (the derive always generates the,
possibly empty, subscription enums). A subscription table stores the model's content hash as its
value, which is what enables content-based change detection.

### Writing a subscription is gated by the wrapping enum

A subscription write is **gated by the enum of the scope being written to**: a model can only
register into a definition's subscription by being inserted *wrapped* in that definition
(`Definition::Model(model)`), and a definition only registers into a repository's subscription when
inserted as `Repository::Definition(def)`. This makes scope → operational access explicit. Concretely,
during `orchestrate_insert`:

- **Model own topics** (`{Model}_{topic}`) are keyed by the model's `PrimaryKey` → `ModelHash`.
- **Definition own topics** (`{Definition}_{topic}`) are keyed by the wrapped `DefinitionPrimaryKey`
  → `ModelHash`, built locally from the inserted item.
- **Repository own topics** (`{Repository}_{topic}`) are keyed by the wrapped `RepositoryPrimaryKey`
  → `ModelHash`.

Each own-topic write is gated on the corresponding topic being present in the `InsertConfig`'s
routed `subscriptions` for that level, so a level only records the topics actually requested. The
value is always the wrapped value's content hash, so the merkle registry (which compares values) is
unaffected by the key change. The `DefinitionPrimaryKey` / `RepositoryPrimaryKey` enums implement
`redb::Key`/`Value` (via their variant-tagged codec) so they can serve as table keys.

The `subscribe(...)` attribute is a **declaration only**: because a child cannot write into a scope
it is not wrapped in, no child→parent subscription write is emitted from it. (A cross-scope
subscription mechanism, if needed, is future work.)

## Custom tables

A model may declare `#[netabase(custom_table(Name, KeyTy, ValueTy))]` and implement
`CustomTableSideEffects<R, D, Model>` to run arbitrary `on_insert` / `on_delete` / `on_get` logic
during orchestration (honoring the `exclude_custom` `InsertConfig` flag). Declaring a custom table
**suppresses the generated default no-op impl** so the user-supplied impl is used; with no custom
table declared, custom side-effects remain a no-op. See `custom_table_tests.rs`.

## TOML schema (future)

A planned feature is to describe the repository/definition/model tree in TOML and rebuild the trait
impls from it via a companion macro. The round-trip contract (which table names, key types, and
relationships go into TOML, and how the macro reads them back) is not yet defined. The
`traits/structural/config.rs` module is reserved for it; the former dead `NetabaseConfig` stub was
removed in favour of designing the config/round-trip traits alongside the actual implementation.

---

# Appendix: Example Domain Model (Decentralized Commerce Network)

This document outlines the structural hierarchy of the decentralized commerce network. Each level of the hierarchy (Repository -> Definition -> Model) serves as a "gate" where business rules and cryptographic permissions are compiled.

## 1. GlobalDirectory (Repository)
**Status:** Public Shared State.
**Permission Gate:**
- **Read:** Ungranular (Public).
- **Write:** Consensus-gated (Protocol multi-sig or DAO).
- **Decentralized Logic:** Data is replicated to all nodes to ensure standardized lookups.

### Taxonomy (Definition)
- **Category**: Universal product categorization (e.g., Electronics, Fashion).
- **StandardUnit**: Globally accepted units of measure (e.g., KG, Meter, Piece).

### Financials (Definition)
- **Currency**: Supported network assets (Fiat-pegged stablecoins or native tokens).
- **ExchangeRate**: Oracle-driven relative values.

---

## 2. MerchantNode (Repository)
**Status:** Tenant-Isolated Data.
**Permission Gate:**
- **Read:** Public (for catalog browsing).
- **Write:** Tenant-gated (Merchant's private key only).
- **Decentralized Logic:** Stored on the merchant's local node or a pinning service.

### Catalog (Definition)
- **Product**: Merchant-specific listings. Relates to `GlobalDirectory::Taxonomy`.
- **Inventory**: Real-time stock availability.

### Logistics (Definition)
- **Warehouse**: Physical distribution points.
- **ShippingMethod**: Supported fulfillment routes.

---

## 3. ConsumerWallet (Repository)
**Status:** Private Encrypted State.
**Permission Gate:**
- **Read:** User-gated (Owner's private key).
- **Write:** User-gated (Owner's private key).
- **Decentralized Logic:** Stored locally on the user's device. Shared selectively via zero-knowledge proofs.

### Identity (Definition)
- **Profile**: Basic user persona.
- **VerifiedCredential**: Cryptographic proofs of identity or status.

### Vault (Definition)
- **SavedPaymentMethod**: Encrypted tokens for checkout.
- **PrivateAddress**: Securely stored physical locations.

---

## 4. TransactionEscrow (Repository)
**Status:** Multi-Party Shared State.
**Permission Gate:**
- **Read:** Involved-parties-gated (Buyer, Seller, Escrow Agent).
- **Write:** State-machine-gated (Transitions require signatures from specific roles).
- **Decentralized Logic:** Resides on a temporary shared sub-net or a specific execution layer.

### Commerce (Definition)
- **Order**: The binding agreement between Buyer and Seller.
- **OrderItem**: Specific lines relating back to the `MerchantNode`.

### Settlement (Definition)
- **PaymentIntent**: Funds locked in escrow.
- **Dispute**: Resolution state for failed transactions.

---

# Heap-Free, Compile-Time-Bounded Data Plane (refactor)

The store was reworked so the **core data plane is provably heap-free** and the safety burden sits
with the compiler. The volatile tier (`netabase_arena`) is now integrated; dynamic model types are
gone; one canonical byte form is shared across every backend.

## Crate layout & the no_std split

- `netabase_arena` — `#![no_std]`, `#![deny(unsafe_op_in_unsafe_fn)]`. The fixed-capacity memory
  tier: `TypedArena`/`ScopedArena`/`DynScopedArena` (bump arenas over `&mut [u8]`) plus the
  `fixed` module (below). All `unsafe` is centralized and Miri-clean.
- `netabase_store` — `#![cfg_attr(not(feature = "std"), no_std)]`. The trait core, key encoding,
  value codec, and the `ArenaStore` backend are heap-free and build with
  `--no-default-features --features arena-store`. redb/fjall and host conveniences are `std`-gated.

## Fixed-capacity model types (`netabase_arena::fixed`)

Dynamic std types are **rejected at macro time** with a diagnostic naming the replacement:

| Heap type            | Fixed replacement   |
|----------------------|---------------------|
| `String`             | `NbString<N>`       |
| `Vec<T>`             | `NbVec<T, N>`       |
| `BTreeMap`/`HashMap` | `NbMap<K, V, N>`    |
| `Option<T>`          | `NbOption<T>`       |

Each has a **fixed-width, align-1, zero-padding archived form** (`rkyv::Portable + NoUndef`, via the
workspace-wide `unaligned` rkyv layout), so a value serializes to a canonical byte string of a
compile-time-known size and lives directly in an arena slot, a redb/fjall value, or an IPC buffer.
A **canonical-tail invariant** (unused capacity holds `Default`) makes equal values byte-identical,
so content hashes are stable. Capacity overflow is an explicit `CapacityError` at the edge — never a
panic, never a reallocation. (`netabase_macros` `validators::type_policy` is the diagnostic layer;
the actual enforcement is the `StoreValue`/`OrderedKeyEncoding` bounds on generated code.)

## One canonical codec; order-preserving keys

- **Values**: a single rkyv codec (`tables/codec.rs`). `StoreValue` has a fixed-width archived form;
  `access_value` returns a **validated zero-copy** `&V::Archived` (corruption → `Codec(Corruption)`
  error, never a panic); the same bytes are stored in arena slots and redb/fjall values.
- **Keys**: `OrderedKeyEncoding` (`keys/ordered.rs`) — a prefix-free, **memcmp-comparable** byte
  encoding where `a.cmp(b) == encode(a).cmp(encode(b))`. Every backend orders entries by raw bytes,
  so the b-tree/LSM comparator is `a.cmp(b)`: **zero decode, zero allocation, no panic** in the hot
  path. A leading variant tag makes each enum-key category a contiguous range. The order/round-trip
  laws are property-tested for every primitive and every generated key.

## Backends & feature matrix

All four implement the same `NetabaseStore` trait and pass one **differential contract suite**
(`tests/backend_contract.rs`): point ops, ordered range == typed `Ord`, multimap set semantics,
atomic commit, drop-rollback.

| Backend       | Feature         | Tier        | Resource         | Transactions                          |
|---------------|-----------------|-------------|------------------|---------------------------------------|
| `MemoryStore` | `std`           | volatile    | `()`             | `BTreeMap` snapshot + staged commit   |
| `RedbStore`   | `redb-backend`  | persistent  | `PathBuf`        | redb ACID                             |
| `FjallStore`  | `fjall-backend` | persistent  | `PathBuf`        | fjall single-writer; composed-key multimaps |
| `ArenaStore`  | `arena-store`   | volatile    | `&'buf mut [u8]` | shadow-copy: stage in work half, atomic commit |

`open(resource)` is resource-generic; **`write_transaction(&mut self)`** makes single-writer a
*compile-time* property (the borrow checker rules out a concurrent reader/writer), not a runtime
lock.

## ArenaStore: heap-free volatile storage

Records live in the caller-supplied `&mut [u8]` as one sorted slab keyed by `(table, key, value)`
(see `databases/arena/region.rs`). The buffer is split `[ base | work ]`: a write transaction copies
base→work at begin, stages into work (read-your-writes), and `commit` copies work→base in one shot
(atomic); drop-without-commit leaves base untouched (rollback). **Manifest-first sizing**: pre-size
the buffer as

```rust
let mut buf = [0u8; 2 * (COUNT_HDR + CAP * RECORD_SIZE)]; // CAP records per half
let mut store = ArenaStore::<R>::open(&mut buf)?;
```

Exceeding `CAP` is a `NetabaseError::Capacity` — bounded, explicit, no allocation. (`RECORD_SIZE`
fixes `NAME_MAX=64`, `KEY_MAX=256`, `VAL_MAX=4096`; values larger than `VAL_MAX` are rejected.)

## Arena soundness (token capabilities)

`DynSlotIdx` carries `(brand, generation, index)`: each arena mints a process-unique **brand** and
bumps a **generation** on `reset()`. Every accessor validates all three, so a token used on a
foreign arena fails `ForeignArena` and a token outliving a `reset` fails `StaleToken` (no ABA aliasing).
This is a deterministic **runtime** check (not a compile-time proof — a closure-scoped generative
brand could strengthen it but would forbid storing arenas in struct fields, which `ArenaStore`
needs). Panicking `Index`/`IndexMut` accessors were removed; write capability requires `&mut` on the
arena (`slot_mut_ptr`), so a shared `Copy` token can never mint an aliasing write pointer.
