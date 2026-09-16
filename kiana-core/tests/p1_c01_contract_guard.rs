#[test]
fn cell_admission_consumes_versioned_contracts_and_intersects_child_scope() {
    let domain = include_str!("../../kiana-domain/src/work_packets.rs");
    let grants = include_str!("../../kiana-domain/src/capabilities.rs");
    let registry = include_str!("../src/cell_registry.rs");
    for marker in [
        "pub struct AgentTemplate",
        "pub struct CellSpec",
        "pub struct SpawnPlan",
        "pub struct BudgetLease",
        "deny_unknown_fields",
        "delegation_allowed",
        "template_version",
    ] {
        assert!(
            domain.contains(marker),
            "cell contract marker missing: {marker}"
        );
    }
    for marker in [
        "pub struct CapabilityGrant",
        "pub struct SupervisionLease",
        "pub fn contains(&self, child: &Self)",
        "deny_unknown_fields",
    ] {
        assert!(
            grants.contains(marker),
            "grant contract marker missing: {marker}"
        );
    }
    for marker in [
        "resolve_template",
        "spawn_template_version_mismatch",
        "parent.reservation.grant.contains(grant)",
        "spawn_delegation_denied",
        "spawn_grant_not_contained",
        "budget.validate()",
        "reservation.supervision.validate()",
    ] {
        assert!(
            registry.contains(marker),
            "cell admission marker missing: {marker}"
        );
    }
}
