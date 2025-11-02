# Global Store Plans

## Progress

* ✅ **Broaden global store values** (2025-03-05): snapshots and state prims now handle f64s, tuples, strings, and CID-backed quotes; regression coverage exercises the new cases.

* ✅ **Bootstrap global state backend** (2025-03-05): in-memory namespaced store wired through `state.read_i64`/`state.write_i64`, interpreter enforces domain tokens, CLI exposes `state snapshot/reset`.

## Design Considerations

* TODO