# SC-28 dependency, license, advisory and SBOM baseline

SC-28 adds a CI-only, read-only supply-chain scanner. `scripts/supply-chain-scan.sh`
resolves the committed `Cargo.lock` with `cargo metadata --locked`, records before/after
SHA-256 digests and checkout dirtiness, runs `cargo-audit` and `cargo-deny`, and always
materializes a bounded `supply-chain-report.json` plus `quarantine.json`. A missing
lockfile, an unresolved dependency graph, a lockfile drift, an unknown license, an
advisory finding, a scanner failure or an unclassified severity quarantines the source
snapshot and returns a non-zero status. Thresholds are explicit
(`SC28_MAX_HIGH_ADVISORIES` and `SC28_MAX_CRITICAL_ADVISORIES`) and default to zero.

The metadata normalizer emits deterministic CycloneDX 1.5 and SPDX 2.3 manifests. The
manifests bind package references and dependency edges to the Cargo.lock digest and source
revision. `deny.toml` remains the policy source for allowed licenses, registry sources,
advisories and bans; the scanner does not silently widen that policy.

GitHub Actions installs the scanner binaries, runs the deny-first normalizer fixtures, the
shell gate and the structural validator, and uploads the report, manifests, advisory output
and quarantine record as CI-only evidence. No release upload, signer, registry publish,
production binary, external effect or live provenance is performed. A passing metadata scan
is source evidence only; SC-29 signing and provenance verification, and SC-41 release gate
integration, remain subsequent steps.

## 2026-10-17 completion slice (CI-only)

The card's four rejection paths now produce machine readable quarantine evidence instead of
depending on prose or on a bare non-zero exit:

| rejection path | reason code(s) | fixture |
|---|---|---|
| `未锁定依赖` (missing lockfile) | `lockfile_missing`, `dependency_graph_empty` | `test_missing_lockfile_quarantines_before_any_resolution` |
| `未锁定依赖` (unresolvable locked graph) | `lockfile_resolution_failed`, `dependency_graph_empty` | `test_an_unlocked_dependency_graph_quarantines_without_a_silent_pass` |
| lockfile drift / dirty checkout | `lockfile_drift`, `lockfile_digest_mismatch` | `test_lockfile_drift_between_resolution_and_scan_quarantines`, `test_expected_lockfile_digest_mismatch_quarantines` |
| `许可证未知` | `license_unknown` | `test_unknown_license_metadata_quarantines_and_stays_noassertion` |
| `已知高危漏洞仍发布` | `advisory_found`, `high_threshold_exceeded`, `critical_threshold_exceeded`, `advisory_severity_unclassified` | `test_default_budget_tolerates_no_advisory_of_any_severity`, `test_a_reviewed_budget_is_the_only_path_that_tolerates_classified_findings`, `test_unclassified_severity_is_never_tolerated_even_with_a_raised_budget` |
| scanner missing or failing | `advisory_scanner_missing`, `policy_scanner_missing`, `advisory_scan_failed`, `dependency_policy_failed` | `test_missing_scanner_binaries_never_count_as_a_pass`, `test_scanner_exit_codes_are_recorded_as_stable_reasons` |

- `scripts/supply-chain-scan.py` publishes the closed reason code set (`REASON_CODES`) in
  both records, so a report cannot carry a free-form failure string, and the shell wrapper
  records the `cargo metadata --locked` exit code, the resolved scanner versions and the
  lockfile presence instead of aborting before evidence is written.
- Advisory thresholds are now a reviewed tolerance budget: `advisory_found` fires whenever
  the finding count exceeds `max_high + max_critical`, and the severity bounds are enforced
  independently. The shipped lane pins both to zero, so the default policy still
  quarantines on a single finding and the guard test fails if that pin is removed.
- `scripts/tests/test_sc28_supply_chain.py` is the deny-first fixture harness: it drives the
  normalizer offline over synthetic Cargo metadata and cargo-audit output, proves every
  denial above before the single success fixture, and asserts manifest determinism, the
  bounded report, the lockfile digest binding and the absence of any process or network
  edge. `scripts/validate-sc28-supply-chain.sh` runs it, so the unified `ci` gate and
  `.github/workflows/sc28-supply-chain.yml` both execute it.
- `kiana-core/tests/sc28_supply_chain_guard.rs` pins the workflow path filters, the
  zero-tolerance threshold env values, the reason code set, the fixture names, the
  deny-first ordering and the negative assertions (no `continue-on-error`, no `git push`,
  no process or network edge in the fixtures).

## Evidence boundary

- **feature_status:** `implemented` for the scanner scripts, deterministic manifest
  contract, deny-first fixtures, workflow and source guard.
- **proof_level:** `source`; GitHub CI is wired but its result is intentionally unobserved.
- **not claimed:** signed artifacts, registry transparency, production dependency resolution,
  live advisory freshness, release approval, or physical/runtime integrity.
- **known open risk:** the advisory gate is fail-closed and the current dependency tree still
  produces real `cargo-audit`/`cargo-deny` findings, so this lane is expected to quarantine
  until those dependencies are upgraded or the policy is reviewed. That remediation is a
  dependency change, not a test or scanner change; the 2026-09-30 evidence block in
  CURRENT_STATUS.md records the observed quarantine.
- **limitations:** the scanner and its reports are CI-only source evidence; this baseline does
  not prove that a released artifact was built from the scanned snapshot. Scanner versions
  are recorded in the report but not pinned, so the tool builds may drift between runs.
- **reviewer:** source review of the reason code set, budget semantics, fixture expectations
  and workflow wiring; no local runtime test reviewer and no unobserved CI receipt.
