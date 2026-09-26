use kiana_domain::*;

fn hash(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn pending() -> Er34DurableGateEvidence {
    let mut value = Er34DurableGateEvidence {
        schema: ER34_DURABLE_GATE_SCHEMA.to_owned(),
        snapshot_digest: hash('a'),
        command_digest: hash('b'),
        origin: Er34GateOrigin::GitHubActions,
        result: Er34GateResult::Pending,
        proof_level: Er34ProofLevel::Source,
        ci_focused_tests_executed: false,
        local_tests_executed: false,
        durable_facts_observed: false,
        cache_deleted_and_rebuilt: false,
        event_frame_hash: None,
        artifact_hash: None,
        process_fact_digest: None,
        limitations: vec!["CI has not returned a durable observation".to_owned()],
        digest: String::new(),
    };
    value.digest = value.canonical_digest();
    value
}

#[test]
fn ci_only_pending_gate_does_not_promote_durable_proof() {
    let value = pending();
    value.validate().expect("pending source evidence");
    assert_eq!(value.proof_level, Er34ProofLevel::Source);
    assert_eq!(value.result, Er34GateResult::Pending);
}

#[test]
fn forged_durable_claim_missing_physical_facts_is_rejected() {
    let mut value = pending();
    value.proof_level = Er34ProofLevel::Durable;
    value.result = Er34GateResult::Passed;
    value.ci_focused_tests_executed = true;
    value.digest = value.canonical_digest();
    assert_eq!(
        value.validate().unwrap_err(),
        "er34_durable_proof_evidence_incomplete"
    );

    let mut source_pass = pending();
    source_pass.result = Er34GateResult::Passed;
    source_pass.digest = source_pass.canonical_digest();
    assert_eq!(
        source_pass.validate().unwrap_err(),
        "er34_source_pass_cannot_claim_gate"
    );
}

#[test]
fn durable_gate_requires_ci_facts_cache_rebuild_and_hashes() {
    let mut value = pending();
    value.result = Er34GateResult::Passed;
    value.proof_level = Er34ProofLevel::Durable;
    value.ci_focused_tests_executed = true;
    value.durable_facts_observed = true;
    value.cache_deleted_and_rebuilt = true;
    value.event_frame_hash = Some(hash('c'));
    value.artifact_hash = Some(hash('d'));
    value.process_fact_digest = Some(hash('e'));
    value.limitations = vec!["remote runner durable observation only".to_owned()];
    value.digest = value.canonical_digest();
    value.validate().expect("complete durable evidence shape");
}
