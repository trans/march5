# STATUS

Use this file as the hand-off between working sessions. Keep it short, actionable, and date-stamped when things change.

## Current Focus _(2025-03-05)_

- Docs refresh in flight: design/planning directories split; individual `PLAN-*.md` files need fleshing out as features land.
- CLI refactor landed—follow-up polish (command docs, help text) can happen incrementally.

## Near-Term TODOs

- Context dispatch, transactions, durability policies, and richer token pools from DESIGN-IV/V still need concrete prototypes (`PLAN-TRANSACTIONS.md`).
- Namespace imports currently drop alias metadata (`use` sugar); capture desired behaviour in `PLAN-LOCKFILE.md` once requirements settle.

## Parking Lot

- Once the doc reorg stabilises, prune or move completed plans into `docs/planning/history/`.
