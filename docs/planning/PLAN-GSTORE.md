# Global Store Plans

## Progress

* ✅ **Broaden global store values** (2025-03-05): snapshots and state prims now handle f64s, tuples, strings, and CID-backed quotes; regression coverage exercises the new cases.

* ✅ **Bootstrap global state backend** (2025-03-05): in-memory namespaced store wired through `state.read_i64`/`state.write_i64`, interpreter enforces domain tokens, CLI exposes `state snapshot/reset`.

## Next Steps

- Add durability options (persist snapshots automatically, scheduled checkpoints) once transaction work lands.
- Extend CLI/state primitives to support structured keys or namespaces for richer use cases.
- Document the store schema and expectations in the design docs.

## Design Considerations

- Keep the global store strictly opt-in for words that declare the appropriate effect tokens.
- Ensure snapshot formats remain backward-compatible; note any breaking changes in `docs/planning/history/`.
