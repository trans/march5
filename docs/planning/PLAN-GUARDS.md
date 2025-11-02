# Guard Plans

## Progress

- ✅ **Stage 1 – Guard quotations** (2025-03-05): builder produces pure quotation graphs, CLI/REPL can register and attach them, interpreter enforces guard execution ahead of word bodies.
- ✅ **Stage 2 – Dispatch lowering** (2025-03-05): overload dispatch now inlines guard evaluation graphs so runtime selection is fast and legacy payloads still decode.

## Next Steps

- Integrate guard metadata into upcoming transaction/context dispatch work so we can differentiate “pure” vs “side-effectful” checks.
- Audit CLI docs/help output to surface guard usage patterns (`guard add`, `word add --guard`, REPL workflow).

## Design Considerations

- Guards should remain pure: continue rejecting guard definitions that declare effects or non-`i64` result types.
- When additional domains (transactions, context guards) appear, prefer encoding policy as guard stacks instead of new bespoke plumbing.
