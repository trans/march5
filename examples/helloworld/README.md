# March α₅ Example Databases

## helloworld.march5.db

Example database at `examples/helloworld.march5.db` demonstrates the CLI flow.

A minimal database containing:

- `org.march.helloworld/hello` word returning the literal `42` (`i64`).
- `org.march.helloworld` namespace exporting `hello`.

Creation transcript:

```bash
cargo run --bin march5 -- new examples/helloworld
cargo run --bin march5 -- --db examples/helloworld.march5.db \
  node lit --ty i64 --value 42
# node CID: a6a46c5335b102a417585e52c5c08e54c03e62f8ebbda507f154d088ce8ac840
cargo run --bin march5 -- --db examples/helloworld.march5.db \
  word add --name org.march.helloworld/hello \
  --root a6a46c5335b102a417585e52c5c08e54c03e62f8ebbda507f154d088ce8ac840 \
  --result i64
# word CID: cdb6fd5c4174a3132915a330155f8c4bcc7666c143b2fdea03bc093290faef9b
cargo run --bin march5 -- --db examples/helloworld.march5.db \
  namespace add --name org.march.helloworld \
  --export hello=cdb6fd5c4174a3132915a330155f8c4bcc7666c143b2fdea03bc093290faef9b
# namespace CID: 5ebbd7ee6351d6455d85e768ec67c3e8461a445516d95c1b81b2119a1b500671
```

Inspect with CLI:

```bash
cargo run --bin march5 -- --db examples/helloworld.march5.db namespace show org.march.helloworld
cargo run --bin march5 -- --db examples/helloworld.march5.db word show org.march.helloworld/hello
```
## YAML `!overloads` example

You can register multiple implementations under one symbol using `!overloads`.
Each entry specifies `params`, `results`, optional `guards`, and a `stack` body.

Example:

```yaml
core:
  add_i64: !prim
    params: [i64, i64]
    results: [i64]
text:
  concat: !word
    params: [text, text]
    results: [text]
    stack:
      - !dup
      - !prim core/add_i64   # placeholder; replace with proper concat prim/word
demo:
  add: !overloads
    - !word
      params: [i64, i64]
      results: [i64]
      stack:
        - !prim core/add_i64
    - !word
      params: [text, text]
      results: [text]
      stack:
        - !word text/concat
```

When applied via `march5 catalog`, each overload is persisted as a concrete
word under a derived name like `demo/add#i64,i64->i64`. The base symbol is
reserved for a future dispatcher or static resolver; for now, call the derived
name or reference the implementation by name.
