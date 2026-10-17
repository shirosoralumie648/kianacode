# BQ-11 model attempt lifecycle baseline

> Snapshot date: 2026-09-25.  This slice adds a bounded, source-level lifecycle contract for
> admitted model attempts.  Runtime tests are GitHub-only and are not run locally.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`BQ-11`](../roadmap.md#step-bq-11) |
| source snapshot | `778cb7c3` plus this BQ-11 source slice |
| feature_status | `implemented` for the domain/core lifecycle contracts and CI fixtures |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | `prepared` → flushed append acknowledgement → `dispatching` → `observed` → `settled` or `unknown` |

The lifecycle record binds server-owned run/turn/model-attempt identity to the billing `AttemptId`,
`QuotaReservation`, permit reference, `NormalizedUsage` and a receipt reference.  A pure ledger emits
immutable facts; the core projector folds only committed lifecycle facts and leaves legacy model
events readable without treating them as billing facts.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| unprepared dispatch | no `model.dispatching` fact can be emitted before `model.prepared` |
| permit fence | a missing or mismatched permit is rejected before dispatch |
| flush fence | an append without a successful prepared flush acknowledgement cannot dispatch |
| duplicate settlement | a terminal `settled` attempt cannot settle a second time |
| usage identity | usage from another run/attempt cannot be observed or settled |
| unknown retention | an uncertain provider result keeps usage when available and retains a bounded error |
| projector replay | the core read model requires the prepared first fact, checks revisions/transitions,
  and uses only RuntimeEvent source facts |

## Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

GitHub Actions runs `cargo fetch --locked`, `cargo fmt --all --check`, the domain lifecycle
fixtures, the core source guard and `cargo check --workspace --tests --locked`.  Local tests,
builds, checks, clippy and smoke commands are intentionally not run; CI results are not awaited.

Limitations: the ledger is a pure in-process contract and does not itself append/flush an EventLog,
consume a provider permit, settle/release a quota reservation or reconcile an external provider
receipt.  The core projection is read-only and has no durable checkpoint, crash recovery, invoice
reconciliation, retry/fallback policy or live provider effect proof.  Legacy `model.prepared`
payloads remain owned by the existing budget path until a later migration; only payloads carrying
the BQ-11 schema enter this projector.

## 2026-10-17 completion slice (CI-only)

The lifecycle contract above is unchanged; this slice closes the card's refusal matrix, adds the
Core-side projector/fence fixtures and re-wires the manual lane (the pre-consolidation lane was
deleted by `08552ada`).  Nothing here promotes the proof level beyond `source`.

Behavior fixtures, denials first:

| Fixture | Assertion |
|---|---|
| `deny_unprepared_missing_permit_and_duplicate_settlement` | dispatch before `prepare`, dispatch without a permit, and a second settlement are refused |
| `deny_flush_without_append_and_mismatched_permit` | flush before the prepared append, flush sequence `0`, a mismatched permit, a nil permit and a second `prepare` are refused; the ledger stays `Prepared`/unprepared |
| `deny_terminal_lifecycle_replay` | a settled or `unknown` attempt cannot dispatch, observe, settle, mark unknown or prepare again; `unknown` keeps `receipt_id` empty |
| `deny_usage_identity_unknown_confidence_and_settlement_mismatch` | usage bound to another attempt/run, unknown-confidence usage, a drifted settlement observation and a nil receipt are refused |
| `event_contract_refuses_unflushed_and_malformed_lifecycle_payloads` | prepared revision ≠ 1, a tampered digest, nil request id / sequence `0`, an unflushed dispatch fact and schema-less `model.observed`/`model.unknown` payloads fail closed |
| `lifecycle_links_run_turn_attempt_usage_and_receipt` (extended) | the settled record binds run/turn/model-attempt/billing-attempt/reservation/permit/usage digest/receipt, validates its digest and passes `validate_runtime_event` + `validate_event_payload` for every fact |
| `unknown_attempt_retains_usage_and_error_for_reconciliation` (extended) | the failed attempt keeps its usage digest and bounded error (code/phase/`request_sent`) while no receipt exists |

Core fixtures (read-only projector and shared dispatch fence):

| Fixture | Assertion |
|---|---|
| `projection_rebuilds_only_prepared_flushed_lifecycle` | prepared → flushed → dispatching rebuilds one record with the full identity chain, a source cursor and deterministic replay; the fence refuses a dispatched record |
| `projection_rejects_dispatch_without_prepared_fact` | a first fact that is not `model.prepared` is refused |
| `projection_rejects_unflushed_schema_less_and_duplicate_facts` | unflushed dispatch, schema-less lifecycle kind, undecodable payload, kind/envelope mismatch and duplicate source event are refused |
| `projection_rejects_duplicate_settlement_revision_and_usage_identity` | second settlement, revision gap, run drift, drifted attempt key, foreign usage identity, settlement without usage, unknown without error and post-terminal re-prepare are refused |
| `projection_folds_unknown_attempts_and_ignores_legacy_prepared` | `unknown` keeps usage/error; legacy `model.prepared` and unrelated kinds stay outside the fold and do not move the cursor; empty/lifecycle-free logs are `SourceEmpty` |
| `dispatch_fence_requires_matching_permit_and_flushed_prepared_record` | the fence passes only for a flushed prepared record with the exact permit; unflushed, missing/mismatched permit and dispatched records are refused |
| `source_guard_keeps_lifecycle_projection_read_only` | markers plus negative assertions: no `reqwest::`, `std::process::Command`, `tokio::spawn`, `CapabilityBroker`, `ProviderClient`, `commit_transition`, `append_event(`, `std::fs`, `#[ignore` |
| `bq11_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shards` | fixture names, lane commands and shard registrations are pinned, and the lane stays `workflow_dispatch`-only |

Event registry: `kiana-domain/src/event_contracts.rs::model_payload_contract` now selects the
payload contract for `model.observed`/`model.unknown` as well.  A schema-less payload for those two
kinds is refused with `event_model_payload_schema_invalid` instead of being accepted by the
lifecycle field whitelist, so the log boundary fails closed before the projector sees it.

CI wiring: manual `.github/workflows/bq11-attempt-lifecycle.yml` (`workflow_dispatch` only,
`permissions: contents: read`, `cargo fetch --locked`, `cargo fmt --all --check`, both targets and
both `--no-run` compile steps).  Automatic coverage stays with the unified `ci.yml` shards
`kiana-domain-s1/4` (`bq11_attempt_lifecycle`) and `kiana-core-s1/6`
(`bq11_attempt_lifecycle_guard`); `scripts/ci/test-shards.json` was already complete for both
targets.  The `ER-01` event-contract lane still runs the domain fixture.

Limitations of this slice: local tests/builds/checks/clippy are not run (CI-only session) and the
CI receipt is not awaited; the projector still has no durable checkpoint, crash recovery or invoice
reconciliation, the ledger has no production dispatch caller yet (BQ-12+ owns settlement, BQ-17/BQ-21
own retry/recovery wiring), and no live provider effect is proven.
