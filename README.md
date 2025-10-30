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
  --attr category=arith --attr commutative=true \
  --effect 9545e3adf7a49fb36233ec4555d0763b694ac65330ffb412a1c438d8ebde09ec
```

By default the CLI persists the primitive object and records the name in the `name_index` under the `prim` scope.

Define an interface (each `--name` entry looks like `name(param,...) -> result,... | effectCID,...`; omit the trailing section for pure exports):

```bash
target/release/march5 --db demo.march5.db iface add \
  --register demo.math/iface \
  --name "hello() -> unit | 9545e3adf7a49fb36233ec4555d0763b694ac65330ffb412a1c438d8ebde09ec"
```

Create a namespace that ties an interface to imported interfaces and exported words:

```bash
target/release/march5 --db demo.march5.db namespace add \
  --name demo.math \
  --import <io_iface_cid> \
  --export hello=<hello_word_cid>
```

When `--iface` is omitted the CLI derives the interface from the exported words
and stores it automatically.

List the registered namespaces:

```bash
target/release/march5 --db demo.march5.db namespace list --prefix demo.
```

Launch the interactive builder to script graphs without manually wiring CIDs:

```bash
target/release/march5 --db demo.march5.db builder
```

Inside the REPL you can run commands such as `begin`, `lit`, `prim <primCID>`,
`call <wordCID>`, `dup`, `swap`, `over`, and `finish <result> [name]`. Type
`help` in the prompt for the full list.

Start the lightweight web UI (serves HTML + JSON endpoints):

```bash
cargo run --bin webui -- --db demo.march5.db --listen 127.0.0.1:8080
```

Visit `http://127.0.0.1:8080/` for a simple index page and JSON API hints.

Run a word directly from the CLI (pass `--arg` per parameter when needed):

```bash
cargo run --bin march5 -- --db examples/helloworld.march5.db run org.march.helloworld/hello
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
  --result i64 \
  --effect 9545e3adf7a49fb36233ec4555d0763b694ac65330ffb412a1c438d8ebde09ec
```

Replace `<root_cid>` with the 64-digit hex CID emitted when creating the root node.

List the registered words under a namespace prefix:

```bash
target/release/march5 --db demo.march5.db word list --prefix demo.math/
```

## CLI reference

The commands above are intentionally thin wrappers around the canonical encoders.
The following notes capture the current contracts the CLI expects:

- **Effect and CID arguments**  
  Every `--effect` flag accepts a raw 32‑byte CID encoded as 64 hex digits.  
  Repeating the flag appends additional effect CIDs.  
  Inputs supplied via `--input` must be written as `CID:PORT`, where `CID` is a
  64‑digit hex string and `PORT` is the producer’s output port number.

- **`iface add`**  
  Each `--name` entry must follow `name(param,...) -> result,... | effectCID,...`.  
  Omit the trailing `| …` section for pure exports. Type atoms are strings for
  now (e.g., `i64`, `unit`). `--register <scope/name>` records the resulting
  interface CID in `name_index`; pass `--no-register` to skip this step.

- **`namespace add`**  
  `--import <ifaceCID>` may be repeated to declare the required interface CIDs.  
  `--export name=<wordCID>` pairs expose word CIDs under sorted names.  
  If `--iface` is omitted the CLI derives the interface automatically from the
  supplied exports. Namespaces are registered via `--name` unless
  `--no-register` is provided.

- **`node` subcommands**  
  - `node lit --ty <atom> --value <i64> [--effect <cid> ...]`  
  - `node prim --ty <atom> --prim <cid> [--input <cid:port> ...] [--effect <cid> ...]`  
  - `node call --ty <atom> --word <cid> [--input <cid:port> ...] [--effect <cid> ...]`  
  - `node arg --ty <atom> --index <u32> [--effect <cid> ...]`  
  - `node load-global --ty <atom> --global <cid> [--effect <cid> ...]`  
  Token nodes (created implicitly by the builder) currently cover only the IO
  domain; if you create effectful nodes manually you must supply any required
  token producer as one of the inputs.

- **`word add` / `prim add`**  
  Parameters and results are passed left-to-right via repeated `--param` and
  `--result` flags. Declared effects use the same `--effect` flags described
  above. Supplying `--no-register` prevents the name from being inserted into
  `name_index`.

- **Builder IO token policy**  
  The interactive builder (`march5 builder`) threads a single IO token
  automatically. Additional effect domains are not yet modelled; effectful
  operations outside IO should be considered experimental until the token pool
  work lands.

## Development

Run the unit test suite (build check):

```bash
cargo test
```

Library consumers can use the `GraphBuilder` API (`src/builder.rs`) to assemble graphs in a Forth-like manner while reusing the same storage primitives exposed through the CLI.
