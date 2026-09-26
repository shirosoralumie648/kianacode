# EQ-37 retry-once flake classifier baseline

## Scope

EQ-37 adds `FlakeClassifierEvaluator` with a bounded two-attempt contract. A first failure
followed by a pass is classified as flaky and must remain `NeedsReview`/`Quarantined` with an
expiring owner/evidence record; it is never a Pass. Infrastructure attempts require a known
runner/fixture/network/resource class and cannot be silently counted as a case result. Retrying
after a pass, malformed attempt order, missing quarantine or unknown infra classification fails
closed.

The evaluator only classifies caller-supplied attempt evidence. It does not rerun a case, quarantine
files, mutate a suite, change a gate or promote a result.

## Evidence and limits

- `kiana-quality/tests/eq37_flake.rs` covers stable pass, flaky-not-pass with quarantine, infra
  classification, retry-after-pass and strict fields.
- `kiana-quality/tests/eq37_flake_guard.rs` protects the retry-once/quarantine-first boundary.
- `.github/workflows/eq37-flake.yml` runs fixtures, source guard, formatting and workspace
  test-target compilation in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: no real rerun, durable quarantine,
infra telemetry, statistical flake confidence, promotion authority, live or physical evidence is
claimed.
