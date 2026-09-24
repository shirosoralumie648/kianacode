use kiana_domain::{
    ReleaseArtifactKind, ReleaseArtifactSubject, ReleaseManifest, ReleaseMaterial,
    ReleaseProvenance, ReleaseSignatureAlgorithm, ReleaseSignatureAttestation,
    ReleaseVerificationReport, ReleaseVerificationStatus,
};

const A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const C: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const D: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const E: &str = "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

fn artifact() -> ReleaseArtifactSubject {
    ReleaseArtifactSubject {
        name: "kiana-linux.tar.gz".to_owned(),
        kind: ReleaseArtifactKind::Archive,
        target: "x86_64-unknown-linux-gnu".to_owned(),
        digest: A.to_owned(),
        size_bytes: 42,
        media_type: "application/gzip".to_owned(),
    }
}

fn manifest() -> ReleaseManifest {
    ReleaseManifest::new(
        "release-1",
        "v1.2.3",
        "git:fixture-source",
        A,
        B,
        D,
        "builder://github-actions",
        vec![artifact()],
        C,
    )
    .expect("manifest fixture")
}

fn provenance(subject: &ReleaseManifest) -> ReleaseProvenance {
    ReleaseProvenance::new(
        "https://slsa.dev/provenance/v1",
        "builder://github-actions",
        B,
        "git:fixture-source",
        A,
        B,
        D,
        vec![ReleaseMaterial {
            uri: "git+https://example.invalid/kiana".to_owned(),
            digest: A.to_owned(),
        }],
        subject.manifest_digest.clone(),
    )
    .expect("provenance fixture")
}

fn signature(subject: &ReleaseManifest, verified: bool, transparency: &str) -> ReleaseSignatureAttestation {
    ReleaseSignatureAttestation::new(
        ReleaseSignatureAlgorithm::Sigstore,
        "release-signer",
        "key-1",
        subject.manifest_digest.clone(),
        E,
        transparency,
        "verifier://ci",
        verified,
    )
    .expect("signature fixture")
}

#[test]
fn exact_manifest_provenance_signature_and_transparency_binding_verifies() {
    let manifest = manifest();
    let provenance = provenance(&manifest);
    let signature = signature(&manifest, true, E);
    let report = ReleaseVerificationReport::verify(
        &manifest,
        &provenance,
        &signature,
        "v1.2.3",
        "git:fixture-source",
        "builder://github-actions",
        D,
    )
    .expect("verification report");
    assert_eq!(report.status, ReleaseVerificationStatus::Verified);
    assert_eq!(report.reason, "ok");
    report.validate().expect("report digest");
}

#[test]
fn tag_builder_toolchain_and_subject_drift_are_blocked() {
    let manifest = manifest();
    let provenance = provenance(&manifest);
    let signature = signature(&manifest, true, E);
    assert_eq!(
        ReleaseVerificationReport::verify(
            &manifest,
            &provenance,
            &signature,
            "v9.9.9",
            "git:fixture-source",
            "builder://github-actions",
            D,
        )
        .unwrap()
        .reason,
        "release_verification_tag_mismatch"
    );
    assert_eq!(
        ReleaseVerificationReport::verify(
            &manifest,
            &provenance,
            &signature,
            "v1.2.3",
            "git:fixture-source",
            "builder://untrusted",
            D,
        )
        .unwrap()
        .reason,
        "release_verification_builder_mismatch"
    );
    assert_eq!(
        ReleaseVerificationReport::verify(
            &manifest,
            &provenance,
            &signature,
            "v1.2.3",
            "git:fixture-source",
            "builder://github-actions",
            E,
        )
        .unwrap()
        .reason,
        "release_verification_toolchain_mismatch"
    );

    let forged = ReleaseProvenance::new(
        "https://slsa.dev/provenance/v1",
        "builder://github-actions",
        B,
        "git:fixture-source",
        A,
        B,
        D,
        vec![ReleaseMaterial {
            uri: "git+https://example.invalid/kiana".to_owned(),
            digest: A.to_owned(),
        }],
        C,
    )
    .expect("forged but well-formed provenance");
    assert_eq!(
        ReleaseVerificationReport::verify(
            &manifest,
            &forged,
            &signature,
            "v1.2.3",
            "git:fixture-source",
            "builder://github-actions",
            D,
        )
        .unwrap()
        .reason,
        "release_verification_provenance_subject_mismatch"
    );
}

#[test]
fn unverified_or_missing_transparency_stays_unknown_and_unknown_fields_fail() {
    let manifest = manifest();
    let provenance = provenance(&manifest);
    let unverified = signature(&manifest, false, E);
    let report = ReleaseVerificationReport::verify(
        &manifest,
        &provenance,
        &unverified,
        "v1.2.3",
        "git:fixture-source",
        "builder://github-actions",
        D,
    )
    .expect("unknown report");
    assert_eq!(report.status, ReleaseVerificationStatus::Unknown);
    assert_eq!(report.reason, "release_verification_signature_unverified");

    let missing_transparency = signature(&manifest, true, "");
    let report = ReleaseVerificationReport::verify(
        &manifest,
        &provenance,
        &missing_transparency,
        "v1.2.3",
        "git:fixture-source",
        "builder://github-actions",
        D,
    )
    .expect("missing transparency report");
    assert_eq!(report.status, ReleaseVerificationStatus::Unknown);
    assert_eq!(report.reason, "release_verification_transparency_missing");

    let mut value = serde_json::to_value(manifest).expect("manifest JSON");
    value["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ReleaseManifest>(value).is_err());
}

#[test]
fn artifact_names_cannot_smuggle_paths_and_digest_drift_is_rejected() {
    let mut bad = artifact();
    bad.name = "../kiana.tar.gz".to_owned();
    assert_eq!(bad.validate().unwrap_err(), "release_artifact_name_invalid");

    let mut manifest = manifest();
    manifest.artifacts[0].digest = E.to_owned();
    assert_eq!(
        manifest.validate().unwrap_err(),
        "release_manifest_digest_mismatch"
    );
}
