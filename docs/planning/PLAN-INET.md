# Interaction-Nets Plan (Agents + Rules)

## PROGRESS

* ✅ **Graph builder**: `GraphBuilder` (`src/builder.rs`) assembles graphs from a Forth-like stack machine, tracks effect-domain tokens, emits `RETURN` nodes, attaches guard quotations, and registers words in the name index.

* **Interpreter & exec stubs**: `run_word` in `src/interp.rs` evaluates graphs (including catalog-authored guards, APPLY nodes, and token threading); `src/exec.rs` contains a minimal JIT stub for add/sub primitives.

* `RETURN`/multi-output encoding is in place; legacy `ty` fields have been removed.

## Design Considerations

TODO