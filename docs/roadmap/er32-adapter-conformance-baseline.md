# ER-32 Memory/JSONL adapter conformance baseline

## Scope

ER-32 adds a strict conformance report for Memory and JSONL logical outcomes across commit, replay,
conflict, cursor gap and Unknown operations. Both adapters must preserve the same source/output
digests and result classes; only JSONL with an explicit sync acknowledgement may claim durable
evidence. Duplicate effects, false success, secret-bearing observations and wrong replay/conflict/
cursor/Unknown classifications fail closed.

The contract does not open a file, append a frame, run property generation, perform migration or
claim a Memory adapter is durable. Existing EventLog `TransitionPlan`/`CommitOutcome` paths remain
the adapter owners.

## CI-only evidence

`.github/workflows/er32-adapter-conformance.yml` runs formatting, domain Memory/JSONL conformance
fixtures, the Core/EventLog boundary guard and affected test-target compilation on GitHub Actions.
Local Cargo tests, builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Failure-first fixture matrix

| Fixture | Assertion |
|---|---|
| `memory_and_jsonl_share_logical_outcomes_but_only_jsonl_can_claim_sync_durable` | logical output parity is preserved while Memory remains non-durable |
| `adapter_conformance_rejects_memory_durability_and_wrong_fault_results` | durable claim, replay result, and secret-safety drift fail closed |
| `report_rejects_duplicate_adapter_operation_observation` | duplicate adapter/operation evidence is rejected |

## Limitations and handoff

- No JSONL file, fsync, truncation, migration, multi-process writer or projector restart ran in
  this source slice; those remain ER-33/34 and PD work.
- CI conformance is not a runtime durability proof and does not promote live/physical evidence.
- CI results are intentionally not awaited; proof level remains `source`.
