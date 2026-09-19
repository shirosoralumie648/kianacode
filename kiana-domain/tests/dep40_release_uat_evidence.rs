use kiana_domain::{
    ReleaseUatEvidence, ReleaseUatEvidenceStatus, ReleaseUatProofLevel, UatProviderMode,
};

const A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn evidence(
    provider_mode: UatProviderMode,
    proof_level: ReleaseUatProofLevel,
    status: ReleaseUatEvidenceStatus,
    approval: Option<String>,
    receipts: Vec<String>,
    unknown_reconciled: bool,
    limitations: Vec<String>,
) -> Result<ReleaseUatEvidence, String> {
    ReleaseUatEvidence::new(
        A,
        B,
        "ci:dep40-1",
        provider_mode,
        proof_level,
        status,
        approval,
        receipts,
        unknown_reconciled,
        "uat-reviewer",
        limitations,
    )
}

#[test]
fn fake_uat_fixture_retains_source_ceiling_and_limitations() {
    let value = evidence(
        UatProviderMode::Fake,
        ReleaseUatProofLevel::Source,
        ReleaseUatEvidenceStatus::Fixture,
        None,
        Vec::new(),
        false,
        vec!["fake matrix is not durable or live deployment evidence".to_owned()],
    )
    .expect("fixture evidence");
    value.validate().expect("fixture validates");
}

#[test]
fn fake_live_and_incomplete_verified_claims_fail_closed() {
    let fake_live = evidence(
        UatProviderMode::Fake,
        ReleaseUatProofLevel::Live,
        ReleaseUatEvidenceStatus::Verified,
        None,
        vec![A.to_owned()],
        true,
        Vec::new(),
    );
    assert_eq!(
        fake_live.expect_err("fake cannot claim live"),
        "release_uat_fake_cannot_claim_live"
    );

    let incomplete = evidence(
        UatProviderMode::LiveOptIn,
        ReleaseUatProofLevel::Live,
        ReleaseUatEvidenceStatus::Verified,
        Some("approval:uat".to_owned()),
        Vec::new(),
        false,
        Vec::new(),
    );
    assert_eq!(
        incomplete.expect_err("verified needs receipt and reconciliation"),
        "release_uat_verified_evidence_incomplete"
    );
}

#[test]
fn live_verified_uat_requires_approval_receipts_and_unknown_reconciliation() {
    let value = evidence(
        UatProviderMode::LiveOptIn,
        ReleaseUatProofLevel::Live,
        ReleaseUatEvidenceStatus::Verified,
        Some("approval:uat".to_owned()),
        vec![A.to_owned()],
        true,
        Vec::new(),
    )
    .expect("verified live UAT shape");
    value.validate().expect("live UAT evidence validates");
}
