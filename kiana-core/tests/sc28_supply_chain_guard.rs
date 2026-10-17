//! SC-28 source guard for dependency, license, advisory and SBOM gates.

#[test]
fn sc28_supply_chain_scanner_is_locked_and_quarantines_failures() {
    let workflow = include_str!("../../.github/workflows/sc28-supply-chain.yml");
    let scanner = include_str!("../../scripts/supply-chain-scan.sh");
    let normalizer = include_str!("../../scripts/supply-chain-scan.py");
    let validator = include_str!("../../scripts/validate-sc28-supply-chain.sh");
    let fixtures = include_str!("../../scripts/tests/test_sc28_supply_chain.py");
    let deny = include_str!("../../deny.toml");
    let baseline = include_str!("../../docs/roadmap/sc28-supply-chain-baseline.md");
    let current_status = include_str!("../../CURRENT_STATUS.md");
    let roadmap = include_str!("../../docs/roadmap.md");
    let card = include_str!("../../docs/roadmap/security-compliance.md");
    for marker in [
        "permissions:",
        "contents: read",
        "cargo fetch --locked",
        "cargo-audit",
        "cargo-deny",
        "cargo fmt --all --check",
        "supply-chain-scan.sh",
        "upload-artifact@v4",
        "scripts/tests/test_sc28_supply_chain.py",
    ] {
        assert!(
            workflow.contains(marker),
            "SC-28 workflow marker missing: {marker}"
        );
    }
    assert!(!workflow.contains("continue-on-error: true"));
    // The shipped lane pins zero tolerance for advisories. Raising either bound is
    // a policy change that has to be visible in this file, not a silent fixture.
    for marker in [
        "SC28_MAX_HIGH_ADVISORIES: \"0\"",
        "SC28_MAX_CRITICAL_ADVISORIES: \"0\"",
    ] {
        assert!(
            workflow.contains(marker),
            "SC-28 workflow must pin the zero-tolerance threshold: {marker}"
        );
    }
    for marker in [
        "Cargo.lock",
        "cargo metadata",
        "cargo audit",
        "cargo deny",
        "SC28_MAX_HIGH_ADVISORIES",
        "SC28_MAX_CRITICAL_ADVISORIES",
        "lock_dirty",
        "--metadata-exit",
        "--lock-present",
        "--audit-version",
    ] {
        assert!(
            scanner.contains(marker),
            "SC-28 scanner marker missing: {marker}"
        );
    }
    for marker in [
        "CycloneDX",
        "SPDX-2.3",
        "REASON_CODES",
        "lockfile_missing",
        "lockfile_resolution_failed",
        "dependency_graph_empty",
        "lockfile_drift",
        "license_unknown",
        "advisory_found",
        "advisory_severity_unclassified",
        "quarantined",
        "automatic_release_allowed",
    ] {
        assert!(
            normalizer.contains(marker),
            "SC-28 normalizer marker missing: {marker}"
        );
    }
    for marker in [
        "[advisories]",
        "[licenses]",
        "[bans]",
        "[sources]",
        "unknown-registry",
        "unknown-git",
    ] {
        assert!(deny.contains(marker), "deny.toml marker missing: {marker}");
    }
    for marker in [
        "SBOM",
        "SPDX",
        "CycloneDX",
        "Cargo.lock",
        "advisory",
        "license",
        "quarantine",
        "completion slice",
        "deny-first",
        "test_sc28_supply_chain.py",
        "lockfile_resolution_failed",
        "limitations",
        "reviewer",
    ] {
        assert!(
            baseline.contains(marker),
            "SC-28 baseline marker missing: {marker}"
        );
    }
    for marker in [
        "set -euo pipefail",
        "SC-28 scanner",
        "supply-chain-scan.py",
        "lockfile_drift",
        "automatic_release_allowed",
        "test_sc28_supply_chain.py",
    ] {
        assert!(
            validator.contains(marker),
            "SC-28 validator marker missing: {marker}"
        );
    }
    for marker in [
        "lockfile_missing",
        "lockfile_resolution_failed",
        "dependency_graph_empty",
        "advisory_severity_unclassified",
        "advisory_found",
        "auto",
    ] {
        assert!(
            fixtures.contains(marker),
            "SC-28 fixture marker missing: {marker}"
        );
    }
    // Deny-first ordering is a contract, not a preference: the success fixture is
    // the last test in the file, so a passing gate cannot hide a missing denial.
    let last_test = fixtures
        .rsplit("def test_")
        .next()
        .expect("SC-28 fixtures define at least one test");
    assert!(
        last_test.starts_with("clean_locked_snapshot_passes"),
        "SC-28 success fixture must be the final deny-first fixture"
    );
    // The fixtures are offline pure-Python: they must never grow a process or
    // network edge, and they must never run the real Cargo or scanner commands.
    for forbidden in [
        "import subprocess",
        "import socket",
        "import urllib",
        "import requests",
        "import shutil",
        "subprocess.run",
        "urllib.request",
        "socket.socket",
        "cargo audit",
        "cargo deny",
    ] {
        assert!(
            !fixtures.contains(forbidden),
            "SC-28 fixtures must stay offline: {forbidden}"
        );
    }
    assert!(current_status.contains("### SC-28"));
    // 【为什么分开读两份文档】
    // 专项卡的锚点写在 `docs/roadmap/security-compliance.md`，不在总队列
    // `docs/roadmap.md`；总队列里的是指向卡片的链接行。
    // 两者合并归一个文件断言，只会把“卡片在”误读成
    // “队列行在”，而且把行号变了就报失败。
    assert!(card.contains("<a id=\"step-sc-28\"></a>SC-28"));
    assert!(roadmap.contains("[`SC-28`](roadmap/security-compliance.md#step-sc-28)"));
    assert!(!scanner.contains("git push"));
}
