use kiana_domain::{
    json_digest, ConformanceBackend, ConformanceCase, ConformanceCaseStatus,
    ConformanceMatrixReport, ConformanceProfile, ConformanceScenario, ConformanceTool,
};
use serde_json::json;

const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn case(
    id: &str,
    backend: ConformanceBackend,
    tool: ConformanceTool,
    scenario: ConformanceScenario,
    status: ConformanceCaseStatus,
    reason: &str,
) -> ConformanceCase {
    let mut value = ConformanceCase {
        case_id: id.to_owned(),
        backend,
        profile: ConformanceProfile::ReadOnly,
        tool,
        scenario,
        status,
        reason: reason.to_owned(),
        backend_disposition: (status == ConformanceCaseStatus::Verified)
            .then_some(kiana_domain::PlatformBackendDisposition::Implemented),
        evidence_digest: None,
        grant_digest: DIGEST_A.to_owned(),
        observed_grant_digest: DIGEST_A.to_owned(),
        catalog_epoch: 1,
        credential_epoch: 1,
        data_epoch: 1,
        effect_fence_verified: status == ConformanceCaseStatus::Verified,
        resume_fence_verified: status == ConformanceCaseStatus::Verified,
    };
    if status == ConformanceCaseStatus::Verified {
        value.evidence_digest = Some(json_digest(&json!({"case": id})));
    }
    value
}

#[test]
fn conformance_matrix_does_not_count_skips_as_verified() {
    let report = ConformanceMatrixReport::evaluate(&[
        case(
            "linux-shell-success",
            ConformanceBackend::Linux,
            ConformanceTool::Shell,
            ConformanceScenario::Success,
            ConformanceCaseStatus::Verified,
            "ci fixture",
        ),
        case(
            "container-shell-success",
            ConformanceBackend::Container,
            ConformanceTool::Shell,
            ConformanceScenario::Success,
            ConformanceCaseStatus::NotImplemented,
            "container runtime fixture pending",
        ),
        case(
            "macos-http-not-applicable",
            ConformanceBackend::Macos,
            ConformanceTool::HttpMcp,
            ConformanceScenario::Success,
            ConformanceCaseStatus::NotApplicable,
            "macOS backend not implemented",
        ),
    ])
    .unwrap();
    report.validate().unwrap();
    assert_eq!(report.verified_count, 1);
    assert_eq!(report.not_implemented_count, 1);
    assert_eq!(report.not_applicable_count, 1);
    assert_eq!(
        report.status,
        kiana_domain::ConformanceMatrixStatus::Partial
    );
}

#[test]
fn conformance_matrix_blocks_grant_drift_and_effect_fence_bypass() {
    let mut grant = case(
        "grant-drift",
        ConformanceBackend::Linux,
        ConformanceTool::Shell,
        ConformanceScenario::Success,
        ConformanceCaseStatus::Verified,
        "fixture",
    );
    grant.observed_grant_digest = DIGEST_B.to_owned();
    assert_eq!(
        grant.validate().unwrap_err(),
        "backend_switch_cannot_expand_existing_grant"
    );

    let mut resume = case(
        "resume-fence",
        ConformanceBackend::Linux,
        ConformanceTool::StdioMcp,
        ConformanceScenario::Resume,
        ConformanceCaseStatus::Verified,
        "fixture",
    );
    resume.resume_fence_verified = false;
    assert_eq!(
        resume.validate().unwrap_err(),
        "extension_transport_and_resume_cannot_bypass_effect_fences"
    );

    let blocked = case(
        "blocked",
        ConformanceBackend::Windows,
        ConformanceTool::ApplyPatch,
        ConformanceScenario::Cancel,
        ConformanceCaseStatus::Blocked,
        "target backend unavailable",
    );
    let report = ConformanceMatrixReport::evaluate(&[blocked]).unwrap();
    assert_eq!(
        report.status,
        kiana_domain::ConformanceMatrixStatus::Blocked
    );
}

#[test]
fn verified_case_requires_implemented_backend_disposition() {
    let mut value = case(
        "target-not-verified",
        ConformanceBackend::Macos,
        ConformanceTool::Shell,
        ConformanceScenario::Success,
        ConformanceCaseStatus::Verified,
        "target receipt pending",
    );
    value.backend_disposition = Some(kiana_domain::PlatformBackendDisposition::TargetOnly);
    assert_eq!(
        value.validate().unwrap_err(),
        "verified_backend_disposition_missing"
    );

    let mut implemented = case(
        "implemented-not-verified",
        ConformanceBackend::Windows,
        ConformanceTool::Shell,
        ConformanceScenario::Success,
        ConformanceCaseStatus::NotImplemented,
        "target backend pending",
    );
    implemented.backend_disposition = Some(kiana_domain::PlatformBackendDisposition::Implemented);
    assert_eq!(
        implemented.validate().unwrap_err(),
        "implemented_backend_requires_verified_case"
    );
}

#[test]
fn conformance_report_rejects_duplicate_case_and_unknown_fields() {
    let one = case(
        "duplicate",
        ConformanceBackend::Linux,
        ConformanceTool::Shell,
        ConformanceScenario::Success,
        ConformanceCaseStatus::NotImplemented,
        "fixture pending",
    );
    assert_eq!(
        ConformanceMatrixReport::evaluate(&[one.clone(), one]).unwrap_err(),
        "conformance_matrix_duplicate_case"
    );
    let report = ConformanceMatrixReport::evaluate(&[case(
        "strict",
        ConformanceBackend::Linux,
        ConformanceTool::Shell,
        ConformanceScenario::Failure,
        ConformanceCaseStatus::NotImplemented,
        "fixture pending",
    )])
    .unwrap();
    let mut value = serde_json::to_value(report).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<ConformanceMatrixReport>(value).is_err());
}

#[test]
fn conformance_report_rejects_forged_status_and_reason_counts() {
    let report = ConformanceMatrixReport::evaluate(&[case(
        "forged-report",
        ConformanceBackend::Linux,
        ConformanceTool::Shell,
        ConformanceScenario::Success,
        ConformanceCaseStatus::Verified,
        "fixture",
    )])
    .unwrap();
    let mut forged = report;
    forged.status = kiana_domain::ConformanceMatrixStatus::Complete;
    forged.not_implemented_count = 1;
    forged.blocking_reasons = vec!["forged:pending".to_owned()];
    forged.report_digest = forged.digest();
    assert_eq!(
        forged.validate().unwrap_err(),
        "conformance_matrix_status_mismatch"
    );
}
