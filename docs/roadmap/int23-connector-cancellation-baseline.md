# INT-23 connector cancellation baseline

## Scope

INT-23 makes connector cancellation reconstructible from server-owned stop evidence. A confirmed
stop before dispatch settles as `not_executed` and may release the lease; a started effect keeps its
lease held for reconciliation; an unconfirmed stop is `unknown`. Every terminal settlement fences
late provider results.

## Implemented source slice

- `ConnectorCancellationSettlement::from_stop_report` binds invocation, attempt, command, permit,
  lease and stop report digests. It rejects cancellation after an observation already exists.
- `NotExecuted`, `StopConfirmed` and `Unknown` have explicit lease settlement rules. Started or
  unconfirmed work cannot be marked released, and `late_result_fenced` is required for every
  terminal settlement.
- `ConnectorCancelRequest`/`cancel_checked` and the existing `StopReport` contract remain the
  adapter boundary; the domain settlement is pure and does not send signals, release leases or
  append facts itself.

## CI-only evidence

GitHub Actions runs the cancellation settlement fixtures, source guard, formatting and workspace
test-target compilation. Local Cargo test/build/check/clippy/smoke commands are intentionally not
run and CI is not awaited.

## 2026-10-17 completion slice (CI-only)

This slice closes the card's exit criteria on top of the settlement contract:

- `ConnectorCancellationRecord` (schema `kiana.connector-cancellation-record.v1`) stores the
  server-owned stop evidence next to the settlement digest, so a cancellation cannot exist without
  stop evidence. `ConnectorCancellationRecord::from_stop_report` is the only constructor, and
  `reconstruct_outcome` re-derives the terminal outcome from the stored `StopReport` instead of
  trusting the stored settlement: a mismatch is refused with
  `connector_cancellation_settlement_mismatch`.
- `expected_settlement` now requires a confirmed stop of a `Dispatching` attempt to carry
  `report.term_sent || report.kill_sent`; a "confirmed" report without any signal is refused with
  `connector_cancellation_stop_attempt_missing` instead of settling as `not_executed`.
- `late_result_disposition` classifies late provider material and returns
  `ConnectorLateResultDisposition::Fenced`; `record_late_result` keeps it as fenced evidence
  (`connector_cancellation_late_result_fenced`) and can never resurrect a settled attempt.
- `validate_for_stop_report`, `validate_for_lifecycle`, `validate_fenced_late_results` and
  `digest` bind the record to the lifecycle, the stop report and the fenced late results, refusing
  forged records with `connector_cancellation_record_invalid` or
  `connector_cancellation_record_digest_mismatch`.

### Fixtures

| target | fixture | proves |
|---|---|---|
| `kiana-domain/tests/int23_connector_cancellation.rs` | `a_cancellation_without_confirmed_stop_evidence_never_reports_not_executed` | unconfirmed/signal-less "confirmed" stops never settle as `not_executed` (`connector_cancellation_stop_attempt_missing`) |
| same | `a_started_effect_cannot_release_its_lease_or_be_recorded_as_not_executed` | a started effect keeps its lease (`connector_cancellation_started_effect_cannot_release_lease`); forged releases and forged records are refused |
| same | `a_late_result_cannot_resurrect_a_settled_attempt` | late material is fenced, an already-observed effect is refused, invocation/attempt mismatches are refused |
| same | `the_three_cancellation_outcomes_reconstruct_from_stop_evidence` | `not_executed` / `stop_confirmed` / `unknown` all reconstruct from the stored stop report; tampered settlements/records are refused |
| `kiana-daemon/tests/int23_connector_cancellation.rs` | `wire_supplied_cancellation_material_is_refused_before_any_fact` | wire-supplied cancellation/settlement/stop-report keys are refused (`connector_arguments_invalid`) before any fact is appended |
| same | `a_settled_attempt_reconstructs_from_durable_facts_and_fences_late_results` | a settled attempt reconstructs from durable `connector.dispatch.lifecycle` facts; a signal-less "confirmed" stop is refused with `connector_cancellation_stop_attempt_missing`; late results are fenced; a cancellation after observation is refused (`connector_cancellation_effect_already_observed`) |
| `kiana-core/tests/int23_connector_cancellation_guard.rs` | `connector_cancellation_settlement_fences_late_results_and_lease_release` | source markers/codes, single constructors, no forbidden execution surface |
| same | `cancellation_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shards` | deny-first ordering, lane steps, `--no-run` compile steps, shard ownership |

Evidence quote: this worktree's only runnable proof for the new fixtures is the manual lane
`.github/workflows/int23-connector-cancellation.yml` plus the automatic shards
`kiana-domain-s3/4`, `kiana-core-s4/6` and the whole-crate `kiana-daemon` shard.

## Evidence boundary and limitations

```text
feature_status: partial
proof_level: source
```

This slice does not wire a live connector cancel command, process supervisor, durable lease store,
reconciliation inbox delivery or provider outcome. It supplies the typed settlement/fence contract
for the later daemon and recovery integrations. Local Cargo build/test/check/clippy/smoke commands
were intentionally not run in this session; the CI-only lane above is the intended proof path and
its results are not awaited here.
