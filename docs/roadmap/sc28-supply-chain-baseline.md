# SC-28 dependency, license, advisory and SBOM baseline (partial)

SC-28 adds a CI-only, read-only supply-chain scanner. `scripts/supply-chain-scan.sh`
resolves the committed `Cargo.lock` with `cargo metadata --locked`, records before/after
SHA-256 digests and checkout dirtiness, runs `cargo-audit` and `cargo-deny`, and always
materializes a bounded `supply-chain-report.json` plus `quarantine.json`. A lockfile drift,
unknown license, advisory finding, scanner failure or missing scanner quarantines the source
snapshot and returns a non-zero status. Thresholds are explicit (`SC28_MAX_HIGH_ADVISORIES`
and `SC28_MAX_CRITICAL_ADVISORIES`) and default to zero.

The metadata normalizer emits deterministic CycloneDX 1.5 and SPDX 2.3 manifests. The
manifests bind package references and dependency edges to the Cargo.lock digest and source
revision. `deny.toml` remains the policy source for allowed licenses, registry sources,
advisories and bans; the scanner does not silently widen that policy.

GitHub Actions installs the scanner binaries, runs the shell gate and structural validator,
and uploads the report, manifests, advisory output and quarantine record as CI-only evidence.
No release upload, signer, registry publish, production binary, external effect or live
provenance is performed. A passing metadata scan is source evidence only; SC-29 signing and
provenance verification, and SC-41 release gate integration, remain subsequent steps.

## Evidence boundary

- **feature_status:** `implemented` for the scanner scripts, deterministic manifest contract,
  workflow and source guard.
- **proof_level:** `source`; GitHub CI is wired but its result is intentionally unobserved.
- **not claimed:** signed artifacts, registry transparency, production dependency resolution,
  live advisory freshness, release approval, or physical/runtime integrity.
- **reviewer:** Codex source review; no local runtime test reviewer.
