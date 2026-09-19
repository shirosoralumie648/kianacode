# EQ-22 Declarative Assertion Baseline

## Scope

EQ-22 adds a bounded pure assertion DSL to `kiana-quality`. It supports exact JSON comparison,
ordered arrays, duplicate-preserving multiset arrays, numeric absolute/relative tolerance, regular
expression matching and string/array/object `contains`. Assertions target explicit dot paths and
return stable mismatch codes plus redacted bounded expected/actual summaries; evaluation does not
mutate the input value or unrelated fields.

Assertion specifications fail closed on unknown schema, wrong expected type, invalid/oversized
regex, invalid tolerance and more than the bounded assertion count. This step does not run a
provider, evaluate model quality, persist findings or authorize promotion.

## Evidence and limits

- `kiana-quality/tests/eq22_assertions.rs` covers all assertion modes, ordered-vs-multiset
  semantics, unrelated-field isolation, stable failure codes, redaction and invalid specs.
- `kiana-quality/tests/eq22_assertions_guard.rs` protects the pure regex/summary-bounded boundary.
- `.github/workflows/eq22-assertions.yml` runs the fixtures, source guard and workspace compilation
  in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: assertion source and CI fixtures
are present, while evaluator aggregation, baseline comparison, capture and durable quality
evidence remain later EQ steps.
