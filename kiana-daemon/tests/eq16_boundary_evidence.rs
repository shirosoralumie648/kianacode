use kiana_daemon::eval_runtime::{
    EvalBoundaryEvidence, EvalCaptureStatus, EvalEvidenceCapture, EvalFileChangeKind, EvalFileDiff,
    EvalProcessObservation,
};
use kiana_domain::{EvalVerdict, RunId};

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
    // Safe boundary evidence clears the pass gate and preserves a candidate pass.
    assert!(!evidence.blocks_eval_pass());
    assert_eq!(
        evidence.apply_boundary_invariant(EvalVerdict::Pass),
        EvalVerdict::Pass
    );

    let unsafe_evidence =
        EvalBoundaryEvidence::new(Vec::new(), Vec::new(), 2, vec!["api_key".to_owned()]).unwrap();
    assert!(!unsafe_evidence.is_safe());
    // Only the allow-listed pattern code is captured; the raw secret value never appears.
    let serialized = serde_json::to_string(&unsafe_evidence).unwrap();
    assert!(serialized.contains("api_key"));
    assert!(!serialized.contains("raw-secret"));
    // Unsafe evidence can never be reported as a passing evaluation.
    assert!(unsafe_evidence.blocks_eval_pass());
    assert_eq!(
        unsafe_evidence.apply_boundary_invariant(EvalVerdict::Pass),
        EvalVerdict::Blocked
    );
    // Non-passing verdicts are preserved unchanged; only `Pass` is downgraded.
    assert_eq!(
        unsafe_evidence.apply_boundary_invariant(EvalVerdict::Fail),
        EvalVerdict::Fail
    );

    // A raw secret value is not a pattern code and is rejected rather than captured.
    assert_eq!(
        EvalBoundaryEvidence::new(Vec::new(), Vec::new(), 0, vec!["sk-live-abcdef".to_owned()])
            .unwrap_err(),
        "eval_boundary_secret_pattern_unknown"
    );
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
    // A safety-violation capture can never be reported as a passing evaluation.
    assert!(receipt.status.blocks_eval_pass());
    assert_eq!(
        receipt.apply_flush_invariant(EvalVerdict::Pass),
        EvalVerdict::Blocked
    );

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

#[test]
fn boundary_evidence_rejects_digest_or_pattern_tamper() {
    let evidence = EvalBoundaryEvidence::new(
        Vec::new(),
        Vec::new(),
        1,
        vec!["api_key".to_owned(), "token".to_owned()],
    )
    .unwrap();
    evidence.validate().unwrap();
    assert!(evidence.blocks_eval_pass());

    // A rewritten evidence digest is rejected before the evidence can be trusted.
    let mut tampered_digest = evidence.clone();
    tampered_digest.evidence_digest = "sha256:tampered".to_owned();
    assert_eq!(
        tampered_digest.validate().unwrap_err(),
        "eval_boundary_evidence_header_invalid"
    );

    // Rewriting the network observation without re-sealing the digest is caught by the binding.
    let mut tampered_network = evidence.clone();
    tampered_network.network_syscalls = 0;
    assert_eq!(
        tampered_network.validate().unwrap_err(),
        "eval_boundary_evidence_header_invalid"
    );

    // Secret pattern codes must be unique and strictly sorted, or they are rejected.
    assert_eq!(
        EvalBoundaryEvidence::new(
            Vec::new(),
            Vec::new(),
            0,
            vec!["token".to_owned(), "api_key".to_owned()],
        )
        .unwrap_err(),
        "eval_boundary_secret_pattern_order"
    );
    assert_eq!(
        EvalBoundaryEvidence::new(
            Vec::new(),
            Vec::new(),
            0,
            vec!["token".to_owned(), "token".to_owned()],
        )
        .unwrap_err(),
        "eval_boundary_secret_pattern_order"
    );
}
