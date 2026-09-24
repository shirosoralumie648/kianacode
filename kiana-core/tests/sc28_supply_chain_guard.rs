//! SC-28 source guard for dependency, license, advisory and SBOM gates.

#[test]
fn sc28_supply_chain_scanner_is_locked_and_quarantines_failures() {
    let workflow = include_str!("../../.github/workflows/sc28-supply-chain.yml");
    let scanner = include_str!("../../scripts/supply-chain-scan.sh");
    let normalizer = include_str!("../../scripts/supply-chain-scan.py");
    let validator = include_str!("../../scripts/validate-sc28-supply-chain.sh");
    let deny = include_str!("../../deny.toml");
    let baseline = include_str!("../../docs/roadmap/sc28-supply-chain-baseline.md");
    let current_status = include_str!("../../CURRENT_STATUS.md");
    let roadmap = include_str!("../../docs/roadmap.md");
    for marker in [
        "permissions:",
        "contents: read",
        "cargo fetch --locked",
        "cargo-audit",
        "cargo-deny",
        "cargo fmt --all --check",
        "supply-chain-scan.sh",
        "upload-artifact@v4",
    ] {
        assert!(workflow.contains(marker), "SC-28 workflow marker missing: {marker}");
    }
    assert!(!workflow.contains("continue-on-error: true"));
    for marker in [
        "Cargo.lock",
        "cargo metadata",
        "cargo audit",
        "cargo deny",
        "SC28_MAX_HIGH_ADVISORIES",
        "SC28_MAX_CRITICAL_ADVISORIES",
        "lock_dirty",
    ] {
        assert!(scanner.contains(marker), "SC-28 scanner marker missing: {marker}");
    }
    for marker in [
        "CycloneDX",
        "SPDX-2.3",
        "lockfile_drift",
        "license_unknown",
        "advisory_found",
        "quarantined",
        "automatic_release_allowed",
    ] {
        assert!(normalizer.contains(marker), "SC-28 normalizer marker missing: {marker}");
    }
    for marker in ["[advisories]", "[licenses]", "[bans]", "[sources]", "unknown-registry", "unknown-git"] {
        assert!(deny.contains(marker), "deny.toml marker missing: {marker}");
    }
    for marker in ["SBOM", "SPDX", "CycloneDX", "Cargo.lock", "advisory", "license", "quarantine", "limitations", "reviewer"] {
        assert!(baseline.contains(marker), "SC-28 baseline marker missing: {marker}");
    }
    for marker in [
        "set -euo pipefail",
        "SC-28 scanner",
        "supply-chain-scan.py",
        "lockfile_drift",
        "automatic_release_allowed",
    ] {
        assert!(validator.contains(marker), "SC-28 validator marker missing: {marker}");
    }
    assert!(current_status.contains("### SC-28"));
    assert!(roadmap.contains("<a id=\"step-sc-28\"></a>SC-28"));
    assert!(roadmap.contains("| 501 | W6 | 专项 | [`SC-28`]"));
    assert!(!scanner.contains("git push"));
}
