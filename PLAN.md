# March α₅ Execution Plan

## Now (unblock drift from design docs)

- **Document current CLI contracts**: capture argument formats for `iface add`, `namespace add`, and token-handling expectations in `README.md` to prevent misuse while encoders are in flux.

## Next (extend capability model)

- **Generalise token pooling**: introduce multiple `TokenDomain` entries (state, fs, net, test) and wire them through `GraphBuilder`, interpreter, and word metadata so we can exercise the split-token designs from DESIGN-IV/V.

- **Implement context guards**: flesh out `GUARDCTX` lowering and interpreter handling, allowing overload dispatch based on runtime predicates and contexts.

- **Transaction scaffolding**: add node kinds and builder helpers for `TXN_BEGIN/COMMIT/ABORT`, threading transaction IDs through the token pool and effect frontier.

## Later (performance & tooling)

- **Mini code cache integration**: teach the interpreter to cache straight-line pure subgraphs via the `code_cache` table and the existing `exec` helpers.

- **Lockfile & resolver polish**: build the lockfile writer/reader pipeline and surface namespace imports (including `use` aliases) in CLI tooling for better reproducibility.

- **Web UI enhancements**: expose effect inventories, dependency graphs, and search in `src/bin/webui.rs` once schema changes settle.

## Open Questions
- When we standardise effect CIDs for core domains (IO, state, net), how do we describe their semantics so downstream tooling can reason about them while still allowing user-defined effects?
- How do we co-develop the Mini Interaction-Net ABI (DESIGN-IV) and the stack builder/interpreter so neither blocks the other?
