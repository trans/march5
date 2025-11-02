# Documentation

## Directory Map

- `docs/design/` – longer-form design references and historical proposals. They capture intent, but may lag behind the implementation; prefer the codebase for ground-truth semantics.
- `docs/planning/` – active workstreams. Each `PLAN-*.md` file tracks progress for a particular area (CLI, guards, inet, etc.) with separate “Progress” and “Design Considerations” sections. Archive completed plans under `docs/planning/history/`.
- `STATUS.md` – living scratchpad for “what we’re working on right now”. Update this as work moves between sessions.

## Keeping Things Current

1. When you land meaningful progress in a subsystem, update the relevant `PLAN-*.md` file’s **Progress** section before closing your branch.
2. Capture new ideas, outstanding questions, or unanswered design trade-offs in `docs/planning/OPEN-QUESTIONS.md`.
3. If a design doc diverges materially from the code, either refresh the affected section or add a short “Implementation Notes (YYYY-MM-DD)” callout describing the deviation.

## Quick Links

- Design overviews: [`docs/design/`](design/)
- Active plans: [`docs/planning/`](planning/)
- Status scratchpad: [`STATUS.md`](../STATUS.md)

## Notes

- `target/` contains compiled artifacts from the latest `cargo test` run.

