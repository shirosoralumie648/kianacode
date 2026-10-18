# INT-19 Broker dispatch / adapter observation baseline

## Scope

INT-19 separates connector dispatch from effect observation. A connector attempt now carries a
strict, digest-bound lifecycle: `prepared → dispatching → observed → result_committed`, with
`unknown` as a terminal quarantine state. The domain transition reducer rejects skipped phases,
incomplete evidence, terminal resurrection and forged lifecycle digests.

## Implemented source slice

- `ConnectorDispatchLifecycle` is a pure domain contract with immutable invocation/attempt,
  command, binding, payload and idempotency digests. `Prepared` and `Dispatching` cannot carry
  receipt/result evidence; `Observed` requires receipt and observation digests; terminal states
  cannot transition again.
- `ConnectorEventJournal` is the only connector-local object holding `EventStorePort`. The
  capability handler reads and appends through that boundary instead of owning the EventStore
  dependency directly.
- The local fixture path uses distinct `LocalFixtureAdapter` and
  `LocalFixtureEffectObserver` objects. The journal commits `prepared` and `dispatching` before
  adapter entry, then commits `observed` and the terminal lifecycle before `connector.invoked`.
  A replay that finds an incomplete or terminal lifecycle returns `result_unknown` and does not
  invoke the adapter again.
- `connector_bindings` validates lifecycle facts and binds the final receipt/observation digests;
  no second execution loop or authorization source is introduced.

## 2026-10-17 completion slice (CI-only)

The card is completed on top of the source slice above:

- `kiana-domain/tests/int19_connector_dispatch.rs` 3 → 7 deny-first fixtures: skipped phases and
  premature artifacts (`connector_dispatch_transition_invalid`,
  `connector_dispatch_pre_effect_artifacts_forbidden`), incomplete observation/result commits,
  terminal resurrection from a late result, a replayed clone or a hand-built value
  (`connector_dispatch_terminal_resurrection`), forged headers/digests/reasons
  (`connector_dispatch_lifecycle_header_invalid`,
  `connector_dispatch_receipt_digest_invalid`/`_observation_`/`_result_`, `reason_invalid`,
  `connector_dispatch_lifecycle_digest_mismatch`, unknown fields) and full canonical digest
  coverage over identity, phase and evidence fields.
- New `kiana-daemon/tests/int19_connector_dispatch.rs` drives
  `DaemonHost → ControlPlane → Broker → EventStore` with a hash-pinned `local_fixture` binding:
  `prepared → dispatching → observed → result_committed` are four committed facts before the
  single `connector.invoked` fact, the fact re-binds the recomputed receipt/observation/result
  digests, a replayed command is effect-free (`replayed: true`, same event id, unchanged stream),
  wire-supplied `dispatch_lifecycle`/stage/observation/receipt material is refused with
  `connector_arguments_invalid` and zero connector facts, and a seeded crash-window chain
  (`dispatching`/`result_committed`/`unknown` without the fact) is reported as
  `result_unknown:connector_dispatch_incomplete` / `result_unknown:connector_result_commit_missing`
  / `result_unknown:connector_dispatch_terminal` without a second adapter entry. A foreign-command
  lifecycle chain is digest-bound and cannot block the current attempt.
- `kiana-daemon/tests/int19_connector_dispatch_guard.rs` pins the reject codes, the journal
  boundary (only `ConnectorEventJournal` may touch the EventStore; the adapter/observer hold
  neither the journal nor authorization) and the commit order before the connector fact.
- New `kiana-core/tests/int19_connector_dispatch_guard.rs` pins the source boundaries, the
  fixture names, the manual lane steps and the shard ownership.
- New manual lane `.github/workflows/int19-connector-dispatch.yml` (`workflow_dispatch`-only,
  `permissions: contents: read`, `cargo fetch --locked` + `cargo fmt --all --check` + four targets
  with `--test-threads=1` + four `--no-run` compile steps).
- Automatic coverage: `kiana-domain-s3/4` owns `int19_connector_dispatch`, `kiana-core-s4/6` owns
  `int19_connector_dispatch_guard`, and the daemon targets are picked up by the whole-crate
  `kiana-daemon` shard. `Cargo.toml`/`Cargo.lock`/`package.json` and the unified `ci.yml` are
  unchanged.

Acceptance fixtures:

| Fixture | Kind |
|---|---|
| `skipped_phases_and_premature_artifacts_are_refused_before_any_dispatch` | domain deny |
| `late_results_cannot_resurrect_a_terminal_attempt` | domain deny |
| `forged_lifecycle_headers_digests_and_reasons_fail_closed` | domain deny |
| `lifecycle_digest_covers_every_identity_phase_and_evidence_field` | domain property |
| `dispatch_and_observation_facts_precede_the_connector_fact_and_replay_is_effect_free` | daemon spine |
| `wire_supplied_lifecycle_or_observation_material_is_refused_before_any_effect` | daemon deny |
| `an_incomplete_or_terminal_attempt_without_a_fact_is_reported_unknown_and_never_reexecutes` | daemon recovery deny |
| `connector_dispatch_observation_and_journal_boundaries_are_explicit` | daemon guard |
| `dispatch_refusal_codes_and_behaviour_fixtures_stay_pinned` | daemon guard |
| `dispatch_and_observation_stay_separate_facts_on_the_single_spine` | core guard |
| `dispatch_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shards` | core guard |

## CI-only evidence

GitHub Actions runs the domain lifecycle fixtures, daemon behaviour fixtures, the two source guards,
formatting and test-target compilation. Local Cargo test/build/check/clippy/smoke commands are
intentionally not run and CI is not awaited.

## Evidence boundary and limitations

```text
feature_status: partial
proof_level: source
```

The current adapter is still a deterministic project-local fixture and does not prove an external
provider effect, durable cross-process EventLog recovery, provider receipt truth, or physical/live
outcome. The journal boundary is local to the daemon composition; a future live connector must
provide the same dispatch/observation ports and durable recovery evidence. Reconciliation and
cancel settlement remain INT-22/INT-23 work.
