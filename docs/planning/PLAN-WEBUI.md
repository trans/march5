# WebUI Plan

## Progress

* ✅ Initial WebUI with some UI for listing words and namespaces.
* ✅  **Web UI**: `src/bin/webui.rs` serves HTML + JSON views over objects stored in a March database.

## Next Steps

- Expose effect inventories, dependency graphs, and search once schema changes settle.
- Document the REST/JSON endpoints and add simple frontend smoke tests so regressions are caught automatically.

## Design Considerations

- Keep the web UI optional: ensure every feature it surfaces is also reachable via CLI.
- As inet/transaction work lands, add drill-down views incrementally rather than designing a full UI up front.
