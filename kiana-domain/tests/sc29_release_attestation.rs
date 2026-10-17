//! SC-29 release artifact signing and provenance fixture.
//!
//! Deny paths come first: filename/tag forgery, builder/source/toolchain drift, subject digest
//! mismatch and strict-shape tampering must all be refused before the single success case binds
//! manifest, provenance, signature and transparency digests to the same subject.

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

const BUILD_TYPE: &str = "https://slsa.dev/provenance/v1";
const SOURCE_REVISION: &str = "git:fixture-source";
const BUILDER_ID: &str = "builder://github-actions";

fn named_artifact(name: &str, digest: &str) -> ReleaseArtifactSubject {
    ReleaseArtifactSubject {
        name: name.to_owned(),
        kind: ReleaseArtifactKind::Archive,
        target: "x86_64-unknown-linux-gnu".to_owned(),
        digest: digest.to_owned(),
        size_bytes: 42,
        media_type: "application/gzip".to_owned(),
    }
}

fn artifact() -> ReleaseArtifactSubject {
    named_artifact("kiana-linux.tar.gz", A)
}

fn release(tag: &str, artifacts: Vec<ReleaseArtifactSubject>) -> Result<ReleaseManifest, String> {
    ReleaseManifest::new(
        "release-1",
        tag,
        SOURCE_REVISION,
        A,
        B,
        D,
        BUILDER_ID,
        artifacts,
        C,
    )
}

fn manifest() -> ReleaseManifest {
    release("v1.2.3", vec![artifact()]).expect("manifest fixture")
}

fn provenance_with(
    source_revision: &str,
    source_tree_digest: &str,
    cargo_lock_digest: &str,
    toolchain_digest: &str,
    subject_manifest_digest: &str,
) -> ReleaseProvenance {
    ReleaseProvenance::new(
        BUILD_TYPE,
        BUILDER_ID,
        B,
        source_revision,
        source_tree_digest,
        cargo_lock_digest,
        toolchain_digest,
        vec![ReleaseMaterial {
            uri: "git+https://example.invalid/kiana".to_owned(),
            digest: A.to_owned(),
        }],
        subject_manifest_digest,
    )
    .expect("provenance fixture")
}

fn provenance(manifest: &ReleaseManifest) -> ReleaseProvenance {
    provenance_with(SOURCE_REVISION, A, B, D, &manifest.manifest_digest)
}

