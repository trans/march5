# Lockfile Plan

## Progress

- Resolver and name-index machinery exist, but no lockfile artifacts are written/read yet.

## Next Steps

- Design the lockfile schema (capturing catalog sources, resolved CIDs, effect masks) and prototype a writer/reader pair.
- Extend the CLI to emit/import lockfiles alongside YAML catalogs for reproducible builds.
- Surface namespace import aliases (`use` sugar) once stored so they round-trip cleanly.

## Design Considerations

- Treat the lockfile as an optional additive artifact; avoid impacting existing workflows until the format is stable.
- Document versioning and compatibility upfront so we can evolve the lockfile without breaking existing projects.
