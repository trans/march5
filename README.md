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

Define an interface (each `--symbol` looks like `name(param,...) -> (result,...) | effectCID,...` and you can omit the trailing section for pure symbols):

```bash
target/release/march5 --db demo.march5.db iface add \
  --name demo.math/iface \
  --symbol "hello() -> unit | 9545e3adf7a49fb36233ec4555d0763b694ac65330ffb412a1c438d8ebde09ec"
```

Create a namespace that ties an interface to imported interfaces and exported words:

```bash
target/release/march5 --db demo.march5.db namespace add \
  --name demo.math \
  --iface <iface_cid> \
  --import <io_iface_cid> \
  --export <hello_word_cid>
```

List the registered namespaces:

```bash
target/release/march5 --db demo.march5.db namespace list --prefix demo.
```

Create a literal node (produces a canonical node object and prints its CID):

```bash
target/release/march5 --db demo.march5.db node lit --ty i64 --value 9
```

Define a word entrypoint and register it under a name:

```bash
target/release/march5 --db demo.march5.db word add \
  --name demo.math/difference \
  --root <root_cid> \
  --param i64 --param i64 \
  --result i64
```

Replace `<root_cid>` with the 64-digit hex CID emitted when creating the root node.

List the registered words under a namespace prefix:

```bash
target/release/march5 --db demo.march5.db word list --prefix demo.math/
```

## Development

Run the unit test suite (build check):

```bash
cargo test
```

Library consumers can use the `GraphBuilder` API (`src/builder.rs`) to assemble graphs in a Forth-like manner while reusing the same storage primitives exposed through the CLI.
