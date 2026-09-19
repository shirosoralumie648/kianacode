//! SC-41 source guard for security workflows, release scripts and fail-closed gates.

#[test]
fn sc41_security_gate_binds_workflows_scripts_and_proof_limits() {
    let workflow = include_str!("../../.github/workflows/sc41-security-gate.yml");
    let validator = include_str!("../../scripts/validate-sc41-security-gate.sh");
    let baseline = include_str!("../../docs/roadmap/sc41-security-gate-baseline.md");
    let current_status = include_str!("../../CURRENT_STATUS.md");
    let roadmap = include_str!("../../docs/roadmap.md");
    for marker in [
        "permissions:",
        "contents: read",
        "cargo fmt --all --check",
        "cargo check --workspace --tests --locked",
        "security_baseline",
        "validate-sc41-security-gate.sh",
        "git diff --check",
    ] {
        assert!(
            workflow.contains(marker),
            "SC-41 workflow marker missing: {marker}"
        );
    }
    assert!(!workflow.contains("continue-on-error: true"));
    for marker in [
        "sc00-baseline.yml",
        "sc18-secret-ref.yml",
        "sc19-secret-rotation.yml",
        "dep39-supply-chain.yml",
        "dep41-release-gate.yml",
        "compliance-audit.sh",
        "package-release.sh",
        "sign-release-artifacts.sh",
        "verify-commercial-release-artifacts.sh",
        "release-preflight.sh",
        "release-signature-verification-smoke.sh",
    ] {
        assert!(
            validator.contains(marker),
            "SC-41 validator marker missing: {marker}"
        );
    }
    for marker in [
        "secret",
        "SBOM",
        "signature",
        "checksum",
        "license",
        "fail-closed",
        "partial",
        "source",
        "durable",
        "live",
        "physical",
        "limitations",
        "reviewer",
    ] {
        assert!(
            baseline.contains(marker),
            "SC-41 baseline marker missing: {marker}"
        );
    }
    assert!(current_status.contains("### SC-41"));
    assert!(roadmap.contains("<a id=\"step-sc-41\"></a>SC-41"));
    assert!(!workflow.contains("CapabilityBroker::new"));
}
