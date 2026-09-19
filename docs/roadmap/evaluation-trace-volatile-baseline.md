# EQ-19 Volatile Trace Normalization Baseline

## Scope

EQ-19 adds an explicit volatile-value policy on top of the EQ-18 canonical trace. Rules name a
canonical event path and classify it as `timestamp`, `uuid`, `temp_path` or `actor`; matching
values become bounded tokens (`<TS>`, `<UUID>`, `<TEMP_PATH>`, `<ACTOR>`) and every replacement is
recorded with its path and kind. Array-member rules support an explicit `*` segment. A bounded
replacement budget prevents an unexpectedly large trace from being silently rewritten.

The normalizer also infers likely volatile values. If a UUID-like string, temporary path,
timestamp field or actor field is not covered by a rule, it returns a stable error instead of
normalizing it implicitly. Source correlation/cursor fields remain provenance metadata; canonical
bytes are computed from the normalized event values, not raw source identity fields.

## Evidence and limits

- `kiana-quality/tests/eq19_volatile.rs` covers declared replacements/counts, undeclared-value
  rejection, wildcard array rules and replacement-budget failure.
- `kiana-quality/tests/eq19_quality_guard.rs` protects the token/rule/no-I/O boundary.
- `.github/workflows/eq19-trace-volatile.yml` runs the fixtures, source guard and workspace
  compilation in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: explicit volatile normalization
and CI fixtures are present, while event/trace digest registration, diffing, capture and durable
quality evidence remain later EQ steps.
