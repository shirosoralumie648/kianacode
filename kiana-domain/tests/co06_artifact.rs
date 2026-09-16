use kiana_domain::{
    json_digest, ArtifactProvenance, ArtifactVersion, Criterion, CriterionId, EventId, EvidenceId,
    EvidenceRef, InvocationId, ProjectId, RunId,
};

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
