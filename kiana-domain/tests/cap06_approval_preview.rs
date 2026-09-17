use kiana_domain::{
    ApprovalChallenge, ApprovalId, ApprovalPlanPreview, CapabilityKind, CapabilityRequest,
    PendingApproval, RequestContext, RequestId, RiskLevel, APPROVAL_CHALLENGE_SCHEMA,
};
use serde_json::json;

fn pending() -> PendingApproval {
    let request_id = RequestId::new();
    let request = CapabilityRequest::new(
        request_id,
        CapabilityKind::Filesystem,
        "apply_patch",
        json!({
            "patch":"*** Begin Patch\n*** Add File: src/new.txt\n+safe\n*** End Patch",
            "sandbox":"workspace-write",
            "path_allow":["src"]
        }),
    )
    .with_risk(RiskLevel::LocalWrite);
    PendingApproval {
        challenge: ApprovalChallenge {
            schema: APPROVAL_CHALLENGE_SCHEMA.to_owned(),
            approval_id: ApprovalId::from_uuid(request_id.as_uuid()),
            request_id,
            request_hash: "sha256:".to_owned() + &"a".repeat(64),
            risk: RiskLevel::LocalWrite,
            expires_at_unix_ms: 2_000,
            reason: "patch approval".to_owned(),
            nonce: "nonce".to_owned(),
            policy_version: "policy-v1".to_owned(),
        },
        request,
    }
}

#[test]
fn approval_preview_binds_final_redacted_plan() {
    let pending = pending();
    let context = RequestContext::local("session-cap06", "/repo");
    let preview = ApprovalPlanPreview::from_pending(&pending, &context, true).unwrap();
    assert_eq!(preview.approval_id, pending.challenge.approval_id);
    assert_eq!(preview.request_id, pending.request.request_id);
    assert_eq!(preview.operation, "apply_patch");
    assert_eq!(preview.risk, RiskLevel::LocalWrite);
    assert!(preview.payload_available);
    assert!(preview.validate().is_ok());
    assert_eq!(
        ApprovalPlanPreview::from_json(&preview.to_json().unwrap()).unwrap(),
        preview
    );
}

#[test]
fn approval_preview_never_becomes_executable_or_accepts_drift() {
    let pending = pending();
    let context = RequestContext::local("session-cap06", "/repo");
    let preview = ApprovalPlanPreview::from_pending(&pending, &context, false).unwrap();
    assert!(!preview.payload_available);
    let mut tampered = preview.to_json().unwrap();
    tampered["preview"]["arguments"]["api_key"] = json!("raw-secret");
    assert_eq!(
        ApprovalPlanPreview::from_json(&tampered).unwrap_err(),
        "approval_plan_preview_invalid"
    );
    let mut unknown = preview.to_json().unwrap();
    unknown["raw_payload"] = json!("secret");
    assert_eq!(
        ApprovalPlanPreview::from_json(&unknown).unwrap_err(),
        "approval_plan_preview_decode_failed"
    );
}
