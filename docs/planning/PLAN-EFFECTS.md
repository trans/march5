# Effects Plans

## Progress

* Effect masks collapse to `effect_mask::IO`; other bit flags exist in `src/types.rs` but have no integration.

* ✅ **Generalised token pooling** (2025-03-05): interpreter/builder now emit per-domain tokens; added regression coverage for mixed IO+State words.

## Design Considerations

