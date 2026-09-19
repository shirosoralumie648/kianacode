# EQ-25 Evaluation Catalog Baseline

## Scope

EQ-25 adds a deterministic scenario catalog builder with curated and deep tiers. Deep entries are
visible as `skipped` until `include_deep` is explicitly enabled; entries without a golden digest
remain visible as `no_golden_trace` rather than being counted as ready. Duplicate dedupe keys fold
into one stable entry with `duplicate_count`, and output ordering is independent of input order.

The catalog is metadata only. It does not discover files, load fixtures, execute reference or
candidate scenarios, create golden traces, or turn a missing golden into a pass.

## Evidence and limits

- `kiana-quality/tests/eq25_catalog.rs` covers deep opt-in, no-golden visibility, stable sorting,
  duplicate folding, permutation stability and invalid input rejection.
- `kiana-quality/tests/eq25_catalog_guard.rs` protects the visible-skip/no-I/O boundary.
- `.github/workflows/eq25-catalog.yml` runs the fixtures, source guard and workspace compilation in
  GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: catalog source and CI fixtures
are present, while fixture discovery, execution, golden capture, evaluator aggregation and
durable catalog storage remain later work.
