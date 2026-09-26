use kiana_domain::*;

fn hash(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn workflow(id: &str) -> AutomationWorkflowView {
    let mut value = AutomationWorkflowView {
        workflow_id: id.to_owned(),
        status: AutomationWorkStatus::Due,
        reason: "waiting for scheduled occurrence".to_owned(),
        evidence_refs: vec!["event:workflow".to_owned()],
        due_at_unix_ms: Some(100),
        digest: String::new(),
    };
    value.digest = value.canonical_digest();
    value
}

fn snapshot() -> AutomationSnapshot {
    let wf = workflow("workflow-1");
    let mut trigger = AutomationTriggerView {
        trigger_id: "trigger-1".to_owned(),
        workflow_id: wf.workflow_id.clone(),
        source: AutomationTriggerSource::Manual,
        occurrence_key: "occurrence-1".to_owned(),
        status: AutomationWorkStatus::Due,
        reason: "manual trigger due".to_owned(),
        evidence_refs: vec!["event:trigger".to_owned()],
        digest: String::new(),
    };
    trigger.digest = trigger.canonical_digest();
    let mut receipt = AutomationReceiptView {
        receipt_id: "receipt-1".to_owned(),
        workflow_id: wf.workflow_id.clone(),
        runtime_status: AutomationWorkStatus::Completed,
        business_outcome_confirmed: false,
        runtime_evidence_refs: vec!["event:run-completed".to_owned()],
        business_evidence_refs: Vec::new(),
        limitations: vec!["business outcome is not inferred".to_owned()],
        digest: String::new(),
    };
    receipt.digest = receipt.canonical_digest();
    let mut incident = AutomationIncidentView {
        incident_id: "incident-1".to_owned(),
        workflow_id: wf.workflow_id.clone(),
        unknown: true,
        reconcile_required: true,
        reason: "provider result unknown".to_owned(),
        evidence_refs: vec!["event:unknown".to_owned()],
        digest: String::new(),
    };
    incident.digest = incident.canonical_digest();
    let mut value = AutomationSnapshot {
        schema: AUT22_SNAPSHOT_SCHEMA.to_owned(),
        snapshot_id: "snapshot-1".to_owned(),
        source_cursor: 10,
        projection_version: 2,
        authority_epoch: 7,
        workflows: vec![wf],
        triggers: vec![trigger],
        receipts: vec![receipt],
        incidents: vec![incident],
        limitations: vec!["read-only projection".to_owned()],
        digest: String::new(),
    };
    value.digest = value.canonical_digest();
    value
}

#[test]
fn automation_snapshot_is_read_only_and_keeps_runtime_and_business_outcome_separate() {
    let value = snapshot();
    value.validate().expect("snapshot");
    assert!(!value.receipts[0].business_outcome_confirmed);
    assert_eq!(value.incidents[0].unknown, true);
}

#[test]
fn unknown_incident_without_reconcile_or_cross_scope_receipt_is_rejected() {
    let mut unknown = snapshot();
    unknown.incidents[0].reconcile_required = false;
    unknown.incidents[0].digest = unknown.incidents[0].canonical_digest();
    unknown.digest = unknown.canonical_digest();
    assert_eq!(
        unknown.validate().unwrap_err(),
        "aut22_unknown_incident_reconcile_missing"
    );

    let mut cross_scope = snapshot();
    cross_scope.receipts[0].workflow_id = "foreign-workflow".to_owned();
    cross_scope.receipts[0].digest = cross_scope.receipts[0].canonical_digest();
    cross_scope.digest = cross_scope.canonical_digest();
    assert_eq!(
        cross_scope.validate().unwrap_err(),
        "aut22_receipt_binding_invalid"
    );
}

#[test]
fn due_trigger_and_business_success_need_evidence() {
    let mut due = snapshot();
    due.workflows[0].due_at_unix_ms = None;
    due.workflows[0].digest = due.workflows[0].canonical_digest();
    due.digest = due.canonical_digest();
    assert_eq!(
        due.validate().unwrap_err(),
        "aut22_due_workflow_deadline_missing"
    );

    let mut business = snapshot();
    business.receipts[0].business_outcome_confirmed = true;
    business.receipts[0].digest = business.receipts[0].canonical_digest();
    business.digest = business.canonical_digest();
    assert_eq!(
        business.validate().unwrap_err(),
        "aut22_business_outcome_evidence_missing"
    );
    assert_eq!(hash('a').len(), 71);
}
