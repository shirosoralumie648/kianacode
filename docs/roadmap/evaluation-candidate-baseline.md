# EQ-40 QualityCandidate binding baseline

## Scope

EQ-40 adds a single-primary-dimension `QualityCandidate` contract. A candidate binds baseline and
suite digests, one changed model/prompt/tool/memory/workflow/route version, explicit passive version
refs for every other snapshot dimension, a recomputed snapshot digest, owner and candidate digest.
Hidden model/prompt/tool drift, snapshot tampering and missing passive refs fail closed.

The evaluator validates a value object only. It does not alter configuration, route, grant, policy,
receipt or gate state, and does not run a target or persist a candidate.

## Evidence and limits

- `kiana-quality/tests/eq40_candidate.rs` covers a valid single prompt change, hidden model/passive
  drift, snapshot/candidate digest tampering and strict fields.
- `kiana-quality/tests/eq40_candidate_guard.rs` protects the single-dimension/no-authority boundary.
- `.github/workflows/eq40-candidate.yml` runs fixtures, source guard, formatting and workspace
  test-target compilation in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: no candidate store, target
execution, QualityGate decision, promotion/rollback, live or physical evidence is claimed.
