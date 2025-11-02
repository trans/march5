# Database Plans

* **Object encoding & storage**: canonical CBOR emitters exist for primitives, nodes (including `RETURN`/multi-result support), words, interfaces, and namespaces; persisted through SQLite (`src/store.rs`).