fn signature(
    subject: &ReleaseManifest,
    verified: bool,
    transparency: &str,
) -> ReleaseSignatureAttestation {
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

fn verify(
    manifest: &ReleaseManifest,
    provenance: &ReleaseProvenance,
    signature: &ReleaseSignatureAttestation,
) -> ReleaseVerificationReport {
    ReleaseVerificationReport::verify(
        manifest,
        provenance,
        signature,
        "v1.2.3",
        SOURCE_REVISION,
        BUILDER_ID,
        D,
    )
    .expect("verification report")
}

#[test]
fn forged_artifact_filenames_cannot_claim_a_different_version_than_the_tag() {
    // A filename claiming another version than the signed tag is refused outright.
    assert_eq!(
        release("v1.2.3", vec![named_artifact("kiana-v9.9.9.tar.gz", A)]).unwrap_err(),
        "release_manifest_artifact_tag_mismatch"
    );
    // A longer version run must not be read as a prefix match.
    assert_eq!(
        release("v1.2.3", vec![named_artifact("kiana-v1.2.34.tar.gz", A)]).unwrap_err(),
        "release_manifest_artifact_tag_mismatch"
    );
    // A matching version token is accepted, including a pre-release tag whose token the
    // filename repeats.
    let exact = release("v1.2.3", vec![named_artifact("kiana-v1.2.3.tar.gz", A)])
        .expect("matching filename version");
    exact
        .validate()
        .expect("matching filename version stays valid");
    let pre_release = release(
        "v1.2.3-rc.1",
        vec![named_artifact("kiana-v1.2.3-rc.1.tar.gz", A)],
    )
    .expect("pre-release filename version");
    pre_release
        .validate()
        .expect("pre-release filename stays valid");
    // Path smuggling and empty tag shapes stay refused.
    assert_eq!(
        named_artifact("../kiana.tar.gz", A).validate().unwrap_err(),
        "release_artifact_name_invalid"
    );
    assert_eq!(
        release("not-a-tag", vec![artifact()]).unwrap_err(),
        "release_manifest_tag_invalid"
    );
}

#[test]
fn builder_source_and_toolchain_drift_are_blocked_before_subject_binding() {
    let manifest = manifest();
    let signature = signature(&manifest, true, E);

    assert_eq!(
        verify(
            &manifest,
            &provenance_with("git:other-source", A, B, D, &manifest.manifest_digest),
            &signature,
        )
        .reason,
        "release_verification_source_revision_mismatch"
    );
    let drifted_builder = ReleaseProvenance::new(
        BUILD_TYPE,
        "builder://untrusted",
        B,
        SOURCE_REVISION,
        A,
        B,
        D,
        vec![ReleaseMaterial {
            uri: "git+https://example.invalid/kiana".to_owned(),
            digest: A.to_owned(),
        }],
        manifest.manifest_digest.clone(),
    )
    .expect("well-formed but untrusted provenance");
    assert_eq!(
        verify(&manifest, &drifted_builder, &signature).reason,
        "release_verification_builder_mismatch"
    );
    assert_eq!(
        verify(
            &manifest,
            &provenance_with(SOURCE_REVISION, A, B, E, &manifest.manifest_digest),
            &signature,
        )
        .reason,
        "release_verification_toolchain_mismatch"
    );
    // Manifest↔provenance source/toolchain digests must agree even when both match the
    // expected release context.
    assert_eq!(
        verify(
            &manifest,
            &provenance_with(SOURCE_REVISION, E, B, D, &manifest.manifest_digest),
            &signature,
        )
        .reason,
        "release_verification_source_tree_digest_mismatch"
    );
    assert_eq!(
        verify(
            &manifest,
            &provenance_with(SOURCE_REVISION, A, C, D, &manifest.manifest_digest),
            &signature,
        )
        .reason,
        "release_verification_cargo_lock_digest_mismatch"
    );
    // Tag forgery is refused before provenance is even consulted.
    let report = ReleaseVerificationReport::verify(
        &manifest,
        &provenance(&manifest),
        &signature,
        "v9.9.9",
        SOURCE_REVISION,
        BUILDER_ID,
        D,
    )
    .expect("tag mismatch report");
    assert_eq!(report.reason, "release_verification_tag_mismatch");
    assert_eq!(report.status, ReleaseVerificationStatus::Blocked);
    // Only SLSA build types are accepted.
    assert!(ReleaseProvenance::new(
        "https://example.invalid/provenance/v0.2",
        BUILDER_ID,
        B,
        SOURCE_REVISION,
        A,
        B,
        D,
        vec![ReleaseMaterial {
            uri: "git+https://example.invalid/kiana".to_owned(),
            digest: A.to_owned(),
        }],
        manifest.manifest_digest.clone(),
    )
    .is_err());
    let mut foreign: ReleaseProvenance =
        provenance_with(SOURCE_REVISION, A, B, D, &manifest.manifest_digest);
    foreign.build_type = "https://example.invalid/provenance/v1".to_owned();
    assert_eq!(
        foreign.validate().unwrap_err(),
        "release_provenance_build_type_invalid"
    );
}

#[test]
fn subject_digest_mismatch_is_blocked_for_provenance_signature_and_artifact_bytes() {
    let manifest = manifest();
    let signature = signature(&manifest, true, E);

    // Provenance and signature must repeat the exact manifest digest.
    assert_eq!(
        verify(
            &manifest,
            &provenance_with(SOURCE_REVISION, A, B, D, E),
            &signature,
        )
        .reason,
        "release_verification_provenance_subject_mismatch"
    );
    let mut foreign = signature(&manifest, true, E);
    foreign.subject_manifest_digest = E.to_owned();
    foreign.attestation_digest = foreign.digest();
    foreign.validate().expect("re-signed foreign signature");
    assert_eq!(
        verify(&manifest, &provenance(&manifest), &foreign).reason,
        "release_verification_signature_subject_mismatch"
    );

    // The artifact subject binds the exact bytes; name, digest and size each carry a
    // distinct refusal code.
    let subject = artifact();
    subject.verify_bytes(A, 42).expect("exact bytes bind");
    assert_eq!(
        subject.verify_bytes(E, 42).unwrap_err(),
        "release_artifact_digest_mismatch"
    );
    assert_eq!(
        subject.verify_bytes(A, 41).unwrap_err(),
        "release_artifact_size_mismatch"
    );
    assert_eq!(
        subject.verify_bytes("sha256:not-hex", 42).unwrap_err(),
        "release_artifact_observed_digest_invalid"
    );
}

#[test]
fn unverified_or_missing_transparency_stays_unknown_and_unknown_fields_fail() {
    let manifest = manifest();
    let provenance = provenance(&manifest);
    let unverified = signature(&manifest, false, E);
    let report = verify(&manifest, &provenance, &unverified);
    assert_eq!(report.status, ReleaseVerificationStatus::Unknown);
    assert_eq!(report.reason, "release_verification_signature_unverified");

    let missing_transparency = signature(&manifest, true, "");
    let report = verify(&manifest, &provenance, &missing_transparency);
    assert_eq!(report.status, ReleaseVerificationStatus::Unknown);
    assert_eq!(report.reason, "release_verification_transparency_missing");

    let mut value = serde_json::to_value(&manifest).expect("manifest JSON");
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

#[test]
fn release_contracts_stay_bounded_strict_and_digest_bound() {
    // Duplicate subjects, empty and oversized subject lists are refused.
    assert_eq!(
        release("v1.2.3", vec![artifact(), artifact()]).unwrap_err(),
        "release_manifest_artifact_duplicate"
    );
    assert_eq!(
        release("v1.2.3", vec![]).unwrap_err(),
        "release_manifest_artifact_count_invalid"
    );
    let oversized: Vec<ReleaseArtifactSubject> = (0..129)
        .map(|index| named_artifact(&format!("kiana-{index}.tar.gz"), A))
        .collect();
    assert_eq!(
        release("v1.2.3", oversized).unwrap_err(),
        "release_manifest_artifact_count_invalid"
    );

    // Header/schema drift and a rewritten body are refused by the self-digest.
    let mut manifest = manifest();
    manifest.schema = "kiana.release-manifest.v2".to_owned();
    assert_eq!(
        manifest.validate().unwrap_err(),
        "release_manifest_header_invalid"
    );
    let mut digest_drift = manifest();
    digest_drift.source_tree_digest = "sha256:not-hex".to_owned();
    assert_eq!(
        digest_drift.validate().unwrap_err(),
        "release_manifest_source_tree_digest_invalid"
    );

    // Provenance materials are unique, bounded and digest bound.
    assert_eq!(
        ReleaseProvenance::new(
            BUILD_TYPE,
            BUILDER_ID,
            B,
            SOURCE_REVISION,
            A,
            B,
            D,
            vec![
                ReleaseMaterial {
                    uri: "git+https://example.invalid/kiana".to_owned(),
                    digest: A.to_owned(),
                },
                ReleaseMaterial {
                    uri: "git+https://example.invalid/kiana".to_owned(),
                    digest: B.to_owned(),
                },
            ],
            A,
        )
        .unwrap_err(),
        "release_provenance_material_duplicate"
    );
    let mut provenance = provenance(&manifest());
    provenance.invocation_digest = E.to_owned();
    assert_eq!(
        provenance.validate().unwrap_err(),
        "release_provenance_digest_mismatch"
    );
}

#[test]
fn exact_manifest_provenance_signature_and_transparency_binding_verifies() {
    let manifest = manifest();
    let provenance = provenance(&manifest);
    let signature = signature(&manifest, true, E);
    let report = verify(&manifest, &provenance, &signature);
    assert_eq!(report.status, ReleaseVerificationStatus::Verified);
    assert_eq!(report.reason, "ok");
    assert_eq!(report.manifest_digest, manifest.manifest_digest);
    assert_eq!(report.provenance_digest, provenance.provenance_digest);
    assert_eq!(
        report.signature_attestation_digest,
        signature.attestation_digest
    );
    report.validate().expect("report digest");
    // The success report round-trips and never carries a filename, URL or tag as authority.
    let encoded = serde_json::to_string(&report).expect("report JSON");
    assert!(!encoded.contains("kiana-linux.tar.gz"));
    assert_eq!(
        serde_json::from_str::<ReleaseVerificationReport>(&encoded).expect("report decode"),
        report
    );
}
