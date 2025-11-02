# Guard Plans

## Progress

* ✅ **Guard quotations (stage 1)** (2025-03-05): guards compile as pure quotations, builder attaches guard CIDs, interpreter runs them ahead of word bodies, YAML/CLI workflows persist and attach them. CLI now includes `guard add/list/show`, `word add --guard`, and REPL support (`begin-guard`, `finish-guard`, `attach-guard`).

* ✅ **Guard lowering (stage 2)** (2025-03-05): dispatch cases now inline guard graphs alongside candidate calls, interpreter consumes the lowered checks with deopt fallbacks, and legacy three-field payloads remain readable.

* ✅ **Guard quotations & predicates**: guards compile to pure quotations with stored CIDs, builder attaches them to words and dispatch cases, the interpreter consumes lowered guard graphs with deopt fallbacks (legacy three-field dispatch payloads still decode), CLI supports `guard add/list/show`, `word add --guard ...`, and the REPL can `begin-guard`/`finish-guard` and `attach-guard`; boolean/comparison primitives (`eq_i64`, `gt_i64`, `and`, `or`, `not`, etc.) are available for guard logic.


## Design Considerations

Guards are runtime context.