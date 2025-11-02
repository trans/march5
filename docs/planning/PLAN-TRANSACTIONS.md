# Transactions & Context Plan

## Progress

- Pending – no prototype exists yet.

## Next Steps

- Sketch the node/DSL surface for `TXN_BEGIN`, `TXN_COMMIT`, `TXN_ABORT`, and token threading so the builder/interpreter semantics are clear.
- Decide how guard evaluation interacts with transactional contexts (e.g., guard failures triggering aborts vs. soft retries).
- Document storage implications (persistence layer, durability knobs) in `docs/design/DESIGN-V.md`.

## Design Considerations

- Transactions should compose with existing effect tokens; avoid introducing a separate channel unless necessary.
- Start with in-memory semantics to prove out the API, then layer in persistence/durability guarantees before shipping.
