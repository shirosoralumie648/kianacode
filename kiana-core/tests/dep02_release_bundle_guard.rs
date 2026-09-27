#[test]
fn dep02_release_bundle_stays_evidence_only() {
    let domain = include_str!("../../kiana-domain/src/deployment_release.rs");
    let attestation = include_str!("../../kiana-domain/src/release_attestation.rs");
    let core = include_str!("../src/deployment_release.rs");
    for marker in [
        "DeploymentReleaseBundle",
        "ReleaseManifest",
        "ReleaseProvenance",
        "ReleaseSignatureAttestation",
        "ReleaseVerificationReport",
        "bundle_digest",
        "secret_free",
        "deployment_release_subject_mismatch",
        "validate_deployment_release",
    ] {
        assert!(
            domain.contains(marker) || attestation.contains(marker) || core.contains(marker),
            "DEP-02 marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::fs",
        "std::process::Command",
        "tokio::",
        "CapabilityBroker",
        "EventStore::append",
        "publish_release",
        "install_release",
        "transparency_log::submit",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "DEP-02 release path crossed an effect boundary: {forbidden}"
        );
    }
}
