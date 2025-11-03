# Database Plans

## Progress

- ✅ **Canonical encoders**: CBOR emitters exist for primitives, nodes (including `RETURN`/multi-result support), words, interfaces, namespaces, and inet artifacts; all persisted through SQLite (`src/store.rs`).
- ✅ **Name index**: CLI and builder flows keep the `name_index` table in sync with object inserts.

## Next Steps

- Add migration tooling once we stabilise schema changes (e.g., effect masks, transactions).
- Capture schema documentation (tables, columns, relationships) in `docs/design` for new contributors.
- Investigate lightweight indexing/search improvements for the web UI and CLI (`list` commands).
- Consider centralising common SQL “query templates” (pre-prepared statements or helper helpers) so callers reuse them instead of embedding raw SQL.

## Design Considerations

- Keep the DB layer simple: treat SQLite as the source of truth and continue to expose pure helper functions rather than an ORM.
- Any schema change should ship with a migration story (even if “drop and rebuild” during alpha).
