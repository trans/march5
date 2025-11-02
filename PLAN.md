# March α₅ Execution Plan

## Now (unblock drift from design docs)

- ✅ **Broaden global store values** (2025-03-05): snapshots and state prims now handle f64s, tuples, strings, and CID-backed quotes; regression coverage exercises the new cases.

- ✅ **YAML catalog loader** (2025-03-05): tagged YAML supports namespaces, effects/prims/words/snapshots, and CLI `catalog` applies documents end-to-end.

- ✅ **Bootstrap global state backend** (2025-03-05): in-memory namespaced store wired through `state.read_i64`/`state.write_i64`, interpreter enforces domain tokens, CLI exposes `state snapshot/reset`.

- ✅ **Generalised token pooling** (2025-03-05): interpreter/builder now emit per-domain tokens; added regression coverage for mixed IO+State words.

- ✅ **Guard quotations (stage 1)** (2025-03-05): guards compile as pure quotations, builder attaches guard CIDs, interpreter runs them ahead of word bodies, YAML/CLI workflows persist and attach them. CLI now includes `guard add/list/show`, `word add --guard`, and REPL support (`begin-guard`, `finish-guard`, `attach-guard`).
- ✅ **Guard lowering (stage 2)** (2025-03-05): dispatch cases now inline guard graphs alongside candidate calls, interpreter consumes the lowered checks with deopt fallbacks, and legacy three-field payloads remain readable.

## Next (extend capability model)

- **Transaction scaffolding**: add node kinds and builder helpers for `TXN_BEGIN/COMMIT/ABORT`, threading transaction IDs through the token pool and effect frontier.

## Later (performance & tooling)

- **Mini code cache integration**: teach the interpreter to cache straight-line pure subgraphs via the `code_cache` table and the existing `exec` helpers.

- **Lockfile & resolver polish**: build the lockfile writer/reader pipeline and surface namespace imports (including `use` aliases) in CLI tooling for better reproducibility.

- **Web UI enhancements**: expose effect inventories, dependency graphs, and search in `src/bin/webui.rs` once schema changes settle.

## Open Questions
- When we standardise effect CIDs for core domains (IO, state, net), how do we describe their semantics so downstream tooling can reason about them while still allowing user-defined effects?
- How do we co-develop the Mini Interaction-Net ABI (DESIGN-IV) and the stack builder/interpreter so neither blocks the other?
