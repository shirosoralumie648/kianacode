use kiana_core::{
    ApprovalBinding, ApprovalConsumptionReceipt, ApproverScope, HumanInboxItem, HumanInboxStatus,
    APPROVAL_BINDING_SCHEMA,
};
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

fn binding_for(request: &CapabilityRequest, context: &RequestContext) -> ApprovalBinding {
    ApprovalBinding::for_request(
        kiana_domain::ApprovalId::new(),
        context,
        ProjectId::new(),
        request,
        json_digest(&json!({"target":"src/lib.rs"})),
        None,
        4,
        json_digest(&json!({"policy":"sc10"})),
        1_000,
    )
    .unwrap()
}

fn binding() -> ApprovalBinding {
    let request = request();
    let context = context();
    binding_for(&request, &context)
}

fn approver_scope(binding: &ApprovalBinding) -> ApproverScope {
    ApproverScope::new(
        approver(),
        vec![binding.project_id],
        vec![binding.command_kind.clone()],
        binding.authority_epoch,
        2_000,
    )
    .unwrap()
}

#[test]
fn approval_binding_is_exact_digest_scope_and_strict() {
    let request = request();
    let context = context();
    let binding = binding_for(&request, &context);
    assert_eq!(binding.schema, APPROVAL_BINDING_SCHEMA);
    assert!(binding.validate().is_ok());
    let encoded = binding.to_json().unwrap();
    assert_eq!(ApprovalBinding::from_json(&encoded).unwrap(), binding);
    assert!(binding
        .validate_request(
            &context,
            binding.project_id,
            &request,
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
fn approval_binding_rejects_request_context_scope_drift() {
    let request = request();
    let original = context();
    let binding = binding_for(&request, &original);
    let mut changed = original;
    changed.path_allow.push("src/other.rs".to_owned());

    assert_eq!(
        binding
            .validate_request(
                &changed,
                binding.project_id,
                &request,
                &binding.target_digest,
                binding.authority_epoch,
                &binding.policy_revision,
                100,
            )
            .unwrap_err(),
        SecurityReasonCode::PolicyApprovalBindingMismatch.as_str()
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
    let consumed = approved.consume(100).unwrap();
    assert_eq!(consumed.status, HumanInboxStatus::Consumed);
    assert!(consumed.consume(100).is_err());
    assert!(consumed
        .decide(approver(), ApprovalDecision::Deny, 100)
        .is_err());
}

#[test]
fn human_inbox_rejects_consumption_after_approval_expiry() {
    let approved = HumanInboxItem::new(binding())
        .unwrap()
        .decide(approver(), ApprovalDecision::Approve, 100)
        .unwrap();

    assert_eq!(
        approved.consume(1_000).unwrap_err(),
        SecurityReasonCode::PolicyApprovalExpired.as_str()
    );
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

#[test]
fn approval_binding_rejects_arguments_only_drift() {
    let request = request();
    let context = context();
    let binding = binding_for(&request, &context);
    // Same tool name and same RequestId: only the arguments differ, so approving the tool
    // name must not authorize the changed payload.
    let mut changed = request.clone();
    changed.arguments["patch"] = json!("*** Update File: src/lib.rs\n+malicious");
    assert_eq!(
        binding
            .validate_request(
                &context,
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
}

#[test]
fn approver_scope_authorizes_matching_binding_and_rejects_drift() {
    let binding = binding();
    let scope = approver_scope(&binding);
    assert!(scope.validate().is_ok());
    assert!(scope.authorize(&binding, 100).is_ok());

    let encoded = scope.to_json().unwrap();
    assert_eq!(ApproverScope::from_json(&encoded).unwrap(), scope);
    let mut unknown = encoded;
    unknown["raw_role"] = json!("admin");
    assert!(ApproverScope::from_json(&unknown).is_err());

    let wrong_project = ApproverScope::new(
        approver(),
        vec![ProjectId::new()],
        vec![binding.command_kind.clone()],
        binding.authority_epoch,
        2_000,
    )
    .unwrap();
    assert_eq!(
        wrong_project.authorize(&binding, 100).unwrap_err(),
        SecurityReasonCode::AuthProjectMismatch.as_str()
    );

    let wrong_command = ApproverScope::new(
        approver(),
        vec![binding.project_id],
        vec!["other.command".to_owned()],
        binding.authority_epoch,
        2_000,
    )
    .unwrap();
    assert_eq!(
        wrong_command.authorize(&binding, 100).unwrap_err(),
        SecurityReasonCode::AuthRoleMismatch.as_str()
    );

    let stale_epoch = ApproverScope::new(
        approver(),
        vec![binding.project_id],
        vec![binding.command_kind.clone()],
        binding.authority_epoch + 1,
        2_000,
    )
    .unwrap();
    assert_eq!(
        stale_epoch.authorize(&binding, 100).unwrap_err(),
        SecurityReasonCode::PolicyAuthorityEpochStale.as_str()
    );

    let self_scope = ApproverScope::new(
        binding.principal.clone(),
        vec![binding.project_id],
        vec![binding.command_kind.clone()],
        binding.authority_epoch,
        2_000,
    )
    .unwrap();
    assert_eq!(
        self_scope.authorize(&binding, 100).unwrap_err(),
        SecurityReasonCode::PolicySelfApprovalForbidden.as_str()
    );

    assert_eq!(
        scope.authorize(&binding, 2_000).unwrap_err(),
        SecurityReasonCode::PolicyApprovalExpired.as_str()
    );
}

#[test]
fn approver_scope_is_strict_and_canonical() {
    let scope = approver_scope(&binding());

    let mut duplicated = scope.to_json().unwrap();
    let project = duplicated["project_ids"][0].clone();
    duplicated["project_ids"] = json!([project.clone(), project]);
    assert!(ApproverScope::from_json(&duplicated).is_err());

    let mut empty_commands = scope.to_json().unwrap();
    empty_commands["command_kinds"] = json!([]);
    assert!(ApproverScope::from_json(&empty_commands).is_err());

    let mut tampered = scope.to_json().unwrap();
    tampered["authority_epoch"] = json!(scope.authority_epoch + 7);
    assert!(ApproverScope::from_json(&tampered).is_err());
}

#[test]
fn human_inbox_decide_with_scope_enforces_approver_scope() {
    let binding = binding();
    let item = HumanInboxItem::new(binding.clone()).unwrap();
    let scope = approver_scope(&binding);
    let decision = ApprovalDecision::Approve;
    let approved = item.decide_with_scope(&scope, decision, 100).unwrap();
    assert_eq!(approved.status, HumanInboxStatus::Approved);
    assert_eq!(
        approved.decided_by.as_ref().unwrap().principal_id,
        "reviewer-principal"
    );

    let wrong_command = ApproverScope::new(
        approver(),
        vec![binding.project_id],
        vec!["other.command".to_owned()],
        binding.authority_epoch,
        2_000,
    )
    .unwrap();
    assert_eq!(
        item.decide_with_scope(&wrong_command, ApprovalDecision::Approve, 100)
            .unwrap_err(),
        SecurityReasonCode::AuthRoleMismatch.as_str()
    );

    let wrong_project = ApproverScope::new(
        approver(),
        vec![ProjectId::new()],
        vec![binding.command_kind.clone()],
        binding.authority_epoch,
        2_000,
    )
    .unwrap();
    assert_eq!(
        item.decide_with_scope(&wrong_project, ApprovalDecision::Approve, 100)
            .unwrap_err(),
        SecurityReasonCode::AuthProjectMismatch.as_str()
    );

    let self_scope = ApproverScope::new(
        binding.principal.clone(),
        vec![binding.project_id],
        vec![binding.command_kind.clone()],
        binding.authority_epoch,
        2_000,
    )
    .unwrap();
    assert_eq!(
        item.decide_with_scope(&self_scope, ApprovalDecision::Approve, 100)
            .unwrap_err(),
        SecurityReasonCode::PolicySelfApprovalForbidden.as_str()
    );

    assert_eq!(
        item.decide_with_scope(&scope, ApprovalDecision::Approve, 5_000)
            .unwrap_err(),
        SecurityReasonCode::PolicyApprovalExpired.as_str()
    );
}

#[test]
fn human_inbox_consume_emits_one_shot_receipt() {
    let binding = binding();
    let approved = HumanInboxItem::new(binding.clone())
        .unwrap()
        .decide(approver(), ApprovalDecision::Approve, 100)
        .unwrap();

    assert_eq!(
        ApprovalConsumptionReceipt::for_consumed(&approved, 200).unwrap_err(),
        "approval_receipt_not_consumed"
    );

    let (consumed, receipt) = approved.consume_with_receipt(200).unwrap();
    assert_eq!(consumed.status, HumanInboxStatus::Consumed);
    assert_eq!(receipt.approval_id, binding.approval_id);
    assert_eq!(receipt.subject_request_id, binding.subject_request_id);
    assert_eq!(receipt.binding_digest, binding.binding_digest);
    assert_eq!(receipt.scope_digest, binding.scope_digest);
    assert_eq!(receipt.consumed_at_unix_ms, 200);
    assert!(receipt.validate().is_ok());

    let encoded = receipt.to_json().unwrap();
    assert_eq!(
        ApprovalConsumptionReceipt::from_json(&encoded).unwrap(),
        receipt
    );

    // One-shot: the consumed item cannot be consumed a second time.
    assert!(consumed.consume_with_receipt(200).is_err());

    // A receipt cannot be forged by editing a digest-bound field.
    let mut tampered = encoded.clone();
    tampered["consumed_at_unix_ms"] = json!(999);
    assert!(ApprovalConsumptionReceipt::from_json(&tampered).is_err());

    let mut unknown = encoded;
    unknown["raw_prompt"] = json!("do not echo");
    assert!(ApprovalConsumptionReceipt::from_json(&unknown).is_err());
}
