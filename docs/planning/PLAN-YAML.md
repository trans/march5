# YAML Serialization

## Progress

* ✅ **YAML catalog loader** (2025-03-05): tagged YAML supports namespaces, effects/prims/words/snapshots, and CLI `catalog` applies documents end-to-end.

## Next Steps

- Extend schema coverage (guards with parameters, inet agents/rules) as those features stabilise.
- Add round-trip tests for the new CLI module structure so YAML-driven workflows keep working after refactors.

## Design Considerations

- Keep the YAML surface aligned with the CLI commands—document new tags in a single place (`examples/` plus README) to avoid divergence.
- Prefer additive changes; treat breaking YAML changes as versioned migrations once we ship user databases.
