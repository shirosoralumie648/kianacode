use kiana_protocol::{
    EvidenceLimitation, UiEvidenceBundle, UiEvidenceCase, UiEvidenceClass, UiEvidenceOutcome,
    UiFeatureStatus, UiProofLevel, UiSurface,
};

fn digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn case(
    id: &str,
    class: UiEvidenceClass,
    outcome: UiEvidenceOutcome,
    proof_level: UiProofLevel,
    feature_status: UiFeatureStatus,
    exit_code: Option<i32>,
    receipt_digest: Option<String>,
    limitations: Vec<EvidenceLimitation>,
) -> UiEvidenceCase {
    UiEvidenceCase::new(
        id,
        UiSurface::Web,
        class,
        vec!["cargo".to_owned(), "test".to_owned(), id.to_owned()],
        "git:ui-40-snapshot",
        digest('a'),
        digest('b'),
        exit_code,
        feature_status,
        proof_level,
        outcome,
        receipt_digest,
        vec![digest('c')],
        limitations,
        "ci-reviewer",
    )
    .expect("evidence case")
}

#[test]
fn bundle_binds_source_and_keeps_skips_explicit() {
    let bundle = UiEvidenceBundle::new(
        "git:ui-40-snapshot",
        vec![
            case(
                "deny",
                UiEvidenceClass::Deny,
                UiEvidenceOutcome::Passed,
                UiProofLevel::Source,
                UiFeatureStatus::Partial,
                Some(0),
                None,
                vec![EvidenceLimitation {
                    code: "source_only".to_owned(),
                    detail: "CI source fixture does not prove runtime behavior".to_owned(),
                }],
            ),
            case(
                "live",
                UiEvidenceClass::Live,
                UiEvidenceOutcome::Skipped,
                UiProofLevel::Source,
                UiFeatureStatus::NotSupported,
                None,
                None,
                vec![EvidenceLimitation {
                    code: "no_host".to_owned(),
                    detail: "No authorized ACP or IDE host is configured".to_owned(),
                }],
            ),
        ],
    )
    .expect("bundle");
    bundle.validate().expect("bundle validates");
}

#[test]
fn implemented_source_and_live_source_pass_are_rejected() {
    let implemented = case(
        "implemented-source",
        UiEvidenceClass::Happy,
        UiEvidenceOutcome::Passed,
        UiProofLevel::Source,
        UiFeatureStatus::Implemented,
        Some(0),
        None,
        Vec::new(),
    );
    assert_eq!(
        implemented
            .validate()
            .expect_err("source cannot claim implemented"),
        "ui_evidence_implemented_requires_behavior"
    );

    let live = case(
        "live-source",
        UiEvidenceClass::Live,
        UiEvidenceOutcome::Passed,
        UiProofLevel::Source,
        UiFeatureStatus::Partial,
        Some(0),
        None,
        Vec::new(),
    );
    assert_eq!(
        live.validate().expect_err("live pass needs live proof"),
        "ui_evidence_live_proof_required"
    );
}

#[test]
fn command_secrets_and_missing_unknown_limits_fail_closed() {
    let secret = UiEvidenceCase::new(
        "secret",
        UiSurface::Cli,
        UiEvidenceClass::Deny,
        vec!["kiana".to_owned(), "--api_key=raw".to_owned()],
        "git:ui-40-snapshot",
        digest('a'),
        digest('b'),
        Some(0),
        UiFeatureStatus::Partial,
        UiProofLevel::Source,
        UiEvidenceOutcome::Passed,
        None,
        Vec::new(),
        Vec::new(),
        "ci-reviewer",
    );
    assert_eq!(
        secret.expect_err("raw command secret must be rejected"),
        "ui_evidence_secret_in_command_argv"
    );

    let unknown = UiEvidenceCase::new(
        "unknown",
        UiSurface::Cli,
        UiEvidenceClass::Recovery,
        Vec::new(),
        "git:ui-40-snapshot",
        digest('a'),
        digest('b'),
        None,
        UiFeatureStatus::Partial,
        UiProofLevel::Source,
        UiEvidenceOutcome::Unknown,
        None,
        Vec::new(),
        Vec::new(),
        "ci-reviewer",
    );
    assert_eq!(
        unknown.expect_err("unknown must carry limits and command"),
        "ui_evidence_command_argv_invalid"
    );
}
