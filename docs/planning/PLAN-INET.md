# Interaction-Nets Plan (Agents + Rules)

## Progress

- ✅ **Graph builder**: `GraphBuilder` (`src/builder.rs`) assembles graphs from a Forth-like stack machine, tracks effect-domain tokens, emits `RETURN` nodes, and keeps the name index in sync.
- ✅ **Net scaffolding**: `inet.rs` encodes agent/rule objects, a reducer scaffold, and a work-in-progress DSL for rewrites (pair/unpair sample rule in place).
- ✅ **Dispatch + guards**: guard lowering feeds into the future inet dispatcher (guard graphs already available on dispatch nodes).

## Next Steps

- Flesh out core rewrite rules (guard type, if/deopt, call/apply, return/token threading) so the reducer can run more of the existing graph inventory.
- Build the graph→inet translator that opportunistically reduces nets and falls back to the interpreter where rules are missing.
- Capture design updates in `docs/design/DESIGN-INET.md` as the ABI firms up.

## Design Considerations

- Keep the reducer incremental: aim for hybrid execution (inet reduction for parts with rules, interpreter fallback elsewhere) to avoid blocking feature delivery.
- Track compatibility between inet agents and existing node kinds so we can highlight missing translation paths early.
