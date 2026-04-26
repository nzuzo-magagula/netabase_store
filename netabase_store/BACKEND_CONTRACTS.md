# Backend Capability & Contract Baseline

This file is the source-of-truth backend contract for the current cycle.

## Cycle target

- **Full support target:** `redb`, `memory`
- **Capability-gated backend:** `fjall` (explicit unsupported behavior)

## Contract matrix

| Contract surface | redb | memory | fjall |
| --- | --- | --- | --- |
| `open_read_table` | Supported. Opens/creates table handle for typed reads. | Supported with typed in-memory reads backed by shared map state. | Supported as capability-gated stub handle. Table ops return `RoutingError`. |
| `open_write_table` | Supported. Opens/creates typed write table. | Supported with typed in-memory insert/remove/get/get_all behavior. | Supported as capability-gated stub handle. Table ops return `RoutingError`. |
| `open_read_multimap_table` | Supported. Reads multimap values for a key. | Supported with typed in-memory multimap reads. | Supported as capability-gated stub handle. Table ops return `RoutingError`. |
| `open_write_multimap_table` | Supported. Multimap insert/remove behavior available. | Supported with typed in-memory multimap insert/remove behavior. | Supported as capability-gated stub handle. Table ops return `RoutingError`. |
| Repository `get(address, key)` | Routed via `route_get` when repository router is zero-sized; non-zero-sized repositories return `RoutingError`. | Routed via `route_get` when repository router is zero-sized; non-zero-sized repositories return `RoutingError`. | Explicitly unsupported: returns `RoutingError("fjall capability-gated backend: repository get is unsupported")`. |
| Repository `insert(item)` | Delegates to `item.route_insert(...)` with default orchestration options. | Same as redb. | Same as redb. |
| Repository `delete(address, key)` | Delegates to `R::route_delete(...)` with default orchestration options. | Same as redb. | Same as redb. |
| Transaction `commit()` | Real backend commit (`redb` write transaction commit). | No-op success (`Ok(())`). | No-op success (`Ok(())`). |

## Capability-gated `fjall` expectations

`fjall` is intentionally capability-gated this cycle:

- `open_*` table methods return valid typed handles so generic routing code can compile and execute.
- Any table data operation on those handles (`get`, `get_all`, `insert`, `remove`) is explicitly unsupported and must return `NetabaseError::RoutingError("fjall capability-gated backend: table data operations are unsupported")`.
- Repository-level `get(address, key)` is also explicitly unsupported and returns `NetabaseError::RoutingError("fjall capability-gated backend: repository get is unsupported")`.

## Test lock-in

These contracts are locked by `tests/backend_contracts_tests.rs` and existing backend gap/routing tests.
