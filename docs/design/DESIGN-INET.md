# Interaction-Nets Design (Agents + Rules)

## Overview
- Core language is a set of agent kinds (nodes with ports) and rewiring rules.
- A lean Rust reducer applies rules to active pairs (principal–principal), reducing nets.
- Programs (“words”) are nets; reduction bottoms out at primitives implemented in Rust.
- Higher-level syntax (YAML, S‑expr, Forth-like) is sugar that builds nets and definitions.

## Key Concepts
- Agent: named kind with ports (one principal, N auxiliaries).
- Net: agents + wires connecting ports; entry indicates a runnable net.
- Rule: LHS pattern (active pair) ⇒ rewire (construct/rewire a sub-net), optional side‑effects.
- Primitive: special agent whose rule executes native code and produces replacement wiring/values.
- Namespaces: group definitions (agents, rules, words, types, effects, guards) with imports/exports.

## Serialization (objects)
- agent: { kind: "agent", name, ports: [principal, ...aux], doc? }
- rule: { kind: "rule", lhs: [a, b], rewire: ... , side_effect? }
- inet: { kind: "inet", agents: [...], wires: [...], entry }
Store canonically in CBOR; address by 32‑byte CID; name via name_index.

## Reducer
- Worklist of active pairs; for each: match rule, apply rewiring, enqueue new pairs.
- AOT: reduce nets to normal form and persist; compiled code keyed by reduced CID.
- JIT/mini‑inet: reduce locally around calls; primitives bottom out via Rust.

## Existing Pieces We Reuse
- Storage and canonical encoders for effects, words, nodes; SQLite schema and name index.
- Guards, effects (token domains), dispatch, apply/call, return, pair/unpair.
- CLI + YAML catalog; lightweight web UI.

## Incremental Path
1) Data/API
   - Add inet.rs with AgentCanon, RuleCanon, Net struct; encoders + store/load helpers.
   - New object kinds: "agent", "rule", "inet".
2) Bootstrap rules
   - Author agents/rules mirroring current node semantics: guard.type, if, deopt, pair/unpair, call/apply, dispatch, return, token.
3) Reducer core
   - Net representation and basic rewiring engine (match+apply).
   - Unit tests for rule application and small reductions.
4) Bridge
   - Translate current graphs to nets; reduce where rules exist; fallback to interpreter while bootstrapping.
5) Specialization + code cache
   - Reduced subgraph CID becomes code_cache key; integrate compiled stubs.
6) Web UI
   - CRUD for Agent/Rule/Word/Type/Effect/Guard/Const/Var/Namespace.
   - Net visualization + stepper; JSON endpoints.

## Notes
- Result-type dispatch is supported if candidates agree on output shape at use sites; token domains unify via effect masks.
- Unconnected ports should be considered invalid unless explicitly permitted by an agent’s rule.
- Primitives bottom out via Rust FFI; they return agents/wiring (pure) or perform side effects guarded by tokens.

## Current Status (2025‑03‑05)
- Implemented overloads + guard lowering on the graph side, plus a Dispatch node with interpreter support.
- YAML `!overloads` (sequence form) expands to `<base>#<params->results>`; builder resolves base symbols by types or synthesizes Dispatch when multiple/guarded.
- Added inet storage + reducer scaffold:
  - Agents/Rules stored in DB as canonical objects; CLI and Web UI can add/show/list them.
  - Net representation (agents, ports, wires); active‑pair search; S‑expr rule DSL.
  - DSL supports: `(seq …)`, `(connect (A port) (B port))`, `(disconnect …)`, `(delete A B)`, `(new KIND alias (ports…))`.
  - Example rule in DB reduces `(pair, unpair)`; tests cover rewiring and a disconnect→connect scenario.

## Next Session Plan
1) Reducer Rules (core semantics)
   - Add guard.type + if + deopt rules in DSL (choose continuation; terminal deopt).
   - Add call/apply rules to rewire into entry subnets (bridge with current CALL/APPLY during bootstrap).
   - Add return + token threading rules (derive domains; validate port counts).
2) Graph → Net Translator
   - Translate node graphs (Lit/Prim/Call/Apply/If/Guard/Return/Pair/Unpair/Token) into inet agents+wires.
   - Reduce where rules exist; fallback to interpreter otherwise.
3) Dispatch + Effects
   - Extend builder Dispatch synthesis to support effectful candidates (unify IO/State/Test/Metric tokens).
   - Compile‑time pruning for type‑only cases before emitting nodes.
4) Web UI Authoring
   - POST forms to create Agents/Rules; list + detail pages.
   - Net visualizer and stepper once reducer has guard/if/deopt.

## File Pointers
- Reducer + DSL: `src/inet.rs`
- Dispatch execution: `src/interp.rs` (kind 14)
- Overload resolution (builder path): `src/main.rs`
- Agent/Rule CLI: `src/main.rs` (AgentCommand, RuleCommand)
- Web UI endpoints: `src/bin/webui.rs`
