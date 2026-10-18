# EQ-40 QualityCandidate binding baseline

## Scope

EQ-40 adds a single-primary-dimension `QualityCandidate` contract to `kiana-quality`. A candidate
records exactly one changed version dimension (model, prompt, tool catalog, memory index, workflow
or route), one explicit passive version reference for every other snapshot dimension, the recomputed
snapshot digest, the bound baseline identity/digest/suite and its own canonical digest. Hidden
model/prompt/tool drift, missing or conflicting passive references, unknown dimensions, a no-op
change and snapshot/digest tampering fail closed, and admission refuses a candidate whose bound
baseline identity, baseline digest, suite digest or validity window does not match the supplied
EQ-39 baseline record.

The evaluator validates a value object only. It does not alter configuration, route, grant, policy,
receipt or gate state, does not run a target, does not persist a candidate and does not decide a
quality gate.

## Completion slice (2026-10-17, CI-only)

The 2026-09-26 slice was one candidate value object with four fixtures and a lane without a locked
fetch or a compile step. This slice closes the card exit conditions in source and CI wiring:

- `QualityCandidate` (schema `kiana.quality-candidate.v2`) binds `candidate_id`, the baseline
  `baseline_id` + `baseline_digest`, the baseline `suite_digest`, the single `changed_dimension` +
  `changed_version_ref`, the three version maps (`base_versions`, `passive_version_refs`,
  `snapshot_versions`), the recomputed `snapshot_digest`, `owner_id`, `status` and the canonical
  `candidate_digest`; every field is covered by `candidate_digest`.
- `QualityCandidate::new` derives the passive references and the candidate snapshot from the
  baseline versions, so a caller cannot construct a candidate that hides a second change, omits a
  passive reference or changes nothing. `validate` re-checks the same invariants on decoded values:
  `candidate_header_invalid` (schema, bounded identities, canonical lowercase `sha256:` digests,
  map sizes), `candidate_dimension_unknown` (a version map may only name the six known dimensions),
  `candidate_changed_dimension_missing`, `candidate_changed_dimension_unchanged`,
  `candidate_primary_dimension_invalid` (the primary dimension appears as passive or the snapshot
  disagrees), `candidate_passive_version_drift` (any other dimension differs between the baseline
  versions, the passive reference and the candidate snapshot), `candidate_snapshot_digest_mismatch`
  and `candidate_digest_mismatch`.
- Admission is a separate evaluator (`candidate-baseline-admission`,
  `kiana.quality-candidate-admission-input.v1`) that binds one candidate to the EQ-39
  `BaselineRecord` it claims to change at a caller-supplied `now_unix_ms`: an invalid record emits
  `candidate.baseline_invalid`, an invalid candidate emits `candidate.binding_invalid` before any
  baseline check, a foreign baseline identity or digest emits `candidate.baseline_identity_mismatch`
  per axis, a suite drift emits `candidate.suite_digest_mismatch`, a comparison before
  `created_at_unix_ms` emits `candidate.baseline_not_yet_valid`, a comparison at or after
  `expires_at_unix_ms` emits `candidate.baseline_stale`, and any status other than `Draft` emits
  `candidate.status_not_admissible`. Only an empty finding list is admissible.
- The candidate-only evaluator (`candidate-binding`, `kiana.quality-candidate-input.v2`) keeps the
  card acceptance behaviour: hidden drift is reported as `candidate.binding_invalid` with the
  specific reject reason as the finding `actual`, and findings cite `candidate:input` or
  `candidate:admission` as their evidence section.
- Typed builders (`QualityCandidate::new`, `CandidateInput::new`/`as_value`,
  `CandidateAdmissionInput::new`/`as_value`), stable keys (`CandidateDimension::key`/`from_key`,
  `CandidateStatus::key`) and the schema/limit constants (`CANDIDATE_SCHEMA`,
  `CANDIDATE_INPUT_SCHEMA`, `CANDIDATE_ADMISSION_INPUT_SCHEMA`, `CANDIDATE_EVALUATOR_ID`,
  `CANDIDATE_ADMISSION_EVALUATOR_ID`, `MAX_CANDIDATE_DIMENSIONS`, `MAX_CANDIDATE_TEXT_BYTES`) are
  exported so fixtures and later steps share one contract.

