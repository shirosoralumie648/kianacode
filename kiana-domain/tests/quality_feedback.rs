use kiana_domain::{
    ArtifactId, FeedbackId, QualityCanonicalTarget, QualityFeedback, QualityFeedbackPrivacyClass,
    QualityFeedbackPrivacyScope, QualityFeedbackServerContext, QualityFeedbackSubmission,
    QualityFeedbackTargetType,
};
use serde_json::json;

const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
fn target() -> QualityCanonicalTarget {
    QualityCanonicalTarget::new(
        QualityFeedbackTargetType::Artifact,
        format!("artifact:{}", ArtifactId::new()),
        DIGEST_A,
        Some("policy:policy-1".to_owned()),
        Some("receipt:receipt-1".to_owned()),
        1,
    )
    .unwrap()
}

fn submission(target: QualityCanonicalTarget) -> QualityFeedbackSubmission {
    QualityFeedbackSubmission {
        schema: "kiana.quality-feedback-submission.v1".to_owned(),
        feedback_id: FeedbackId::new(),
        target,
        label: "correct".to_owned(),
        comment_ref: Some("comment:ref-1".to_owned()),
        correction_ref: None,
        idempotency_key: "idempotency:feedback-1".to_owned(),
    }
}

fn context(class: QualityFeedbackPrivacyClass) -> QualityFeedbackServerContext {
    QualityFeedbackServerContext {
        principal_ref: "principal:user-1".to_owned(),
        project_ref: "project:project-1".to_owned(),
        session_ref: "session:session-1".to_owned(),
        source_cursor: 7,
        source_event_ids: vec!["event:event-1".to_owned()],
        source_event_digest: DIGEST_B.to_owned(),
        target_privacy_class: class,
        captured_at_unix_ms: 100,
    }
}

#[test]
fn server_derives_feedback_provenance_and_privacy_scope() {
    let feedback = QualityFeedback::derive(
        submission(target()),
        context(QualityFeedbackPrivacyClass::Confidential),
    )
    .unwrap();
    assert_eq!(
        feedback.privacy_scope,
        QualityFeedbackPrivacyScope::Principal
    );
    assert_eq!(
        feedback.provenance.target_digest,
        feedback.target.canonical_digest
    );
    assert!(feedback.validate().is_ok());
    assert!(feedback.canonical_bytes().is_ok());
}

#[test]
fn feedback_cannot_edit_policy_or_receipt_reference() {
    let feedback = QualityFeedback::derive(
        submission(target()),
        context(QualityFeedbackPrivacyClass::Internal),
    )
    .unwrap();
    let mut value = serde_json::to_value(&feedback).unwrap();
    value["target"]["policy_ref"] = json!("policy:other");
    let mutated: QualityFeedback = serde_json::from_value(value).unwrap();
    assert!(mutated.validate().is_err());

    let mut value = serde_json::to_value(&feedback).unwrap();
    value["target"]["receipt_ref"] = json!("receipt:other");
    let mutated: QualityFeedback = serde_json::from_value(value).unwrap();
    assert!(mutated.validate().is_err());
}

#[test]
fn feedback_rejects_secret_observation_and_unknown_fields() {
    let mut value = serde_json::to_value(
        QualityFeedback::derive(
            submission(target()),
            context(QualityFeedbackPrivacyClass::Public),
        )
        .unwrap(),
    )
    .unwrap();
    value["label"] = json!("api_key=secret-value");
    let mutated: QualityFeedback = serde_json::from_value(value).unwrap();
    assert!(mutated.validate().is_err());

    let mut submission = serde_json::to_value(submission(target())).unwrap();
    submission["unexpected"] = json!(true);
    assert!(serde_json::from_value::<QualityFeedbackSubmission>(submission).is_err());
}

#[test]
fn feedback_requires_valid_server_context() {
    let mut server_context = context(QualityFeedbackPrivacyClass::Internal);
    server_context.source_cursor = 0;
    assert!(QualityFeedback::derive(submission(target()), server_context).is_err());
}
