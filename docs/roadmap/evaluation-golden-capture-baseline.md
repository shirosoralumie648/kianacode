# EQ-23 GoldenTrace Capture Baseline

## Scope

EQ-23 adds a pure typed capture contract for creating a new `GoldenTrace` from exactly one
explicit source: a committed run reference with source-event/receipt digests or an opaque fixture
reference with fixture digest. Capture validates the source metadata, constructs a fresh domain
`GoldenTraceId`, and returns a logical destination receipt. An occupied destination reference is
rejected before construction, so an existing golden file/version cannot be overwritten.

The destination is an opaque logical version slot, not a filesystem path. This step does not read
files, write an EvalStore, capture live events, replay a run, call a provider or alter an existing
GoldenTrace.

## Evidence and limits

- `kiana-quality/tests/eq23_capture.rs` covers explicit fixture capture, run binding, occupied
  destination rejection and invalid source/destination metadata.
- `kiana-quality/tests/eq23_capture_guard.rs` protects the explicit-source/no-filesystem and
  no-overwrite boundary.
- `.github/workflows/eq23-golden-capture.yml` runs the fixtures, source guard and workspace
  compilation in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: typed capture source and CI
fixtures are present, while source-run collection, fixture loading, immutable storage, capture
authorization and durable provenance remain later work.
