# CM-29 projection lag, recovery and result_unknown baseline

> Snapshot date: 2026-09-20. Focused fixtures run in GitHub Actions only; local tests are intentionally not run.

| Item | Record |
|---|---|
| roadmap card | [`CM-29`](context-memory.md#step-cm-29) |
| feature_status | `implemented` (visible projection lag and original-key Unknown reconciliation contracts) |
| proof_level | `source`; local checks are formatting/diff hygiene only, CI fixtures are remote and not awaited |
| canonical path | committed source cursor → explicit projection lag view/read gate; uncertain mutation → original idempotency key/digest reconciliation |
| authority | EventLog commit/cursor and mutation idempotency facts remain authoritative; projection/index/checkpoint are rebuildable views |

## Contract and behavior

`ProjectionLagView` distinguishes caught-up, pending and unknown projection state. An absent
projector cursor is `unknown` and `projection_pending=true`; a lagging cursor is visible with a
reason; only a validated caught-up view permits a consistent read. Run receipts now include this
projection view instead of silently treating an unobserved projector as current.

`UnknownMutationReconciliation` binds `result_unknown` to the original mutation ID and protected
request digest. Reconciliation with the same key/digest is allowed; changing the ID or payload
digest is rejected, so callers cannot blind-retry a possibly committed mutation under a new key.
Existing ProjectionCheckpoint/ProjectionDriver and MemoryJournal replay remain the rebuild path.

## CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `projection_lag_is_visible` | lagging and unobserved projection cursors are explicit and not read-consistent |
| `unknown_mutation_is_not_retried_with_new_id` | Unknown mutation accepts only original-key/digest reconciliation |
| `projection_lag_and_unknown_mutation_share_one_recovery_boundary` | receipt, checkpoint, memory journal and EventLog source guards remain one recovery boundary |

## Proof ceiling and handoff

The CM-29 ceiling is `source` plus remote CI wiring. Cross-process projector persistence, physical
crash/power-loss rehearsal, index rebuild throughput, UI hydration and live provider effects remain
PD/ER/UI/provider work; no local runtime or durable production claim is made.
