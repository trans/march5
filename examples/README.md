# March α₅ Example Databases

## helloworld.march5.db

A minimal database containing:

- `demo/hello` word returning the literal `42` (`i64`).
- `demo.ns` namespace exporting `hello`.

Creation transcript:

```bash
cargo run --bin march5 -- new examples/helloworld
cargo run --bin march5 -- --db examples/helloworld.march5.db \
  node lit --ty i64 --value 42
# node CID: a6a46c5335b102a417585e52c5c08e54c03e62f8ebbda507f154d088ce8ac840
cargo run --bin march5 -- --db examples/helloworld.march5.db \
  word add --name demo/hello \
  --root a6a46c5335b102a417585e52c5c08e54c03e62f8ebbda507f154d088ce8ac840 \
  --result i64
# word CID: cdb6fd5c4174a3132915a330155f8c4bcc7666c143b2fdea03bc093290faef9b
cargo run --bin march5 -- --db examples/helloworld.march5.db \
  namespace add --name demo.ns \
  --export hello=cdb6fd5c4174a3132915a330155f8c4bcc7666c143b2fdea03bc093290faef9b
# namespace CID: 5ebbd7ee6351d6455d85e768ec67c3e8461a445516d95c1b81b2119a1b500671
```

Inspect with CLI:

```bash
cargo run --bin march5 -- --db examples/helloworld.march5.db namespace show demo.ns
cargo run --bin march5 -- --db examples/helloworld.march5.db word show demo/hello
```
