use kiana_domain::{
    AgentTemplate, BudgetLease, CapabilityGrant, CapabilityGrantId, CapabilityKind, RoleSpec,
    SupervisionLease, CAPABILITY_GRANT_SCHEMA, SUPERVISION_LEASE_SCHEMA,
};
use serde_json::json;

#[test]
fn child_grant_cannot_exceed_parent_grant() {
    let parent = CapabilityGrant {
        schema: CAPABILITY_GRANT_SCHEMA.to_owned(),
        grant_id: CapabilityGrantId::new(),
        capability: CapabilityKind::Filesystem,
        operation: "apply_patch".to_owned(),
        resources: vec!["workspace".to_owned()],
        paths: vec!["src".to_owned()],
        expires_at_unix_ms: 2_000,
        approval_id: None,
        delegation_allowed: true,
    };
    let child = CapabilityGrant {
        schema: CAPABILITY_GRANT_SCHEMA.to_owned(),
        grant_id: CapabilityGrantId::new(),
        capability: CapabilityKind::Filesystem,
        operation: "apply_patch".to_owned(),
        resources: vec!["workspace".to_owned()],
        paths: vec!["src/lib.rs".to_owned()],
        expires_at_unix_ms: 1_000,
        approval_id: None,
        delegation_allowed: false,
    };
    assert!(parent.validate().is_ok());
    assert!(child.validate().is_ok());
    assert!(parent.contains(&child));

    let mut widened = child.clone();
    widened.paths = vec!["src".to_owned(), "secrets".to_owned()];
    assert!(!parent.contains(&widened));
    widened.paths = child.paths.clone();
    widened.expires_at_unix_ms = parent.expires_at_unix_ms + 1;
    assert!(!parent.contains(&widened));
    widened.expires_at_unix_ms = child.expires_at_unix_ms;
    widened.delegation_allowed = true;
    assert!(!parent.clone().contains(&widened));
}

#[test]
fn templates_pin_version_and_default_to_non_delegable() {
    let template = AgentTemplate::for_role(&RoleSpec::builder(), "1.0.0");
    assert!(template.validate().is_ok());
    assert!(!template.template_id.to_string().is_empty());
    assert!(!template.delegation_allowed);

    let mut encoded = serde_json::to_value(&template).unwrap();
    encoded["unexpected"] = json!(true);
    assert!(serde_json::from_value::<AgentTemplate>(encoded).is_err());

    let budget = BudgetLease::new(2, 100, 1_000, 1, 1);
    let supervision = SupervisionLease {
        schema: SUPERVISION_LEASE_SCHEMA.to_owned(),
        lease_id: kiana_domain::SupervisionLeaseId::new(),
        heartbeat_interval_seconds: 5,
        stall_threshold_seconds: 30,
        retry_limit: 0,
        retries_used: 0,
    };
    assert!(budget.validate().is_ok());
    assert!(supervision.validate().is_ok());
}
