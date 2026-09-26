use kiana_domain::*;

fn bundle() -> CompanyEvidenceBundle {
    let run_id = RunId::new();
    let invocation_id = InvocationId::new();
    let request_id = RequestId::new();
    let receipt = RuntimeReceiptRef::new(
        request_id,
        ExecutionStatus::Completed,
        vec!["event:execution-result".to_owned()],
    )
    .expect("receipt");
    let mut bundle = CompanyEvidenceBundle {
        schema: COMPANY_EVIDENCE_SCHEMA.to_owned(),
        bundle_id: "bundle-1".to_owned(),
        project_id: "project-1".to_owned(),
        packet_id: "packet-1".to_owned(),
        packet_version: 2,
        run_id,
        invocation_id,
        request_id,
        operation: "cargo test --test co24".to_owned(),
        command_digest: format!("sha256:{}", "a".repeat(64)),
        source_revision: "source-rev-1".to_owned(),
        workspace_revision: "workspace-rev-1".to_owned(),
        exit_code: Some(0),
        runtime_receipt: receipt,
        file_changes: vec![CompanyEvidenceFileChange {
            path: "src/lib.rs".to_owned(),
            operation: "modified".to_owned(),
            content_hash: format!("sha256:{}", "b".repeat(64)),
        }],
        artifacts: vec![CompanyEvidenceArtifact {
            artifact_ref: "artifact:output".to_owned(),
            project_id: "project-1".to_owned(),
            packet_id: "packet-1".to_owned(),
            run_id,
            content_hash: format!("sha256:{}", "c".repeat(64)),
        }],
        tests: vec![CompanyEvidenceTestResult {
            command: "cargo test --test co24".to_owned(),
            exit_code: 0,
            matched_tests: 3,
            source_revision: "source-rev-1".to_owned(),
            output_digest: format!("sha256:{}", "d".repeat(64)),
        }],
        model_claimed: false,
        result_unknown: false,
        digest: String::new(),
    };
    bundle.digest = bundle.canonical_digest();
    bundle
}

#[test]
fn evidence_bundle_rejects_model_claims_zero_test_matches_and_foreign_artifacts() {
    let original = bundle();
    let mut model_claim = original.clone();
    model_claim.model_claimed = true;
    model_claim.digest = model_claim.canonical_digest();
    assert_eq!(
        model_claim.validate().unwrap_err(),
        "company_evidence_runtime_or_identity_invalid"
    );

    let mut zero_tests = original.clone();
    zero_tests.tests[0].matched_tests = 0;
    zero_tests.digest = zero_tests.canonical_digest();
    assert_eq!(
        zero_tests.validate().unwrap_err(),
        "company_evidence_test_zero_matches"
    );

    let mut foreign = original.clone();
    foreign.artifacts[0].packet_id = "packet-foreign".to_owned();
    foreign.digest = foreign.canonical_digest();
    assert_eq!(
        foreign.validate().unwrap_err(),
        "company_evidence_foreign_artifact"
    );
}

#[test]
fn builder_evidence_links_each_output_to_the_actual_run_and_invocation() {
    let bundle = bundle();
    assert!(bundle.validate().is_ok());
    let ready = CompanyEvidenceReady::from_bundle(&bundle, 42, 100).expect("ready");
    assert_eq!(ready.bundle_digest, bundle.digest);
    assert_eq!(ready.run_id, bundle.run_id);
    assert_eq!(ready.invocation_id, bundle.invocation_id);
    assert!(ready.validate().is_ok());
    let reopened: CompanyEvidenceBundle =
        serde_json::from_value(serde_json::to_value(&bundle).expect("serialize")).expect("reopen");
    assert_eq!(reopened, bundle);
}

#[test]
fn unknown_or_source_drift_cannot_become_evidence_ready() {
    let original = bundle();
    let mut unknown = original.clone();
    unknown.result_unknown = true;
    unknown.digest = unknown.canonical_digest();
    assert_eq!(
        unknown.validate().unwrap_err(),
        "company_evidence_runtime_or_identity_invalid"
    );
    let mut drift = original.clone();
    drift.tests[0].source_revision = "source-rev-2".to_owned();
    drift.digest = drift.canonical_digest();
    assert_eq!(
        drift.validate().unwrap_err(),
        "company_evidence_test_source_revision_mismatch"
    );
}
