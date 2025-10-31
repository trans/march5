# March α₅ Execution Plan

## Now (unblock drift from design docs)

- **Bootstrap global state backend**: stand up a mutable global store (even if backed by an in-memory stub), add state read/write prims, and route the new `State` tokens through it so the multi-domain token plumbing exercises real data flow.

- ✅ **Generalised token pooling** (2025-03-05): interpreter/builder now emit per-domain tokens; added regression coverage for mixed IO+State words.

## Next (extend capability model)

- **Implement context guards**: flesh out `GUARDCTX` lowering and interpreter handling, allowing overload dispatch based on runtime predicates and contexts.

- **Transaction scaffolding**: add node kinds and builder helpers for `TXN_BEGIN/COMMIT/ABORT`, threading transaction IDs through the token pool and effect frontier.

## Later (performance & tooling)

- **Mini code cache integration**: teach the interpreter to cache straight-line pure subgraphs via the `code_cache` table and the existing `exec` helpers.

- **Lockfile & resolver polish**: build the lockfile writer/reader pipeline and surface namespace imports (including `use` aliases) in CLI tooling for better reproducibility.

- **Web UI enhancements**: expose effect inventories, dependency graphs, and search in `src/bin/webui.rs` once schema changes settle.

## Open Questions
- When we standardise effect CIDs for core domains (IO, state, net), how do we describe their semantics so downstream tooling can reason about them while still allowing user-defined effects?
- How do we co-develop the Mini Interaction-Net ABI (DESIGN-IV) and the stack builder/interpreter so neither blocks the other?
