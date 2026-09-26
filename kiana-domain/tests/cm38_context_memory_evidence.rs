use kiana_domain::{
    ContextMemoryEvidenceStatus, ContextMemoryGoldenPathEvidence, ContextMemoryProofLevel,
    ContextMemoryProviderMode, ContextMemoryStageDigests,
};

fn digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn stages() -> ContextMemoryStageDigests {
    ContextMemoryStageDigests {
        context_plan_digest: Some(digest('a')),
        provider_request_digest: Some(digest('b')),
        tool_receipt_digest: Some(digest('c')),
        retrieval_digest: Some(digest('d')),
        candidate_digest: Some(digest('e')),
        approval_receipt_digest: Some(digest('f')),
        projection_digest: Some(digest('0')),
        recovery_digest: Some(digest('1')),
        run_receipt_digest: Some(digest('2')),
    }
}

#[test]
fn fake_golden_path_requires_every_stage_but_cannot_claim_live() {
    let evidence = ContextMemoryGoldenPathEvidence::new(
        ContextMemoryProviderMode::FakeCassette,
        ContextMemoryProofLevel::LocalBehavior,
        digest('3'),
        digest('4'),
        stages(),
        None,
        None,
        ContextMemoryEvidenceStatus::Verified,
        Vec::new(),
    )
    .expect("complete fake fixture");
    evidence.validate().expect("fake fixture validates");

    let live_claim = ContextMemoryGoldenPathEvidence::new(
        ContextMemoryProviderMode::FakeCassette,
        ContextMemoryProofLevel::Live,
        digest('3'),
        digest('4'),
        stages(),
        Some(digest('5')),
        Some("approval:live".to_owned()),
        ContextMemoryEvidenceStatus::Verified,
        Vec::new(),
    );
    assert_eq!(
        live_claim.expect_err("fake fixture cannot claim live"),
        "context_memory_fake_cannot_claim_live"
    );
}

#[test]
fn partial_path_keeps_scope_redaction_and_limitation() {
    let evidence = ContextMemoryGoldenPathEvidence::new(
        ContextMemoryProviderMode::FakeCassette,
        ContextMemoryProofLevel::Source,
        digest('3'),
        digest('4'),
        ContextMemoryStageDigests::default(),
        None,
        None,
        ContextMemoryEvidenceStatus::Partial,
        vec!["candidate approval and restart recovery are not wired".to_owned()],
    )
    .expect("partial evidence");
    evidence.validate().expect("partial evidence validates");
}

#[test]
fn live_proof_requires_explicit_opt_in_and_independent_evidence() {
    let missing = ContextMemoryGoldenPathEvidence::new(
        ContextMemoryProviderMode::LiveOptIn,
        ContextMemoryProofLevel::Live,
        digest('3'),
        digest('4'),
        stages(),
        None,
        None,
        ContextMemoryEvidenceStatus::Verified,
        Vec::new(),
    );
    assert_eq!(
        missing.expect_err("live proof needs approval and provider evidence"),
        "context_memory_live_opt_in_evidence_missing"
    );

    let evidence = ContextMemoryGoldenPathEvidence::new(
        ContextMemoryProviderMode::LiveOptIn,
        ContextMemoryProofLevel::Live,
        digest('3'),
        digest('4'),
        stages(),
        Some(digest('5')),
        Some("approval:live".to_owned()),
        ContextMemoryEvidenceStatus::Verified,
        Vec::new(),
    )
    .expect("explicit live evidence");
    evidence.validate().expect("live evidence validates");
}

#[test]
fn live_context_memory_evidence_requires_typed_operator_approval() {
    let error = ContextMemoryGoldenPathEvidence::new(
        ContextMemoryProviderMode::LiveOptIn,
        ContextMemoryProofLevel::Live,
        digest('3'),
        digest('4'),
        stages(),
        Some(digest('5')),
        Some("operator-approval".to_owned()),
        ContextMemoryEvidenceStatus::Verified,
        Vec::new(),
    )
    .expect_err("approval ref must be typed");
    assert_eq!(error, "context_memory_live_approval_ref_invalid");
}
