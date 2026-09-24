//! SC-29 source guard for release artifact, provenance and signature binding.

#[test]
fn release_attestation_keeps_subject_builder_and_transparency_bindings() {
    let domain = include_str!("../../kiana-domain/src/release_attestation.rs");
    let verifier = include_str!("../../scripts/verify-release-provenance.py");
    let workflow = include_str!("../../.github/workflows/sc29-release-provenance.yml");
    let baseline = include_str!("../../docs/roadmap/sc29-release-provenance-baseline.md");
    for marker in [
        "ReleaseManifest",
        "ReleaseProvenance",
        "ReleaseSignatureAttestation",
        "subject_manifest_digest",
        "transparency_log_entry_digest",
        "release_verification_builder_mismatch",
        "release_verification_toolchain_mismatch",
        "release_verification_signature_unverified",
        "release_verification_tag_mismatch",
        "ReleaseVerificationStatus::Unknown",
    ] {
        assert!(domain.contains(marker), "SC-29 domain marker missing: {marker}");
    }
    for marker in [
        "manifest_digest",
        "source_revision",
        "builder_id",
        "toolchain_digest",
        "transparency_log_entry_digest",
        "release_verification_artifact_digest_mismatch",
        "unknown_field",
        "self_test_forged_tag_accepted",
    ] {
        assert!(verifier.contains(marker), "SC-29 verifier marker missing: {marker}");
    }
    for marker in [
        "contents: read",
        "verify-release-provenance.py --self-test",
        "cargo test -p kiana-domain --test sc29_release_attestation",
        "cargo test -p kiana-core --test sc29_release_attestation_guard",
        "cargo fmt --all --check",
    ] {
        assert!(workflow.contains(marker), "SC-29 workflow marker missing: {marker}");
    }
    for marker in [
        "ReleaseManifest",
        "SLSA",
        "transparency",
        "builder",
        "toolchain",
        "subject digest",
        "source",
        "limitations",
        "reviewer",
    ] {
        assert!(baseline.contains(marker), "SC-29 baseline marker missing: {marker}");
    }
    assert!(!verifier.contains("requests."));
    assert!(!verifier.contains("urllib."));
}
