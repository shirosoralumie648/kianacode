use kiana_daemon::eval_runtime::{
    EvalBoundaryEvidence, EvalCaptureStatus, EvalEvidenceCapture, EvalFileChangeKind, EvalFileDiff,
    EvalProcessObservation,
};
use kiana_domain::RunId;

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

#[test]
fn eval_evidence_has_no_unredacted_secret() {
    let evidence = EvalBoundaryEvidence::new(
        vec![EvalProcessObservation {
            pid: 42,
            parent_pid: Some(1),
            command_digest: digest('a'),
        }],
        vec![EvalFileDiff {
            path: "OUTPUT.txt".to_owned(),
            kind: EvalFileChangeKind::Modified,
            before_digest: Some(digest('b')),
            after_digest: Some(digest('c')),
        }],
        0,
        Vec::new(),
    )
    .unwrap();
    assert!(evidence.is_safe());

    let unsafe_evidence =
        EvalBoundaryEvidence::new(Vec::new(), Vec::new(), 2, vec!["api_key".to_owned()]).unwrap();
    assert!(!unsafe_evidence.is_safe());
    assert!(serde_json::to_string(&unsafe_evidence)
        .unwrap()
        .contains("api_key"));
    assert!(!serde_json::to_string(&unsafe_evidence)
        .unwrap()
        .contains("raw-secret"));
}

#[test]
fn boundary_evidence_blocks_capture_and_rejects_path_or_pattern_drift() {
    let unsafe_evidence =
        EvalBoundaryEvidence::new(Vec::new(), Vec::new(), 1, vec!["token".to_owned()]).unwrap();
    let mut capture = EvalEvidenceCapture::new(RunId::new()).unwrap();
    capture.attach_safety_evidence(unsafe_evidence).unwrap();
    let receipt = capture.finish(Ok(()));
    assert_eq!(receipt.status, EvalCaptureStatus::SafetyViolation);
    assert_eq!(receipt.failure_code.as_deref(), Some("safety_violation"));
    receipt.validate().unwrap();

    assert!(EvalBoundaryEvidence::new(
        Vec::new(),
        vec![EvalFileDiff {
            path: "../escape".to_owned(),
            kind: EvalFileChangeKind::Added,
            before_digest: None,
            after_digest: Some(digest('d')),
        }],
        0,
        Vec::new(),
    )
    .is_err());
    assert!(EvalBoundaryEvidence::new(
        Vec::new(),
        Vec::new(),
        0,
        vec!["unknown-pattern".to_owned()],
    )
    .is_err());
}
