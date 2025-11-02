# CLI Plans

## Progress

- ✅ **Module split** (2025-03-05): `src/main.rs` now just parses arguments and delegates to `src/cli/commands/`, keeping subcommand logic in focused modules (catalog, prim, guard, etc.).
- ✅ **YAML + catalog tooling** (2025-03-05): CLI pipeline continues to expose `run --args-yaml` and catalog import with regression coverage.

## Next Steps

- Document the new module structure (`docs/planning/PLAN-CLI.md` + `README`) so new contributors know where to hook subcommands.
- Tighten integration tests for high-touch commands (e.g., guard/word add flows) using the refactored modules.

## Design Considerations

- Keep the CLI as a thin orchestrator: prefer pushing reusable logic into library modules so the web UI (or other front-ends) can reuse the same helpers.
- When adding new subcommands, update `STATUS.md` and this plan to capture follow-on work (docs, tests, examples).
