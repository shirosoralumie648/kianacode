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
