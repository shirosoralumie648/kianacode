use kiana_domain::{
    DeploymentProfile, DeploymentReleaseBundle, ReleaseArtifactKind, ReleaseArtifactSubject,
    ReleaseManifest, ReleaseMaterial, ReleaseProvenance, ReleaseSignatureAlgorithm,
    ReleaseSignatureAttestation, ReleaseVerificationReport,
};

const D: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn bundle(builder_id: &str, verified: bool) -> Result<DeploymentReleaseBundle, String> {
    let manifest = ReleaseManifest::new(
        "release-1",
        "v1.2.3",
        "git-commit-1",
        D,
        D,
        D,
        builder_id,
        vec![ReleaseArtifactSubject {
            name: "kiana-linux.tar.gz".to_owned(),
            kind: ReleaseArtifactKind::Archive,
            target: "x86_64-unknown-linux-gnu".to_owned(),
            digest: D.to_owned(),
            size_bytes: 42,
            media_type: "application/gzip".to_owned(),
        }],
        D,
    )?;
    let provenance = ReleaseProvenance::new(
        "https://slsa.dev/provenance/v1",
        builder_id,
        D,
        "git-commit-1",
        D,
        D,
        D,
        vec![ReleaseMaterial {
            uri: "git+https://example.invalid/kiana".to_owned(),
            digest: D.to_owned(),
        }],
        manifest.manifest_digest.clone(),
    )?;
    let signature = ReleaseSignatureAttestation::new(
        ReleaseSignatureAlgorithm::External,
        "release-signer",
        "release-key",
        manifest.manifest_digest.clone(),
        D,
        D,
        "ci-verifier",
        verified,
    )?;
    let verification = ReleaseVerificationReport::verify(
        &manifest,
        &provenance,
        &signature,
        "v1.2.3",
        "git-commit-1",
        builder_id,
        D,
    )?;
    DeploymentReleaseBundle::new(
        DeploymentProfile::Container,
        manifest,
        provenance,
        signature,
        verification,
    )
}

#[test]
fn verified_release_bundle_is_digest_bound_and_round_trips() {
    let first = bundle("builder-1", true).unwrap();
    let second = bundle("builder-1", true).unwrap();
    assert_eq!(first.bundle_digest, second.bundle_digest);
    assert_eq!(
        serde_json::from_str::<DeploymentReleaseBundle>(&serde_json::to_string(&first).unwrap())
            .unwrap(),
        first
    );
}

#[test]
fn unverified_subject_and_secret_release_evidence_fail_closed() {
    assert_eq!(
        bundle("builder-1", false).unwrap_err(),
        "deployment_release_not_verified"
    );
    assert_eq!(
        bundle("api_key=redacted", true).unwrap_err(),
        "deployment_release_secret_marker"
    );

    let mut mismatched = bundle("builder-1", true).unwrap();
    mismatched.verification.manifest_digest = D.to_owned();
    mismatched.verification.report_digest = mismatched.verification.digest();
    mismatched.bundle_digest = mismatched.digest();
    assert_eq!(
        mismatched.validate().unwrap_err(),
        "deployment_release_subject_mismatch"
    );
}
