//! DEP-39 source guard for supply-chain, package and compliance evidence.

#[test]
fn supply_chain_gate_is_fail_closed_and_script_bound() {
    let source = include_str!("../../kiana-domain/src/supply_chain.rs");
    let release = include_str!("../../kiana-domain/src/supply_chain_release_evidence.rs");
    let workflow = include_str!("../../.github/workflows/dep39-supply-chain.yml");
    let baseline = include_str!("../../docs/roadmap/dep39-supply-chain-baseline.md");
    for marker in [
        "SupplyChainArtifactKind",
        "DesktopPackage",
        "checksum_verified",
        "signature_verified",
        "sbom_bound",
        "license_unknown_count",
        "secret_scan_passed",
        "dependency_scan_passed",
        "signing_policy_digest",
        "compliance_policy_digest",
        "reviewer",
        "publish_allowed",
        "supply_chain_license_unknown",
        "supply_chain_secret_scan_failed",
        "supply_chain_desktop_package_missing",
        "supply_chain_artifact_evidence_incomplete",
        "SupplyChainReleaseEvidence",
        "SupplyChainReleaseDisposition",
        "supply_chain_blocked_release",
        "supply_chain_publish_receipt_missing",
        "supply_chain_unknown_publish_cannot_verify",
    ] {
        assert!(
            source.contains(marker) || release.contains(marker),
            "DEP-39 source marker missing: {marker}"
        );
    }
    for marker in [
        "scripts/compliance-audit.sh",
        "scripts/generate-license-summary.sh",
        "scripts/release-signature-verification-smoke.sh",
        "scripts/sign-release-artifacts.sh",
        "scripts/verify-commercial-release-artifacts.sh",
        "scripts/package-release.sh",
        "scripts/package-desktop-deb.sh",
        "cargo test -p kiana-domain --test dep39_supply_chain",
        "cargo test -p kiana-core --test dep39_supply_chain_guard",
    ] {
        assert!(
            workflow.contains(marker),
            "DEP-39 workflow marker missing: {marker}"
        );
    }
    for marker in [
        "SBOM",
        "signature",
        "checksum",
        "license",
        "secret scanning",
        "Desktop",
        "compliance",
        "partial",
        "fixture",
        "not a live release",
    ] {
        assert!(
            baseline.contains(marker),
            "DEP-39 baseline marker missing: {marker}"
        );
    }
    assert!(!source.contains("CapabilityBroker::new"));
}
