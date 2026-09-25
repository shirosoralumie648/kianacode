# EXT-31 · Extension performance, supply-chain and release baseline

This source slice defines fixed benchmark and release-gate metadata. It does not run a benchmark,
perform a supply-chain scan, package the Desktop shell or publish a release.

## Source contract

- `ExtensionBenchmarkObservation` fixes six metrics (cold/warm catalog, resource read, Hook
  latency, package verification and snapshot rebuild) to an environment digest, toolchain digest,
  package size, memory/disk counters and concurrency. Observed rows require samples and ordered
  p50/p95; skipped/blocked rows require an explicit limitation and cannot carry metrics.
- `ExtensionReleaseGate` binds source, lockfile, manifest, supply-chain and deny-matrix digests,
  requires every metric exactly once, and keeps `Ready` impossible while any benchmark is skipped
  or blocked. `blockers()` exposes the remaining evidence without running anything.
- Existing `scripts/release-smoke.sh`, `scripts/harness-golden-smoke.sh`,
  `scripts/supply-chain-scan.sh` and `docs/module-map.md` remain the operational/source
  boundaries; this contract only records their evidence references.

## CI-only fixtures

`.github/workflows/ext31-extension-release.yml` runs formatting, the domain release-gate fixture,
the Core source guard and workspace test-target compilation. Its path filter includes current
CM-36 `kiana-domain/src/memory_workbench.rs` so a fresh remote run covers the repository-wide
format dependency. Local tests, builds, checks, clippy, benchmark and release/supply-chain smoke
commands are deliberately not run, and GitHub CI is not awaited.

## Evidence ceiling and limitations

The slice is `feature_status=partial` with `proof_level=source` plus CI wiring. No p50/p95,
memory/disk or package verification measurement has been observed locally; no supply-chain scan,
release smoke, artifact signature, Desktop package, publication, live provider or physical proof is
claimed. The gate remains partial until CI and fixed-environment measurements produce evidence.
