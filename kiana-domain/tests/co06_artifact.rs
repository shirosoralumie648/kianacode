use kiana_domain::{
    json_digest, ArtifactProvenance, ArtifactVersion, Criterion, CriterionId, EventId, EvidenceId,
    EvidenceRef, InvocationId, ProjectId, RunId,
};
use uuid::Uuid;

fn scope() -> String {
    json_digest(&serde_json::json!({
        "organization": "org-1",
        "project": ProjectId::new(),
        "principal": "local-user",
    }))
}

fn provenance() -> ArtifactProvenance {
    ArtifactProvenance {
        producer_kind: "run".to_owned(),
        producer_id: "run-1".to_owned(),
        source_event_id: Some(EventId::new()),
        source_run_id: Some(RunId::new()),
        recorded_by: "local-user".to_owned(),
    }
}

#[test]
fn artifact_version_reference_and_evidence_bind_immutable_content() {
    let bytes = b"historical evidence";
    let scope = scope();
    let version = ArtifactVersion::new(
        kiana_domain::ArtifactId::new(),
        1,
        "kiana.test-evidence.v1",
        bytes,
        scope.clone(),
        provenance(),
        100,
    )
    .unwrap();
    version.validate().unwrap();
    let reference = version.as_ref();
    reference.validate().unwrap();
    let evidence = EvidenceRef::new(
        EvidenceId::new(),
        "verification_log",
        RunId::new(),
        Some(InvocationId::new()),
        reference.clone(),
        scope,
        provenance(),
        100,
    )
    .unwrap();
    evidence.validate().unwrap();

    let mut forged = reference;
    forged.content_hash =
        "sha256:0000000000000000000000000000000000000000000000000000000000000000".to_owned();
    forged.validate().unwrap();
    assert_ne!(forged.content_hash, version.content_hash);
}

#[test]
fn criterion_ids_keep_same_text_distinct_and_scope_mismatch_fails() {
    let scope = scope();
    let first = Criterion::new(
        CriterionId::new(),
        "charter:v1",
        1,
        "the service responds",
        "isolated_http_fixture",
        vec!["verification_log".to_owned()],
        true,
        scope.clone(),
    )
    .unwrap();
    let second = Criterion::new(
        CriterionId::new(),
        "charter:v1",
        1,
        "the service responds",
        "isolated_http_fixture",
        vec!["verification_log".to_owned()],
        true,
        scope.clone(),
    )
    .unwrap();
    assert_ne!(first.criterion_id, second.criterion_id);
    assert_ne!(first.criterion_digest, second.criterion_digest);

    let version = ArtifactVersion::new(
        kiana_domain::ArtifactId::new(),
        1,
        "kiana.test-evidence.v1",
        b"evidence",
        scope.clone(),
        provenance(),
        100,
    )
    .unwrap();
    let mismatched = EvidenceRef::new(
        EvidenceId::new(),
        "verification_log",
        RunId::new(),
        None,
        version.as_ref(),
        json_digest(&serde_json::json!({"other": true})),
        provenance(),
        100,
    );
    assert_eq!(mismatched.unwrap_err(), "evidence_scope_mismatch");

    let mut unknown = serde_json::to_value(first).unwrap();
    unknown["unknown"] = serde_json::json!(true);
    assert!(serde_json::from_value::<Criterion>(unknown).is_err());
}

#[test]
fn artifact_and_evidence_identity_ids_must_be_non_nil() {
    let scope = scope();
    let nil_artifact = kiana_domain::ArtifactId::from_uuid(Uuid::nil());
    let version = ArtifactVersion::new(
        nil_artifact,
        1,
        "kiana.test-evidence.v1",
        b"evidence",
        scope.clone(),
        provenance(),
        100,
    );
    assert_eq!(version.unwrap_err(), "artifact_version_identity_invalid");

    let valid_version = ArtifactVersion::new(
        kiana_domain::ArtifactId::new(),
        1,
        "kiana.test-evidence.v1",
        b"evidence",
        scope.clone(),
        provenance(),
        100,
    )
    .unwrap();
    let mut reference = valid_version.as_ref();
    reference.artifact_id = nil_artifact;
    assert_eq!(
        reference.validate().unwrap_err(),
        "artifact_ref_identity_invalid"
    );

    let nil_evidence = EvidenceRef::new(
        kiana_domain::EvidenceId::from_uuid(Uuid::nil()),
        "verification_log",
        RunId::new(),
        Some(InvocationId::new()),
        valid_version.as_ref(),
        scope.clone(),
        provenance(),
        100,
    );
    assert_eq!(nil_evidence.unwrap_err(), "evidence_ref_identity_invalid");

    let nil_run = EvidenceRef::new(
        EvidenceId::new(),
        "verification_log",
        RunId::from_uuid(Uuid::nil()),
        Some(InvocationId::new()),
        valid_version.as_ref(),
        scope.clone(),
        provenance(),
        100,
    );
    assert_eq!(nil_run.unwrap_err(), "evidence_ref_identity_invalid");

    let nil_invocation = EvidenceRef::new(
        EvidenceId::new(),
        "verification_log",
        RunId::new(),
        Some(InvocationId::from_uuid(Uuid::nil())),
        valid_version.as_ref(),
        scope.clone(),
        provenance(),
        100,
    );
    assert_eq!(nil_invocation.unwrap_err(), "evidence_ref_identity_invalid");

    let criterion = Criterion::new(
        CriterionId::from_uuid(Uuid::nil()),
        "charter:v1",
        1,
        "the service responds",
        "isolated_http_fixture",
        vec!["verification_log".to_owned()],
        true,
        scope,
    );
    assert_eq!(criterion.unwrap_err(), "criterion_identity_invalid");
}

#[test]
fn artifact_provenance_source_ids_must_be_non_nil() {
    let scope = scope();
    for (source_event_id, source_run_id) in [
        (Some(EventId::from_uuid(Uuid::nil())), Some(RunId::new())),
        (Some(EventId::new()), Some(RunId::from_uuid(Uuid::nil()))),
    ] {
        let mut invalid = provenance();
        invalid.source_event_id = source_event_id;
        invalid.source_run_id = source_run_id;
        assert_eq!(
            ArtifactVersion::new(
                kiana_domain::ArtifactId::new(),
                1,
                "kiana.test-evidence.v1",
                b"evidence",
                scope.clone(),
                invalid,
                100,
            )
            .expect_err("nil provenance source identity must fail closed"),
            "artifact_provenance_identity_invalid"
        );
    }
}
