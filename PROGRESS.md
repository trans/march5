# March α₅ Project Progress

## Snapshot
- `cargo test` passes locally (2025-10-28) with 34 core unit tests plus CLI/webui smoke checks.
- Working tree tracked on branch `main`; recent focus is on canonical encodings, stack builder, and CLI tooling.

## Implemented Capabilities
- **Object encoding & storage**: canonical CBOR emitters exist for primitives, nodes (including `RETURN`/multi-result support), words, interfaces, and namespaces; persisted through SQLite (`src/store.rs`).
- **CLI tooling**: binary in `src/main.rs` supports `new`, `effect`, `prim`, `iface`, `namespace`, `node`, `word`, `builder`, and `run` subcommands with test coverage (`src/main.rs`).
- **Graph builder**: `GraphBuilder` (`src/builder.rs`) assembles graphs from a Forth-like stack machine, tracks IO tokens/effects, emits `RETURN` nodes, and registers words in the name index.
- **Interpreter & exec stubs**: `run_word` in `src/interp.rs` evaluates graphs (including guards, APPLY nodes, and token threading); `src/exec.rs` contains a minimal JIT stub for add/sub primitives.
- **Web UI**: `src/bin/webui.rs` serves HTML + JSON views over objects stored in a March database.

## Known Gaps & Divergences
- Interface encoding currently serialises under the `names` key with per-symbol maps; the design documents expect a `symbols` array with positional fields.
- Effect handling is restricted to a single IO token (`TokenDomain::Io`); state/fs/net/test domains and R/W split tokens from DESIGN-IV/V are not implemented.
- Effect masks collapse to `effect_mask::IO`; other bit flags exist in `src/types.rs` but have no integration.
- Namespace imports encode raw interface CIDs but do not retain alias metadata (`use` sugar) as noted in DESIGN-II (expected, but flag for future ergonomic layers).
- Context dispatch, transactions, durability policies, and token pools described in DESIGN-IV/V remain to be prototyped beyond placeholder node kinds.
- `RETURN`/multi-output encoding is in place; legacy `ty` fields have been removed.

## Artifacts & Data
- Example database at `examples/helloworld.march5.db` demonstrates the CLI flow.
- `target/` contains compiled artifacts from the latest `cargo test` run.

## Questions / Open Threads
- Confirm whether to rename interface payload fields (`names` → `symbols`) and adopt positional arrays per DESIGN-II before wider tooling depends on the current shape.
- Decide how to extend `TokenDomain` to support multiple domains and transactions, and how that threads through `WordCanon.effect_mask`.
- Clarify priority between extending the interpreter (context guards, deopts) and starting the Mini-INet ABI implementation sketched in DESIGN-IV.
