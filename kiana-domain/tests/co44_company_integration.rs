use kiana_domain::*;
use std::collections::BTreeMap;

fn plan() -> CompanyIntegrationPlan {
    let mut value = CompanyIntegrationPlan {
        schema: COMPANY_INTEGRATION_SCHEMA.to_owned(),
        integration_id: "integration-1".to_owned(),
        project_id: "project-1".to_owned(),
        base_revision: 4,
        output_contract: "receipt-only".to_owned(),
        child_output_digests: BTreeMap::from([
            (
                "partition-a".to_owned(),
                format!("sha256:{}", "a".repeat(64)),
            ),
            (
                "partition-b".to_owned(),
                format!("sha256:{}", "b".repeat(64)),
            ),
        ]),
        conflict_ids: vec!["conflict-1".to_owned()],
        integrator_ref: "integrator-1".to_owned(),
        approval_ref: "approval:merge-1".to_owned(),
        digest: String::new(),
    };
    value.digest = value.canonical_digest();
    value
}

fn decision() -> CompanyConflictDecision {
    let mut value = CompanyConflictDecision {
        conflict_id: "conflict-1".to_owned(),
        path: "report.md".to_owned(),
        resolution: "keep reviewed child output".to_owned(),
        reviewer_ref: "reviewer-1".to_owned(),
        evidence_refs: vec!["evidence:conflict-1".to_owned()],
        digest: String::new(),
    };
    value.digest = value.canonical_digest();
    value
}

#[test]
fn integration_rejects_stale_base_unreviewed_outputs_and_hidden_conflict_resolution() {
    let plan = plan();
    let mut stale = plan.clone();
    stale.base_revision = 3;
    stale.digest = plan.digest.clone();
    assert_eq!(
        stale.validate().unwrap_err(),
        "company_integration_plan_digest_mismatch"
    );

    let mut receipt = CompanyMergeReceipt {
        schema: COMPANY_INTEGRATION_SCHEMA.to_owned(),
        receipt_id: "receipt-1".to_owned(),
        integration_id: plan.integration_id.clone(),
        plan_digest: plan.digest.clone(),
        base_revision: plan.base_revision,
        result_revision: 5,
        child_output_digests: plan.child_output_digests.clone(),
        conflict_decisions: Vec::new(),
        revalidated: true,
        accepted: true,
        pushed_or_published: false,
        evidence_refs: vec!["evidence:merge".to_owned()],
        digest: String::new(),
    };
    receipt.digest = receipt.canonical_digest();
    assert_eq!(
        receipt.validate_against(&plan).unwrap_err(),
        "company_merge_conflict_coverage_required"
    );
}

#[test]
fn integrated_output_is_revalidated_and_traced_to_all_child_artifacts() {
    let plan = plan();
    let mut receipt = CompanyMergeReceipt {
        schema: COMPANY_INTEGRATION_SCHEMA.to_owned(),
        receipt_id: "receipt-1".to_owned(),
        integration_id: plan.integration_id.clone(),
        plan_digest: plan.digest.clone(),
        base_revision: 4,
        result_revision: 5,
        child_output_digests: plan.child_output_digests.clone(),
        conflict_decisions: vec![decision()],
        revalidated: true,
        accepted: true,
        pushed_or_published: false,
        evidence_refs: vec!["evidence:merge".to_owned()],
        digest: String::new(),
    };
    receipt.digest = receipt.canonical_digest();
    receipt.validate_against(&plan).expect("receipt");
    assert!(!receipt.pushed_or_published);
}

#[test]
fn result_unknown_or_changed_child_output_never_proves_merge() {
    let plan = plan();
    let mut receipt = CompanyMergeReceipt {
        schema: COMPANY_INTEGRATION_SCHEMA.to_owned(),
        receipt_id: "receipt-unknown".to_owned(),
        integration_id: plan.integration_id.clone(),
        plan_digest: plan.digest.clone(),
        base_revision: 4,
        result_revision: 5,
        child_output_digests: plan.child_output_digests.clone(),
        conflict_decisions: vec![decision()],
        revalidated: false,
        accepted: true,
        pushed_or_published: false,
        evidence_refs: vec!["evidence:merge".to_owned()],
        digest: String::new(),
    };
    receipt.digest = receipt.canonical_digest();
    assert_eq!(
        receipt.validate_against(&plan).unwrap_err(),
        "company_merge_receipt_binding_invalid"
    );
}
