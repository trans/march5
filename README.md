# march5

Prototype command-line tooling for the March α₅ project.

## Usage

Build the binary with Cargo:

```bash
cargo build --release
```

Create a new March database (produces `demo.march5.db` in the current directory when no extension is supplied):

```bash
target/release/march5 new demo
```

The `new` command initialises the SQLite file with the `object`, `name_index`, and `code_cache` tables and applies the PRAGMAs described in `PROTOTYPE.md`.

Add an effect descriptor to an existing store:

```bash
target/release/march5 --db demo.march5.db effect add io --doc "performs input/output"
```

The command hashes the canonical CBOR encoding of the effect, inserts it into the `object` table if it is not already present, and prints the resulting CID.

Register a primitive descriptor (including optional attributes and automatic name-indexing):

```bash
target/release/march5 --db demo.march5.db prim add add_i64 \
  --param i64 --param i64 \
  --result i64 \
  --attr category=arith --attr commutative=true
```

By default the CLI persists the primitive object and records the name in the `name_index` under the `prim` scope.

## Development

Run the unit test suite (build check):

```bash
cargo test
```
