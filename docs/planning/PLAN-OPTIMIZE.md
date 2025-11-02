# Optimization Plans

## Progress

- Initial interpreter/JIT scaffolding (`src/exec.rs`) exists, but no caching primitives are wired up yet.

## Next Steps

- Prototype a straight-line “mini code cache” keyed by reduced subgraph CID so hot paths can bypass the interpreter.
- Measure guard/dispatch overhead after the cache prototype to identify additional hotspots.

## Design Considerations

- Optimisations should remain optional/debuggable: keep a toggle so we can bisect issues by disabling the cache at runtime.
