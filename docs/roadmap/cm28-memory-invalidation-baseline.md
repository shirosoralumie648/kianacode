# CM-28 deletion, expiry and revocation propagation baseline

> Snapshot date: 2026-09-20. Focused fixtures run in GitHub Actions only; local tests are intentionally not run.

| Item | Record |
|---|---|
| roadmap card | [`CM-28`](context-memory.md#step-cm-28) |
| feature_status | `implemented` (epoch-fenced invalidation plan and historical-receipt preservation contract) |
| proof_level | `source`; local checks are formatting/diff hygiene only, CI fixtures are remote and not awaited |
| canonical path | deletion/expiry/revocation fact → data epoch → `MemoryInvalidationPlan` → derived targets invalidated → history retained but non-reinjectable |
| authority | EventLog/tombstone/data-governance epoch is the source of truth; JSONL/index/cache/UI are projections and disposable |

## Contract and behavior

`MemoryInvalidationPlan` expands one invalidation across memory JSONL/body, BM25, dense and repo
indexes, ContextPlan, summary, checkpoint, prompt cache, UI and historical receipt state. Every
target carries the new data epoch and tombstone digest; all active material is `Invalidated` and
the historical-receipt target is `PreservedInvalid` with reinjection disabled.

The existing data-governance projection now exposes the expanded derived-store set, and the daemon
governance result reports `historical_receipts=preserved_invalid` and `reinjection=blocked`. A run
receipt can still retain its event IDs after revocation, but its historical receipt projection is
explicitly invalid and cannot feed a new context view.

## CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `deletion_propagates_to_memory_and_index` | delete tombstone advances epoch and invalidates memory/index/context/cache targets |
| `historical_receipt_is_preserved_but_not_reinjected` | historical receipt metadata is retained as invalid and `can_reinject` is false |
| `deletion_propagation_is_epoch_fenced_and_history_is_not_reinjected` | core/daemon governance and receipt projections preserve the same deny-first boundary |

## Proof ceiling and handoff

The CM-28 ceiling is `source` plus remote CI wiring. Physical cross-process projector recovery,
durable index generation rebuild, artifact/blob erasure, UI refresh and retention GC remain CM-29/
PD/SC/UI work; historical EventLog facts are intentionally retained and no provider exposure is
claimed reversible.