## Acceptance fixtures (deny first)

| Fixture | Proves |
|---|---|
| `candidate_cannot_hide_model_or_prompt_drift` | the card acceptance: snapshot-only model drift, a hidden primary prompt change and a re-sealed rewritten snapshot are each refused, and the rewritten snapshot is a different candidate identity that cannot reuse the earlier digest |
| `passive_version_references_must_cover_every_unchanged_dimension` | missing, extra, foreign-key and drifted passive references fail closed |
| `a_second_changed_dimension_is_refused` | a second snapshot change trips passive drift; a maintained two-dimension change moves both digests; a no-op change is refused by the constructor |
| `unknown_dimensions_and_no_op_changes_are_refused` | a renamed map key, a missing primary dimension (constructor and decoded value) and a decoded no-op change are refused |
| `snapshot_or_candidate_digest_tamper_is_rejected` | snapshot/candidate digest tamper, schema drift, non-canonical digests, whitespace identity, oversized text and empty maps are refused, and the evaluator reports the reject reason |
| `rewriting_the_version_snapshot_changes_the_candidate_identity` | a changed primary version, a rewritten base map, an earlier identity over a rewritten snapshot and a foreign snapshot digest are distinguished |
| `candidate_envelopes_and_unknown_fields_fail_closed` | both envelopes reject schema drift, unset admission time and unknown fields at the envelope, candidate and record level |
| `admission_requires_the_bound_baseline_identity_and_digest` | foreign identity/digest (both axes), digest-only drift, suite drift and a tampered record are refused before admission |
| `admission_refuses_a_baseline_outside_its_validity_window` | not-yet-valid and stale baselines are refused; the window is inclusive at its start and exclusive at its end |
| `admission_refuses_a_candidate_that_is_not_a_draft` | every non-draft status is refused with its stable status key |
| `invalid_candidates_are_never_admitted` | a broken candidate short-circuits every baseline check, even a stale window |
| `a_single_prompt_change_is_admissible_against_its_baseline` | the success path: one prompt change with five explicit passive references passes both evaluators |
| `admission_output_is_deterministic_and_registry_wired` | repeated evaluation is stable, findings canonicalize, and both evaluators register with the expected identifiers |
| `typed_constructors_round_trip` | candidate/envelope serialization round trips, digest binding and the stable dimension/status keys |

## Evidence and limits

- `kiana-quality/tests/eq40_candidate.rs` covers the 14 rejection-first fixtures above; the success
  fixtures stay last.
- `kiana-quality/tests/eq40_candidate_guard.rs` protects the single-dimension/no-authority boundary
  (no filesystem/network/process/store/broker dependency), the marker and reject-code matrix, the
  fixture names with deny-before-success ordering, the manual lane's exact steps and the automatic
  `kiana-quality` shard (`targets: null`).
- `.github/workflows/eq40-candidate.yml` runs `cargo fetch --locked`, the format gate, both fixture
  targets with `--test-threads=1` and a locked compile of the `kiana-quality` test targets in GitHub
  Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: no candidate store, target
execution, QualityGate decision, promotion/rollback, live or physical evidence is claimed.

## Known limits

- The evaluator validates caller-supplied candidate/version evidence only; it cannot prove that the
  version maps describe the real model/prompt/tool/memory/workflow/route versions or that a target
  was ever executed with them.
- `base_versions` is a caller claim bound by `candidate_digest`: a caller that rewrites the base map
  and re-seals produces a different candidate identity, which is detectable, but there is no durable
  baseline version snapshot that pins the base map to the captured baseline yet.
- `baseline_digest`/`suite_digest` binding is the only link to EQ-39; owner identity on both records
  is a bounded token, not authenticated authority, and matching owners is not required (or proven).
- Admission never persists anything, never changes a route, grant, policy or gate and never decides
  a gate verdict; mapping these findings into blocking rules belongs to EQ-41/EQ-42/EQ-43.
- The manual lane is `workflow_dispatch`-only; automatic coverage depends on the unified `ci.yml`
  `kiana-quality` shard (both targets are discovered because `targets` is null).
