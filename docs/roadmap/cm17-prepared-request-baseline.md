# CM-17 PreparedModelRequest snapshot baseline

> Snapshot date: 2026-09-20. Focused fixtures run in GitHub Actions only; local tests are intentionally not run.

| Item | Record |
|---|---|
| roadmap card | [`CM-17`](context-memory.md#step-cm-17) |
| feature_status | `implemented` (immutable prepared context snapshot and binding rechecks) |
| proof_level | `source`; local checks are formatting/diff hygiene only, CI fixtures are remote and not awaited |
| canonical path | ScopeSet + ContextPlan + source snapshots + CM-16 wire budget + route/catalog epochs → `PreparedModelRequest` |
| authority | the snapshot carries evidence and authorization bindings but grants no capability and never executes a provider call |

## Contract and behavior

`ResolvedStepContext` now carries the complete ContextPlan, ScopeSet, PromptBundle/plan digest,
tool catalog digest, source snapshots, model profile, route/catalog/workspace/data bindings and
the CM-16 `WireBudget`. Its request digest covers every field used by estimate/send/receipt;
`PreparedModelRequest` is the provider-facing alias for this same value, not a second compiler.

`validate_against` rejects plan, prompt, scope, source snapshot, budget and digest drift.
`recheck_bindings` rejects route, tool catalog, scope, workspace revision and data epoch drift
before the prepared snapshot can be reused.

## CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `same_step_snapshot_renders_same_wire_and_provenance` | the same immutable plan/snapshot renders the same prompt and keeps source/budget provenance |
| `route_change_between_prepare_and_send_is_fenced` | route and scope changes fail before reuse |
| `prepared_request_is_one_snapshot_and_rechecks_bindings` | Core source guard keeps one snapshot and forbids a second Model/Broker path |

## Proof ceiling and handoff

The CM-17 ceiling is `source` plus remote CI wiring. Provider transport still needs to consume
this object at its actual send boundary, and durable cross-process hydration/receipt replay remains
open; this slice does not claim live or durable execution.
