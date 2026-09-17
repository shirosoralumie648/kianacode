use kiana_domain::{
    CapabilityKind, CapabilityRequest, PermissionProfile, PolicyDecision, RequestContext,
    RequestId, RiskLevel, SecurityPolicyId, SecurityReasonCode,
};
use kiana_policy::{
    BundlePolicyEngine, DecisionTrace, PolicyBundle, PolicyEffect, PolicyOutcome, PolicyRevision,
    PolicyRule, POLICY_BUNDLE_SCHEMA, POLICY_DECISION_TRACE_SCHEMA,
};
use serde_json::json;

fn trusted_context() -> RequestContext {
    let mut context = RequestContext::local("sc05-session", "/repo");
    context.project_trusted = true;
    context.permission_profile = PermissionProfile::Balanced;
    context
}

fn request(operation: &str) -> CapabilityRequest {
    CapabilityRequest::new(
        RequestId::new(),
        CapabilityKind::Query,
        operation,
        json!({"query":"bounded"}),
    )
    .with_risk(RiskLevel::ReadOnly)
}

#[test]
fn policy_bundle_is_versioned_digest_bound_and_default_deny() {
    let rules = vec![
        PolicyRule::new(
            "allow-read",
            20,
            "safe.read",
            Some(CapabilityKind::Query),
            Some(RiskLevel::ReadOnly),
            PolicyEffect::Allow,
            None,
        )
        .unwrap(),
        PolicyRule::new(
            "ask-export",
            10,
            "safe.export",
            Some(CapabilityKind::Query),
            None,
            PolicyEffect::Ask,
            Some(SecurityReasonCode::PolicyApprovalRequired),
        )
        .unwrap(),
    ];
    let bundle = PolicyBundle::new(SecurityPolicyId::new(), 4, 7, rules).unwrap();
    assert_eq!(bundle.schema, POLICY_BUNDLE_SCHEMA);
    assert_eq!(bundle.default_effect, PolicyEffect::Deny);
    assert!(bundle.validate().is_ok());
    let revision = bundle.revision_snapshot().unwrap();
    assert!(revision.validate().is_ok());
    assert_eq!(
        PolicyRevision::from_json(&revision.to_json().unwrap()).unwrap(),
        revision
    );
    assert_eq!(
        PolicyBundle::from_json(&bundle.to_json().unwrap()).unwrap(),
        bundle
    );
    assert!(bundle
        .clone()
        .with_default_effect(PolicyEffect::Allow)
        .is_err());
}

#[test]
fn policy_evaluator_is_deny_first_and_trace_is_replayable() {
    let rules = vec![
        PolicyRule::new(
            "allow-query",
            50,
            "safe.read",
            Some(CapabilityKind::Query),
            None,
            PolicyEffect::Allow,
            None,
        )
        .unwrap(),
        PolicyRule::new(
            "deny-query",
            90,
            "safe.read",
            None,
            None,
            PolicyEffect::Deny,
            Some(SecurityReasonCode::PolicyGrantWidening),
        )
        .unwrap(),
        PolicyRule::new(
            "ask-export",
            1,
            "safe.export",
            None,
            None,
            PolicyEffect::Ask,
            Some(SecurityReasonCode::PolicyApprovalRequired),
        )
        .unwrap(),
    ];
    let bundle = PolicyBundle::new(SecurityPolicyId::new(), 2, 3, rules).unwrap();
    let context = trusted_context();

    let denied = bundle.evaluate(&context, &request("safe.read")).unwrap();
    assert!(matches!(denied.decision, PolicyDecision::Deny { .. }));
    assert_eq!(denied.trace.outcome, PolicyOutcome::Deny);
    assert_eq!(
        denied.trace.reason,
        Some(SecurityReasonCode::PolicyGrantWidening)
    );
    assert_eq!(denied.trace.schema, POLICY_DECISION_TRACE_SCHEMA);
    assert_eq!(
        DecisionTrace::from_json(&denied.trace.to_json().unwrap()).unwrap(),
        denied.trace
    );

    let asking = bundle.evaluate(&context, &request("safe.export")).unwrap();
    assert!(matches!(asking.decision, PolicyDecision::Ask { .. }));
    assert_eq!(asking.trace.outcome, PolicyOutcome::Ask);

    let unknown = bundle.evaluate(&context, &request("unregistered")).unwrap();
    assert!(matches!(unknown.decision, PolicyDecision::Deny { .. }));
    assert_eq!(
        unknown.trace.reason,
        Some(SecurityReasonCode::PolicyOperationUnregistered)
    );
}

