use kiana_core::{ApprovalBinding, HumanInboxItem, HumanInboxStatus, APPROVAL_BINDING_SCHEMA};
use kiana_domain::{
    json_digest, ApprovalDecision, AuthenticatedPrincipalRef, CapabilityKind, CapabilityRequest,
    ProjectId, RequestContext, RequestId, RiskLevel, SecurityReasonCode,
};
use serde_json::json;

fn context() -> RequestContext {
    let mut context = RequestContext::local("sc10-session", "/repo");
    context.project_trusted = true;
    context.permission_profile = kiana_domain::PermissionProfile::Balanced;
    context
}

fn request() -> CapabilityRequest {
    CapabilityRequest::new(
        RequestId::new(),
        CapabilityKind::Filesystem,
        "apply_patch",
        json!({"path":"src/lib.rs","patch":"*** Update File: src/lib.rs"}),
    )
    .with_risk(RiskLevel::LocalWrite)
}

fn approver() -> AuthenticatedPrincipalRef {
    let mut principal = AuthenticatedPrincipalRef::local();
    principal.principal_id = "reviewer-principal".to_owned();
    principal.principal_digest = principal.digest();
    principal
}

fn binding() -> ApprovalBinding {
    let request = request();
    let context = context();
    ApprovalBinding::for_request(
        kiana_domain::ApprovalId::new(),
        &context,
        ProjectId::new(),
        &request,
        json_digest(&json!({"target":"src/lib.rs"})),
        None,
        4,
        json_digest(&json!({"policy":"sc10"})),
        1_000,
    )
    .unwrap()
}

#[test]
fn approval_binding_is_exact_digest_scope_and_strict() {
    let binding = binding();
    assert_eq!(binding.schema, APPROVAL_BINDING_SCHEMA);
    assert!(binding.validate().is_ok());
    let encoded = binding.to_json().unwrap();
    assert_eq!(ApprovalBinding::from_json(&encoded).unwrap(), binding);
    assert!(binding
        .validate_request(
            &context(),
            binding.project_id,
            &request(),
            &binding.target_digest,
            binding.authority_epoch,
            &binding.policy_revision,
            100,
        )
        .is_ok());

    let mut target = encoded.clone();
    target["target_digest"] = json!(json_digest(&json!({"target":"other"})));
    assert!(ApprovalBinding::from_json(&target).is_err());
    let mut unknown = encoded;
    unknown["arguments"] = json!({"secret":"must-not-serialize"});
    assert!(ApprovalBinding::from_json(&unknown).is_err());
}

#[test]
fn approval_binding_rejects_payload_scope_epoch_and_expiry_drift() {
    let binding = binding();
    let mut changed = request();
    changed.arguments["path"] = json!("src/other.rs");
    assert_eq!(
        binding
            .validate_request(
                &context(),
                binding.project_id,
                &changed,
                &binding.target_digest,
                binding.authority_epoch,
                &binding.policy_revision,
                100,
            )
            .unwrap_err(),
        SecurityReasonCode::PolicyApprovalBindingMismatch.as_str()
    );
    assert_eq!(
        binding
            .validate_request(
                &context(),
                binding.project_id,
                &request(),
                &binding.target_digest,
                binding.authority_epoch + 1,
                &binding.policy_revision,
                100,
            )
            .unwrap_err(),
        SecurityReasonCode::PolicyApprovalBindingMismatch.as_str()
    );
    assert_eq!(
        binding
            .validate_request(
                &context(),
                binding.project_id,
                &request(),
                &binding.target_digest,
                binding.authority_epoch,
                &binding.policy_revision,
                1_000,
            )
            .unwrap_err(),
        SecurityReasonCode::PolicyApprovalExpired.as_str()
    );
}

#[test]
fn human_inbox_rejects_self_approval_and_consumes_once() {
    let item = HumanInboxItem::new(binding()).unwrap();
    assert_eq!(item.status, HumanInboxStatus::Pending);
    let owner = item.binding.principal.clone();
    assert_eq!(
        item.decide(owner, ApprovalDecision::Approve, 100)
            .unwrap_err(),
        SecurityReasonCode::PolicySelfApprovalForbidden.as_str()
    );
    let approved = item
        .decide(approver(), ApprovalDecision::Approve, 100)
        .unwrap();
    assert_eq!(approved.status, HumanInboxStatus::Approved);
    let consumed = approved.consume().unwrap();
    assert_eq!(consumed.status, HumanInboxStatus::Consumed);
    assert!(consumed.consume().is_err());
    assert!(consumed
        .decide(approver(), ApprovalDecision::Deny, 100)
        .is_err());
}

#[test]
fn human_inbox_denial_and_expiry_are_terminal_and_strict() {
    let denied = HumanInboxItem::new(binding())
        .unwrap()
        .decide(approver(), ApprovalDecision::Deny, 100)
        .unwrap();
    assert_eq!(denied.status, HumanInboxStatus::Denied);
    assert!(denied
        .decide(approver(), ApprovalDecision::Approve, 100)
        .is_err());

    let expired = HumanInboxItem::new(binding());
    assert_eq!(
        expired
            .unwrap()
            .decide(approver(), ApprovalDecision::Approve, 1_000)
            .unwrap_err(),
        SecurityReasonCode::PolicyApprovalExpired.as_str()
    );
    let mut unknown = serde_json::to_value(HumanInboxItem::new(binding()).unwrap()).unwrap();
    unknown["raw_prompt"] = json!("do not echo");
    assert!(HumanInboxItem::from_json(&unknown).is_err());
}
