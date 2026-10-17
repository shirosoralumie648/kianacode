# EQ-27 deterministic evaluator baseline

## Scope

EQ-27 adds a provider independent deterministic evaluator boundary to `kiana-quality`. A strict
`kiana.quality-finding.v1` finding carries `code`, `expected`, `actual`, `message` and a single
`evidence_ref`. Stable identifiers, canonical JSON values, redaction, size/depth limits and
`deny_unknown_fields` make malformed or unsafe findings fail closed. `EvaluatorRegistry` invokes
registered evaluators in stable identifier order and sorts all findings by a canonical key before
returning them; the finding-set digest includes every evidence reference.

The trait accepts caller supplied JSON only. It does not own an EventLog, start a Runner, contact a
provider, read the filesystem or dispatch a capability. Domain-specific runtime, safety, evidence,
recovery and context checks remain EQ-28 and later steps.

## Evidence and limits

- `kiana-quality/tests/eq27_evaluator.rs` covers stable code/evidence fields, order independence,
  strict unknown-field handling, secret rejection/redaction, bounds and duplicate evaluator IDs.
- `kiana-quality/tests/eq27_evaluator_guard.rs` protects the pure no-provider/no-runner boundary
  and the required schema/redaction/limit markers.
- `.github/workflows/eq27-evaluator.yml` runs the fixtures, source guard, formatting and workspace
  test-target compilation in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: source contracts and CI wiring
are present, while no durable EvalStore, real target execution, provider/model quality result,
promotion authority or live/physical evidence is claimed.

## 2026-10-17 completion slice (CI-only)

The contract above is unchanged. This slice closes the card's acceptance from the refusal side: the
card-named `every_finding_has_stable_code_and_evidence` only observed findings that were built by
`Finding::new`, so a finding that bypassed the constructor, or a caller that passed an unstable code
or a missing/secret-shaped evidence reference, had no fixture. Nothing here promotes the proof level
beyond `source`; local tests/builds/checks/clippy/smoke are not run and the CI receipt is not awaited.

Behavior fixtures, denials first (`kiana-quality/tests/eq27_evaluator.rs`, 4 → 9):

| Fixture | Assertion |
|---|---|
| `every_finding_requires_a_stable_code_and_evidence_ref` | seven unstable code shapes plus an over-long code → `quality_finding_code_invalid`; empty, whitespace/newline-bearing and over-long evidence refs → `quality_finding_evidence_ref_invalid`; blank/newline/over-long message → `quality_finding_message_invalid`; a secret-bearing message → `quality_finding_secret_detected:message`; an over-limit `expected` value → `quality_finding_value_too_large:expected`; a valid finding keeps `runtime.terminal` + `event:1#terminal`, and a tampered schema → `quality_finding_schema_invalid` |
| `registry_never_returns_a_finding_without_a_stable_code_or_evidence_ref` | findings constructed field-by-field (bypassing `Finding::new`) are refused by the registry before anything is returned: `EvaluatorError::Finding(CodeInvalid)` for an unstable code and `Finding(EvidenceRefInvalid)` for an empty or whitespace-bearing evidence reference |
| `registry_limits_and_input_bounds_fail_closed` | `MAX_EVALUATORS` full registry → `quality_evaluator_limit_exceeded`; an unstable evaluator id → `quality_evaluator_id_invalid` and an empty registry; an input over `MAX_EVALUATOR_INPUT_BYTES` → `quality_evaluator_input_too_large`; `MAX_FINDINGS + 1` emitted findings → `quality_evaluator_finding_limit_exceeded` |
| `finding_digest_binds_every_evidence_ref_and_value` | changing code, `expected`, `actual` or `evidence_ref` each changes `digest_findings`; emission order does not (canonicalized copies share one digest); `EvaluationResult::new` refuses an empty or duplicated evaluator id set, and a recomputed `findings_digest` mismatch → `quality_finding_value_invalid:findings_digest` |
| `evaluator_input_is_versioned_and_bounded` | `EvaluationInput` requires `kiana.quality-evaluator-input.v1` (a stale schema → `quality_evaluator_input_schema_invalid`), refuses an invalid evidence reference (`Finding(EvidenceRefInvalid)`), is bounded by `MAX_EVALUATOR_INPUT_BYTES`, and rejects unknown fields on decode |

Guard and wiring (`kiana-quality/tests/eq27_evaluator_guard.rs`):

- `evaluator_stays_pure_and_bounded` keeps the forbidden-dependency scan (`std::fs`, `std::net`,
  `std::process`, `tokio::`, `reqwest::`, provider/broker/daemon/harness/EventStore/ControlPlane,
  `Command::new`, `include_str!`, `#[ignore`, `unsafe`) and now pins the code/message/evidence/input
  bound markers plus `digest_findings`/`validate_evidence_ref`, the six fixture names, and that no
  fixture disables itself with `#[ignore`/`--ignored`.
- `eq27_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shard` binds
  `.github/workflows/eq27-evaluator.yml` (dispatch-only, `contents: read`,
  `cargo fetch --locked`, format gate, both targets with `--test-threads=1`, both `--no-run`
  compile steps) and the `kiana-quality` entry (`targets: null`) of `scripts/ci/test-shards.json`,
  which is what makes the unified `ci.yml` discover both targets without a separate registration.

The lane was missing `cargo fetch --locked` and the `--no-run` compile steps that the newer lanes
use; it now matches that convention and stays manual-only, so the automatic receipt is the
`kiana-quality` shard of `ci.yml`.