#[test]
fn stale_snapshot_and_invalid_policy_never_fall_back_to_allow() {
    let rule = PolicyRule::new(
        "allow-read",
        1,
        "safe.read",
        Some(CapabilityKind::Query),
        None,
        PolicyEffect::Allow,
        None,
    )
    .unwrap();
    let bundle = PolicyBundle::new(SecurityPolicyId::new(), 1, 4, vec![rule]).unwrap();
    let context = trusted_context();
    let stale = bundle
        .evaluate_with_snapshot(&context, &request("safe.read"), 5, &bundle.policy_digest)
        .unwrap();
    assert!(matches!(stale.decision, PolicyDecision::Deny { .. }));
    assert_eq!(
        stale.trace.reason,
        Some(SecurityReasonCode::PolicyAuthorityEpochStale)
    );

    let stale_digest = bundle
        .evaluate_with_snapshot(
            &context,
            &request("safe.read"),
            4,
            "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        )
        .unwrap();
    assert!(matches!(stale_digest.decision, PolicyDecision::Deny { .. }));
    assert_eq!(
        stale_digest.trace.reason,
        Some(SecurityReasonCode::PolicyRevisionStale)
    );

    let mut unknown_major = bundle.to_json().unwrap();
    unknown_major["version"]["major"] = json!(2);
    assert!(PolicyBundle::from_json(&unknown_major).is_err());
    let engine = BundlePolicyEngine::new(bundle).unwrap();
    assert!(matches!(
        engine.evaluate(&context, &request("safe.read")),
        PolicyDecision::Allow { .. }
    ));
}

#[test]
fn policy_rules_reject_ambiguity_and_unknown_fields() {
    let deny = PolicyRule::new(
        "deny",
        1,
        "safe.read",
        None,
        None,
        PolicyEffect::Deny,
        Some(SecurityReasonCode::PolicyScopeEmpty),
    )
    .unwrap();
    let duplicate = PolicyRule::new(
        "deny-two",
        1,
        "safe.read",
        None,
        None,
        PolicyEffect::Deny,
        Some(SecurityReasonCode::PolicyScopeEmpty),
    )
    .unwrap();
    assert_eq!(
        PolicyBundle::new(SecurityPolicyId::new(), 1, 1, vec![deny, duplicate]).unwrap_err(),
        "policy_rule_duplicate_selector"
    );

    let invalid_allow = PolicyRule::new(
        "allow-with-reason",
        1,
        "safe.read",
        None,
        None,
        PolicyEffect::Allow,
        Some(SecurityReasonCode::PolicyApprovalRequired),
    );
    assert_eq!(invalid_allow.unwrap_err(), "policy_allow_reason_unexpected");

    let mut unknown = json!({
        "schema":"kiana.policy-bundle.v1",
        "version":{"major":1,"minor":0},
        "policy_id": SecurityPolicyId::new(),
        "revision":1,
        "authority_epoch":1,
        "default_effect":"deny",
        "rules":[],
        "policy_digest":"sha256:0000000000000000000000000000000000000000000000000000000000000000",
        "bundle_digest":"sha256:0000000000000000000000000000000000000000000000000000000000000000",
        "unexpected":true,
    });
    assert!(PolicyBundle::from_json(&unknown).is_err());
    unknown["unexpected"] = json!(false);
}
