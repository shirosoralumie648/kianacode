#[test]
fn sc25_policy_keeps_trust_resolution_pure_and_deny_first() {
    let source = include_str!("../src/project_trust.rs");
    for marker in [
        "ProjectTrustScope",
        "precedence_order",
        "priority",
        "ProjectTrustPlan",
        "ProjectTrustLoadAudit",
        "authorize_loading",
        "load_audit",
        "project_trust_root_missing",
        "project_trust_untrusted",
        "project_trust_unknown",
        "project_trust_conflict",
        "project_trust_root_limit",
        "project_trust_plan_requested_scope_mismatch",
        "PROJECT_TRUST_PLAN_SCHEMA",
        "PROJECT_TRUST_LOAD_AUDIT_SCHEMA",
        "audit_ref",
        "resolution_digest",
    ] {
        assert!(source.contains(marker), "missing SC-25 marker: {marker}");
    }
    assert!(!source.contains("std::fs"));
    assert!(!source.contains("read_to_string"));
    assert!(!source.contains("load_all_skills"));
}
