# EQ-18 Canonical Trace Baseline

## Scope

EQ-18 extends the pure `kiana-quality` normalizer with one canonical JSON path. Object keys are
sorted through the existing domain `canonical_journal_bytes` boundary; event payload fields are
checked against the registered event-kind allowlist; structured values pass through the existing
trace redaction profile before encoding. Arrays preserve order by default, and multiset sorting
is available only when the caller explicitly selects `ArrayPolicy::Multiset`.

The canonical trace retains source cursor, event identity, kind, correlation/causation metadata,
redacted payload and terminal indexes. It does not silently normalize volatile values, compute
trace digests, compare traces, persist EvalStore records or dispatch a target.

## Evidence and limits

- `kiana-quality/tests/eq18_canonical.rs` covers object/array determinism, shared redaction,
  canonical event output and unknown payload field rejection.
- `kiana-quality/tests/eq18_quality_guard.rs` protects reuse of domain canonical/redaction
  boundaries and the pure/no-runtime boundary.
- `.github/workflows/eq18-trace-canonical.yml` runs the fixtures, source guard and workspace
  compilation in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: canonical/redacted source output
and CI fixtures are present, while volatile replacement, event/trace digest, diff and durable
quality evidence remain later EQ steps.
