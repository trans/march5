# Effects Plans

## Progress

* ✅ **Generalised token pooling** (2025-03-05): interpreter/builder now emit per-domain tokens; added regression coverage for mixed IO+State words.
* ℹ️ Effect masks currently collapse to `effect_mask::IO`; additional bit-flags exist in `src/types.rs` but are not used yet.

## Next Steps

- Decide on canonical CIDs and semantics for core domains (IO, state, net) and document them for downstream tooling.
- Extend the effect parser/encoder to accept multiple domains once transaction/state work lands.

## Design Considerations

- Keep effect descriptions declarative so external tooling can reason about them without hard-coding behaviour.
