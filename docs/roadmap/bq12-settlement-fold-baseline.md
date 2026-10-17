# BQ-12 settlement/release/unknown fold baseline

> Snapshot date: 2026-09-25. This slice adds a bounded source contract and read-only projector.
> Runtime fixtures are GitHub-only and are not run locally.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`BQ-12`](../roadmap.md#step-bq-12) |
| source snapshot | `88959eb5` plus this BQ-12 source slice |
| feature_status | `implemented` for the domain fold contract, protocol/event registry, core projector and CI fixtures |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | `reserved` → `consumed` → explicit `released`, or `reserved` → `unknown` → reconciliation |

The fold binds `RunId`, `ModelAttemptId`, billing `AttemptId`, `QuotaReservationId`, reservation
digest, optional normalized usage digest, receipt reference and source event references. Known
usage records only the measured consumption; an explicit release event can return the unused
reservation remainder. Partial usage keeps the fold occupied and is marked `partial`. Cancellation,
timeout, EOF and `result_unknown` remain `unknown` with `reconciliation_required=true`; they never
become an automatic zero or release.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| known settlement | consumed units are bounded by the reservation; only an explicit release returns the unused remainder |
| partial usage | known portion is retained and partial usage cannot be refunded before reconciliation |
| unknown result | reservation remains conservatively occupied and release is rejected |
| duplicate settlement/release | terminal transitions and conflicting digests cannot charge or release twice |
| identity/digest fence | cross-run, cross-attempt and reservation/usage source drift fail closed |
| core projector | only committed `usage.*` facts are folded; schema, kind, revision and source replay are checked |

## Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

GitHub Actions runs `cargo fetch --locked`, `cargo fmt --all --check`, the domain settlement
fixtures, the core source/projector guard and `cargo check --workspace --tests --locked`. Local
tests, builds, checks, clippy and smoke commands are intentionally not run; CI results are not
awaited.

Limitations: the ledger is a pure in-process contract and does not itself append/flush an
EventLog, consume a provider permit, mutate a quota adapter or reconcile an external receipt.
The projector is read-only and has no durable checkpoint, crash recovery, invoice correction,
retry policy, cross-process CAS or live provider effect proof. A partial or unknown fold therefore
remains occupied until a later explicit reconciliation step; no `durable`, `live` or `physical`
claim is made.

## 2026-10-17 completion slice (CI-only)

The fold contract above is unchanged; this slice closes the card's refusal matrix, adds the
conservative three-path projection fixtures and re-wires the manual lane (the pre-consolidation
lane was deleted by `08552ada`).  Nothing here promotes the proof level beyond `source`.

Behavior fixtures, denials first (`kiana-domain/tests/bq12_settlement_fold.rs`, 5 → 13):

| Fixture | Assertion |
|---|---|
| `cancel_timeout_and_eof_never_become_an_automatic_zero_or_release` | a cancelled, timed-out or EOF attempt folds to `usage.unknown` with `consumed=None`, `released` all-zero and the full reservation held; a later release, a late `consumed` fact and a late settled lifecycle are all refused and the state stays `Unknown` |
| `unknown_keeps_the_observed_portion_but_never_refunds_it` | a partial observation retained on the unknown path stays visible (`tokens=7`) but is never refunded; release is still refused |
| `observed_or_reserved_attempts_cannot_release_without_a_known_settlement` | an unsettled reservation cannot release (`settlement_not_consumed`), a forged valid release fact is a transition refusal and an observed attempt still holds `released=0` |
| `known_usage_is_never_refunded_only_the_unused_remainder_is_released` | full refund, oversized release, oversized consumption and a release without known tokens are refused with stable codes; the only accepted release is `reserved − consumed = 88`, once |
| `duplicate_settlement_and_release_cannot_charge_or_refund_twice` | an exact replay is `Duplicate` with an unchanged record; a second settlement or release is refused without moving the record |
| `identity_usage_digest_and_revision_drift_fail_closed` | foreign reservation digest, drifted usage digest, revision gap and a reused source event reference are refused, leaving the fold equal to the settled record |
| `known_partial_and_unknown_paths_produce_a_deterministic_ledger` | five paths (known released/held, partial held, unknown with/without retained usage) each fold twice to field- and digest-identical records; `reserved` is never rewritten and only the known path releases the unused remainder |
| `fold_facts_are_registered_versioned_billing_attempt_events` | every fold fact passes `validate_runtime_event` + `validate_event_payload` as a `billing_attempt` fact; a missing `reservation_id` or an extra field is refused |

Core fixtures (`kiana-core/tests/bq12_settlement_fold_guard.rs`, 4 → 8):

| Fixture | Assertion |
|---|---|
| `projector_keeps_known_partial_and_unknown_paths_conservative_and_deterministic` | known/partial/unknown rebuild deterministically; a release fact for a partial or unknown fold is refused (`settlement_released_payload_invalid`, `settlement_unknown_requires_reconciliation`) |
| `projector_rejects_duplicate_settlement_release_and_digest_drift` | duplicate settlement/release, over-refund, usage-digest drift, revision gap, source-ref replay, duplicated runtime event id and a foreign attempt are refused with stable reasons |
| `projector_ignores_non_fold_usage_events_and_reports_empty_sources` | cost/model noise is skipped without moving the cursor; empty or fold-free logs are `SourceEmpty`; schema-less, undecodable and kind-mismatched envelopes fail closed |
| `source_guard_keeps_fold_projector_read_only_and_conservative` | markers plus negative assertions: no `commit_transition`, `append_event(`, `std::fs`, `std::net`, `TcpStream`, `unsafe`, `#[ignore`, `Uuid::new_v4` |
| `bq12_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shards` | fixture names, lane commands and shard registrations are pinned, and the lane stays `workflow_dispatch`-only |

CI wiring: manual `.github/workflows/bq12-settlement-fold.yml` (`workflow_dispatch` only,
`permissions: contents: read`, `cargo fetch --locked`, `cargo fmt --all --check`, both test targets
and both `--no-run` compile steps).  Automatic coverage stays with the unified `ci.yml` shards
`kiana-domain-s1/4` (`bq12_settlement_fold`) and `kiana-core-s1/6`
(`bq12_settlement_fold_guard`); `scripts/ci/test-shards.json` was already complete for both targets.

Limitations of this slice: local tests/builds/checks/clippy are not run (CI-only session) and the
CI receipt is not awaited.  The fold is still an in-process contract: no EventLog append/flush
receipt, no provider permit consumption, no durable settlement ledger or projector checkpoint, no
explicit reconciliation execution path, no invoice correction and no retry/fallback wiring
(BQ-17/BQ-21 own those) and no live provider effect proof.
