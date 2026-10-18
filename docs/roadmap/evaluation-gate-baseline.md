# EQ-41 QualityGate configuration/decision baseline

## Scope

EQ-41 separates an immutable `QualityGateConfig` from an immutable `QualityGateDecision`. A
decision binds the exact gate config version (identity, version, config digest, suite digest) and
never embeds a copy of the configuration; a configuration update requires a strictly higher version
and a distinct digest, while old decisions remain byte-identical and bound to the original config.
A Pass that carries blocking findings is structurally impossible.

This is a pure contract/evaluator. It does not persist a gate, mutate an old decision, authorize a
route, promote or revert a candidate, or execute a target.

## Contract (source of truth: `kiana-quality/src/gate.rs`)

Wire schemas: `kiana.quality-gate-config.v2` (`GATE_CONFIG_SCHEMA`),
`kiana.quality-gate-decision.v2` (`GATE_DECISION_SCHEMA`),
`kiana.quality-gate-config-history.v1` (`GATE_CONFIG_HISTORY_SCHEMA`),
`kiana.quality-gate-decision-log.v1` (`GATE_DECISION_LOG_SCHEMA`), with envelopes
`kiana.quality-gate-input.v2` (`GATE_INPUT_SCHEMA`) and `kiana.quality-gate-log-input.v1`
(`GATE_LOG_INPUT_SCHEMA`). Both evaluators register under stable identifiers `quality-gate`
(`GATE_EVALUATOR_ID`) and `quality-gate-log` (`GATE_LOG_EVALUATOR_ID`).

- `QualityGateConfig` carries identity (`gate_id`), monotone `version` starting at 1, `suite_digest`,
  named per-mille `thresholds` from the closed `GATE_THRESHOLD_DIMENSIONS` vocabulary (8 dimensions,
  including `policy_safety`, `evidence_completeness`, `recovery_correctness` and
  `replay_correctness`), sorted/unique `blocking_rules` from the closed `GATE_BLOCKING_RULES`
  vocabulary (the six `EQ-42` rules: `evidence`, `fixture_integrity`, `forbidden_effect`, `infra`,
  `replay`, `safety`) and a canonical `config_digest`. It carries no verdict, candidate, approver or
  decision-time field. An unknown threshold or rule fails closed.
- `QualityGateDecision` carries `decision_id`, the four binding axes copied from the config
  (`gate_id`, `gate_version`, `config_digest`, `suite_digest`) plus `candidate_id`,
  `candidate_digest`, `verdict`, `score_delta_milli`, `blocking_findings`, `known_regressions`,
  `evidence_ref`, `source_cursor`, `decided_at_unix_ms` and a canonical `decision_digest`. It
  carries no threshold or rule field.
- Verdict consistency matrix: `Pass` requires no blocking findings and no known regressions;
  `Reject` and `Rollback` require at least one of the two; `NeedsShadow` may not conceal a blocking
  finding. A decoded `Pass` with injected blockers is refused as
  `gate_decision_verdict_inconsistent` and reported as `gate.pass_with_blocking_findings`.
- Update law (`quality_gate_config_update`): the current config must validate
  (`gate_config_update_current_invalid`), the replacement must validate, share the gate identity
  (`gate_config_update_identity_mismatch`) and carry a strictly higher version
  (`gate_config_update_version_not_incremented`). Because the version is part of the digested
  content, two published versions can never share a digest, and the update never mutates `current`.
- Binding law (`quality_gate_decision_binding`): identity, version, config digest and suite digest
  must all match the exact configuration version the decision names
  (`gate_decision_config_identity_mismatch`, `gate_decision_gate_version_mismatch`,
  `gate_decision_config_digest_mismatch`, `gate_decision_suite_digest_mismatch`).
- `GateConfigHistory` is append-only per gate: versions are consecutive from 1, a published version
  can never be rewritten (`gate_config_version_rewritten`), ordering cannot regress or skip
  (`gate_config_version_regressed`, `gate_config_version_gap`), and an identical replay returns
  `Ok(false)` without mutating the history digest.
