use kiana_domain::*;
use serde_json::json;

fn source_digest() -> String {
    format!("sha256:{}", "a".repeat(64))
}

#[test]
fn quality_ids_and_state_transitions_are_validated() {
    let mut artifact =
        QualityArtifact::new("suite", "quality-owner", source_digest(), 100).unwrap();
    assert_eq!(artifact.status, QualityArtifactStatus::Draft);
    let admitted = artifact
        .transition(QualityArtifactStatus::Admitted, 101)
        .unwrap();
    assert_eq!(admitted.from, QualityArtifactStatus::Draft);
    assert_eq!(admitted.to, QualityArtifactStatus::Admitted);
    artifact
        .transition(QualityArtifactStatus::Running, 102)
        .unwrap();
    artifact
        .transition(QualityArtifactStatus::Completed, 103)
        .unwrap();
    artifact
        .transition(QualityArtifactStatus::Passed, 104)
        .unwrap();
    assert!(artifact.status.is_terminal());
    assert_eq!(
        artifact
            .transition(QualityArtifactStatus::Running, 105)
            .unwrap_err(),
        "quality_transition_invalid"
    );
    assert_eq!(
        QualityStateTransition::new(
            artifact.artifact_id,
            "suite",
            QualityArtifactStatus::Draft,
            QualityArtifactStatus::Passed,
            1,
            "invalid direct transition",
        )
        .unwrap_err(),
        "quality_transition_invalid"
    );
    artifact.validate().unwrap();
}

#[test]
fn quality_contract_rejects_unknown_fields_bad_digest_owner_and_revision_time() {
    let artifact = QualityArtifact::new("case", "quality-owner", source_digest(), 100).unwrap();
    let mut encoded = serde_json::to_value(&artifact).unwrap();
    encoded["unexpected"] = json!(true);
    assert!(serde_json::from_value::<QualityArtifact>(encoded).is_err());

    let mut bad_digest = artifact.clone();
    bad_digest.source_digest = "not-a-digest".to_owned();
    bad_digest.artifact_digest = bad_digest.digest();
    assert_eq!(
        bad_digest.validate().unwrap_err(),
        "quality_source_digest_invalid"
    );

    assert_eq!(
        QualityArtifact::new("suite", "", source_digest(), 100).unwrap_err(),
        "quality_owner_invalid"
    );
    let mut time_regression = artifact.clone();
    assert_eq!(
        time_regression
            .transition(QualityArtifactStatus::Admitted, 99)
            .unwrap_err(),
        "quality_transition_time_regression"
    );
    let canonical = canonical_quality_bytes(&artifact).unwrap();
    let decoded: QualityArtifact = serde_json::from_slice(&canonical).unwrap();
    assert_eq!(decoded, artifact);
}
