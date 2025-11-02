# CLI Plans

## Progress 

* **CLI tooling**: binary in `src/main.rs` supports `new`, `effect`, `prim`, `iface`, `namespace`, `node`, `word`, `guard`, `builder`, `run`, and `catalog` subcommands; YAML loaders power `run --args-yaml` and catalog import with regression coverage.

## Design Considerations

Organize commands in to separate subcommand files within cli/ diretory.