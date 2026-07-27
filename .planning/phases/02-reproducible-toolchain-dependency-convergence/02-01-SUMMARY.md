---
phase: 02-reproducible-toolchain-dependency-convergence
plan: "01"
status: complete
commit: c0bd383
duration: ~20min
tasks_completed: 3
files_changed: 8
---

# Plan 02-01 Summary

## What Was Done

Wave 1 of Phase 2: locked the Rust toolchain declaration and established the build-inputs reproducibility schema.

### Task 1 — rust-toolchain.toml + upgrade policy doc
- Created `rust-toolchain.toml` at repo root: `channel = "stable"`, `components = ["rustfmt", "clippy", "rust-src"]`, upgrade policy comment pointing to docs.
- Created `docs/toolchain-upgrade-policy.md`: explains stable channel rationale, upgrade steps (2-3 weeks wait, one point version at a time), MSRV constraint, CI note on @master.
- Verification: `python3 -c "import tomllib ..."` passes; all three components present; channel = "stable".

### Task 2 — kiana-build-inputs.v1 schema + fixtures
- Created `docs/schemas/kiana-build-inputs.v1.schema.json`: 7 required fields (schema, toolchain_channel, toolchain_version, cargo_lock_hash, source_hash, build_timestamp, ci_run_id), `additionalProperties: false`.
- Created `scripts/fixtures/valid-build-inputs.json` and `scripts/fixtures/invalid-build-inputs-missing-version.json`.
- Extended `scripts/schema-contract-smoke.sh` to validate both fixtures: valid passes, invalid-missing-version fails as expected.
- Verification: `bash scripts/schema-contract-smoke.sh` exits 0.

### Task 3 — CI toolchain fix (@stable → @master)
- Updated `.github/workflows/release-smoke.yml` line 14: `dtolnay/rust-toolchain@stable` → `dtolnay/rust-toolchain@master`.
- Updated `.github/workflows/release.yml` line 42: same substitution.
- Verification: `grep -c "dtolnay/rust-toolchain@stable"` returns 0:0 on both files.

## Evidence

- `bash scripts/schema-contract-smoke.sh` exits 0; final lines: "schema contract smoke passed"
- No `@stable` references remain in CI workflows
- `rust-toolchain.toml` parses correctly via `tomllib`

## What Remains for Phase 2

- **Wave 2 (02-02-PLAN):** CI step generating `dist/build-inputs.json` at runtime; blocker report new checks (toolchain.rust-toolchain-file, toolchain.build-inputs, sbom.present); `--md`/`--audience` dual-audience output.
- **Wave 3 (02-03-PLAN):** SBOM signing + redacted user export; standalone license compliance summary; acceptance readiness evidence visibility scaffold.
