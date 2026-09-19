# EQ-20 Version-Bound Evidence Digest Baseline

## Scope

EQ-20 adds version-bound SHA-256-style digests for normalized events, traces, artifacts and
receipts in the pure `kiana-quality` crate. Event digests include the normalized event value,
source cursor and kind; trace digests include the ordered event digest list, array policy, cursor
range, source normalizer version and volatile replacement accounting. Artifact and receipt values
use the same structured canonical digest envelope. Every digest includes the exact
`normalization_version`, so changing normalization rules cannot silently reuse an old digest.

This step computes immutable evidence fingerprints only. It does not persist an EvalResult,
capture source runs, compare candidates, call a provider or authorize a promotion.

## Evidence and limits

- `kiana-quality/tests/eq20_digest.rs` covers key-order stability, normalization-version binding,
  event/trace/artifact/receipt kind separation and empty/wrong-version rejection.
- `kiana-quality/tests/eq20_digest_guard.rs` protects the version-bound/no-effect boundary.
- `.github/workflows/eq20-trace-digest.yml` runs the fixtures, source guard and workspace
  compilation in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: digest source and CI fixtures
are present, while trace diff, capture, evaluator findings and durable EvalStore evidence remain
later EQ steps.
