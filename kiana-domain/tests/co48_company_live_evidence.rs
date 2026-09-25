use kiana_domain::{
    CompanyLiveCloseoutEvidence, CompanyLiveMode, CompanyLiveProofLevel, CompanyLiveRole,
    CompanyLiveRoleRoute, CompanyLiveStatus,
};

fn digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn route(role: CompanyLiveRole, byte: char) -> CompanyLiveRoleRoute {
    CompanyLiveRoleRoute {
        role,
        model_id: "gpt-4.1-mini".to_owned(),
        route_digest: digest(byte),
        request_count: 1,
        attempt_receipt_digest: Some(digest((byte as u8 + 1) as char)),
    }
}

fn roles() -> Vec<CompanyLiveRoleRoute> {
    vec![
        route(CompanyLiveRole::Planner, 'a'),
        route(CompanyLiveRole::Builder, 'b'),
        route(CompanyLiveRole::Reviewer, 'c'),
        route(CompanyLiveRole::Closer, 'd'),
    ]
}

fn evidence(
    mode: CompanyLiveMode,
    proof_level: CompanyLiveProofLevel,
    status: CompanyLiveStatus,
    result_unknown: bool,
    limitations: Vec<String>,
) -> Result<CompanyLiveCloseoutEvidence, String> {
    CompanyLiveCloseoutEvidence::new(
        "project-live",
        "openai-compatible",
        "gpt-4.1-mini",
        digest('e'),
        digest('f'),
        digest('0'),
        mode,
        proof_level,
        status,
        roles(),
        Some(digest('1')),
        Some(digest('2')),
        Some(digest('3')),
        Some(digest('4')),
        Some(digest('5')),
        Some(digest('6')),
        true,
        true,
        true,
        result_unknown,
        (mode == CompanyLiveMode::LiveOptIn).then(|| "approval:company-live".to_owned()),
        (mode == CompanyLiveMode::LiveOptIn).then(|| digest('7')),
        limitations,
    )
}

#[test]
fn fake_company_path_remains_partial_and_cannot_claim_live() {
    let fake = evidence(
        CompanyLiveMode::FakeCassette,
        CompanyLiveProofLevel::LocalBehavior,
        CompanyLiveStatus::Partial,
        false,
        vec!["fake model is not live provider evidence".to_owned()],
    )
    .expect("fake partial evidence");
    fake.validate().expect("fake partial validates");

    let live_claim = evidence(
        CompanyLiveMode::FakeCassette,
        CompanyLiveProofLevel::Live,
        CompanyLiveStatus::Verified,
        false,
        Vec::new(),
    );
    assert_eq!(
        live_claim.expect_err("fake path cannot verify live"),
        "company_live_fake_cannot_claim_live"
    );
}

#[test]
fn live_closeout_requires_role_receipts_approval_and_outcome_evidence() {
    let live = evidence(
        CompanyLiveMode::LiveOptIn,
        CompanyLiveProofLevel::Live,
        CompanyLiveStatus::Verified,
        false,
        Vec::new(),
    )
    .expect("complete live evidence shape");
    live.validate().expect("live evidence validates");
}

#[test]
fn result_unknown_cannot_be_promoted_to_verified_closeout() {
    let unknown = evidence(
        CompanyLiveMode::LiveOptIn,
        CompanyLiveProofLevel::Live,
        CompanyLiveStatus::Verified,
        true,
        Vec::new(),
    );
    assert_eq!(
        unknown.expect_err("unknown needs reconciliation"),
        "company_live_unknown_cannot_verify"
    );
}

#[test]
fn live_opt_in_requires_typed_approval_and_provider_revision() {
    let mut bad_approval = evidence(
        CompanyLiveMode::LiveOptIn,
        CompanyLiveProofLevel::Live,
        CompanyLiveStatus::Verified,
        false,
        Vec::new(),
    )
    .expect("live evidence shape");
    bad_approval.operator_approval_ref = Some("operator-approval".to_owned());
    bad_approval.evidence_digest = bad_approval.digest();
    assert_eq!(
        bad_approval
            .validate()
            .expect_err("approval ref must be typed"),
        "company_live_opt_in_approval_ref_invalid"
    );

    let mut missing_revision = evidence(
        CompanyLiveMode::LiveOptIn,
        CompanyLiveProofLevel::Live,
        CompanyLiveStatus::Verified,
        false,
        Vec::new(),
    )
    .expect("live evidence shape");
    missing_revision.provider_revision = "none".to_owned();
    missing_revision.evidence_digest = missing_revision.digest();
    assert_eq!(
        missing_revision
            .validate()
            .expect_err("provider revision must be bound"),
        "company_live_opt_in_provider_identity_missing"
    );
}
