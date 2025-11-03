# DESIGN — DATABASE

## Overview
- Centralised SQLite store underpins March object persistence, accessed through `src/db.rs`.
- All persisted artifacts are content-addressed via 32-byte CIDs; metadata such as names or caches hang off those roots.
- Database helpers wrap `rusqlite::Connection` to enforce schema setup, pragma tuning, and common query patterns.

## Persistence Goals
- Provide durable storage for canonical March objects (words, prims, guards, inet rules, etc.).
- Allow name-based lookup and discovery without sacrificing CID immutability.
- Support both on-disk databases and in-memory connections for testing.
- Keep the schema minimal to ease future migrations or alternative backends.

## Schema Layout
- `object(cid BLOB PRIMARY KEY, kind TEXT, cbor BLOB)` — single table for all serialized objects, declared `WITHOUT ROWID` for predictable primary-key storage.
- `name_index(scope TEXT, name TEXT, cid BLOB)` — logical namespace for human-readable identifiers per scope; `(scope, name)` is the primary key.
- `code_cache(subgraph_cid, arch, abi, flags, blob)` — stores compiled artifacts keyed by execution environment parameters.
- Indexes:
  - `object_kind_idx` speeds `kind`-filtered scans.
  - `name_scope_cid_idx` aids reverse-lookups from CID back to name(s).

## Connection & Pragmas
- `derive_db_path` normalises user-supplied paths (adds `.march5.db` when no extension).
- `ensure_parent_dirs` guarantees directory existence before opening.
- `create_store` / `open_store` apply a common pragma set:
  - WAL journaling for concurrent reads.
  - `synchronous=NORMAL` and tuned cache/MMAP sizes for long-running CLI/Web UI usage.

## Query Facade
- `put_object`, `load_object_cbor`, and `load_cbor_for_kind` encapsulate validation around `object`.
- `put_name`, `get_name`, `list_names`, and `list_names_for_cid` centralise `name_index` usage, ensuring consistent ordering and CID decoding.
- `load_all_cbor_for_kind` supplies type-specific loaders (e.g., inet reducer) with ready-to-parse payloads.
- `count_objects_of_kind` aids tests and diagnostics when verifying insertions.
- Higher-level modules (builder, CLI, web UI) avoid embedding SQL, instead calling these helpers to reduce duplication and future schema touch points.

## Access Patterns
- Reads typically occur via short-lived connections per command invocation; helpers re-run `install_schema` defensively to tolerate missing migrations.
- Tests utilise `Connection::open_in_memory()` combined with `install_schema` to ensure isolation and speed.
- Long-running services (web UI) hold open connections, relying on WAL for reader/writer harmony.

## Future Extensions
- Introduce view/helpers for batched inserts (e.g., importing catalogs) to reduce transaction overhead.
- Evaluate additional indexes for workloads that rely heavily on `kind`/`name` prefixes.
- Consider migrating `code_cache` into its own module once compilation caching evolves.
- Track schema versioning in a meta table to allow migrations beyond `install_schema` idempotency.

## Open Questions
- Do we need a generalized transaction helper to coordinate multi-table updates (e.g., object + name registration)?
- Should we split read/write handles to pave the way for alternate backing stores (e.g., PostgreSQL) without `rusqlite` dependencies leaking outward?