- `QualityGateDecisionLog` is append-only per gate: an identical replay returns `Ok(false)`; the
  same `decision_id` can never be rewritten (`gate_decision_rewritten`); a candidate may be decided
  only once per config version (`gate_decision_candidate_redecided`); the config version, the
  evidence `source_cursor` (strictly increasing) and `decided_at_unix_ms` (non-decreasing) cannot
  regress (`gate_decision_log_version_regressed`, `gate_decision_log_cursor_regressed`,
  `gate_decision_log_time_regressed`). Re-deciding a candidate requires publishing a new gate
  version first.
- `evaluate_gate_log` binds the log to the published history: identity drift
  (`gate.log_identity_mismatch`), a decision that names an unpublished version
  (`gate.log_entry_version_ahead`) or cites a rewritten digest for a published version
  (`gate.log_entry_config_mismatch`) are refused.

## Completion slice (2026-10-17)

The earlier narrow slice (config + decision with a single `updated_config` input) is replaced by
the full v2 contract above: append-only `GateConfigHistory`/`QualityGateDecisionLog`, the two
evaluators with their own envelopes, the closed threshold/blocking-rule vocabularies and the
verdict consistency matrix. No other crate referenced the v1 types.

## Acceptance fixtures (`kiana-quality/tests/eq41_gate.rs`)

Deny paths first; the success/round-trip fixture is last.

| fixture | law under test |
| --- | --- |
| `gate_config_update_does_not_mutate_old_decision` | card acceptance: a legal update leaves the old decision byte-identical; the log refuses a rewrite and a same-version re-decision, and allows re-deciding under a newer version |
| `gate_config_and_decision_are_separate_value_contracts` | config carries no verdict/candidate fields, decision carries no thresholds/rules |
| `gate_config_rejects_unknown_thresholds_and_blocking_rules` | closed vocabularies, sorted/unique/bounded thresholds and rules |
| `gate_config_digest_tamper_is_refused` | digest binding, schema drift, version zero, invalid digest, whitespace/oversized identity |
| `gate_decision_verdict_matrix_is_enforced` | the four verdict consistency rules and `gate.pass_with_blocking_findings` |
| `gate_decision_digest_and_header_tamper_is_refused` | decision header bounds, digest binding, ordering of reasons/evidence |
| `gate_decision_must_bind_the_exact_config_version` | all four binding axes, separately |
| `gate_update_requires_a_newer_distinct_version` | update law axes, including invalid current/replacement configs |
| `gate_decision_log_is_append_only_and_replay_safe` | replay is a no-op; history rewrite is refused |
| `gate_decision_log_refuses_a_rewritten_decision` | same `decision_id`, different content, on the wire and in-appended |
| `gate_decision_log_refuses_redeciding_a_candidate_under_the_same_config` | one decision per candidate per config version; a newer version allows a new decision |
| `gate_decision_log_refuses_a_config_version_regression` | config version, cursor and time ordering |
| `gate_log_entries_must_bind_the_published_config_history` | history/log identity, ahead-of-history and rewritten-digest entries, gapped history, envelope strictness |
| `gate_contracts_round_trip_and_are_registry_wired` | JSON round trips, deterministic repeat evaluation, registry wiring for both evaluators |

`kiana-quality/tests/eq41_gate_guard.rs` keeps the source boundary honest: the source contains none
of the forbidden persistence/authority/provider imports, every schema/limit/reject/finding marker
stays present, both targets stay bound to `.github/workflows/eq41-gate.yml`, and the automatic
`kiana-quality` shard (`scripts/ci/test-shards.json`, `targets: null`) keeps discovering them.

## Evidence and limits

- `.github/workflows/eq41-gate.yml` is manual-only (`workflow_dispatch`), read-only
  (`contents: read`), fetches locked dependencies, gates formatting, runs both targets with
  `--test-threads=1` and compiles the whole `kiana-quality` test target set with `--locked`.
- Local tests are intentionally not executed in this slice; the workflow is the first integrated
  receipt and is not awaited here.

This slice is `feature_status=implemented`, `proof_level=source`: no durable gate store, no
ControlPlane authorization, no promotion or rollback, no live or physical evidence is claimed.

## Known limits

- Re-deciding a candidate under the same configuration version is refused by design; a caller that
  wants to revisit a decision must publish the next gate version first.
- The verdict matrix constrains recorded evidence (blocking findings, known regressions), not the
  evaluator scores that produced it; nothing here recomputes a score.
- No persistence, no route mutation, no approval, no receipt and no execution: the contracts are
  values, and the two evaluators are pure functions over caller-supplied JSON.
