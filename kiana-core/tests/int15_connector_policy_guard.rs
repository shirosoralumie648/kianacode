//! INT-15 source and wiring guard: the connector R0--R4 mapping must stay on the single
//! ControlPlane policy/gate/approval spine, deny before any dispatch and keep its fixtures wired to
//! CI.  `include_str!` assertions are source-level evidence only; they are not runtime receipts.

const DOMAIN_SOURCE: &str = include_str!("../../kiana-domain/src/connector_policy.rs");
const POLICY_SOURCE: &str = include_str!("../../kiana-policy/src/lib.rs");
const GATES_SOURCE: &str = include_str!("../../kiana-gates/src/lib.rs");
const CORE_SOURCE: &str = include_str!("../src/connectors.rs");
const CAPABILITIES_SOURCE: &str = include_str!("../src/capabilities.rs");
const APPROVALS_SOURCE: &str = include_str!("../src/approvals.rs");
const DAEMON_SOURCE: &str = include_str!("../../kiana-daemon/src/connectors.rs");
const DOMAIN_FIXTURES: &str = include_str!("../../kiana-domain/tests/int15_connector_policy.rs");
const POLICY_FIXTURES: &str = include_str!("../../kiana-policy/tests/int15_connector_policy.rs");
const DAEMON_FIXTURES: &str = include_str!("../../kiana-daemon/tests/int15_connector_policy.rs");
const MANUAL_LANE: &str = include_str!("../../.github/workflows/int15-connector-policy.yml");
const SHARDS: &str = include_str!("../../scripts/ci/test-shards.json");
const BASELINE: &str = include_str!("../../docs/roadmap/int15-connector-policy-baseline.md");

#[test]
fn connector_risk_policy_gate_approval_stays_on_control_plane_path() {
    for marker in [
        "ConnectorOperationRisk",
        "R0ReadOnly",
        "R1ReadOnly",
        "R2DataGrant",
        "R3OnceApproval",
        "R4DefaultDeny",
        "ConnectorDataGrant",
        "ConnectorOnceApproval",
        "evaluate_connector_admission",
        "connector_policy_decision",
        "connector_r4_default_denied",
        "connector_final_payload_digest",
        "connector_binding_revoked",
        "connector_policy_epoch_mismatch",
        "connector_authority_epoch_mismatch",
        "connector_final_payload_approval_required",
        "connector_fixture_hash_valid",
        "final_payload_digest",
        "authorize_and_execute",
        "stage_capability_action",
        "decide_approval_with_proof",
    ] {
        assert!(
            DOMAIN_SOURCE.contains(marker)
                || POLICY_SOURCE.contains(marker)
                || GATES_SOURCE.contains(marker)
                || CORE_SOURCE.contains(marker)
                || APPROVALS_SOURCE.contains(marker),
            "missing INT-15 marker: {marker}"
        );
    }
    assert!(CORE_SOURCE.contains("self.authorize_and_execute(&context, request).await"));
    assert!(CORE_SOURCE.contains("connector_payload_sha256(payload)"));
    assert!(DOMAIN_SOURCE.contains("crate::connector_fixture_hash_valid(&self.payload_digest)"));
    assert!(DOMAIN_SOURCE.contains("!valid_digest(&self.approval_digest)"));
    assert!(POLICY_SOURCE.contains("evaluate_connector_admission"));
    assert!(POLICY_SOURCE.contains("DefaultPolicyEngine"));
    assert!(APPROVALS_SOURCE.contains("stage_capability_action"));
    assert!(APPROVALS_SOURCE.contains("decide_approval_with_proof"));
    assert!(!CORE_SOURCE.contains("CapabilityBroker"));
    assert!(!POLICY_SOURCE.contains("CapabilityBroker"));
}

#[test]
fn connector_admission_mapping_is_declared_risk_bound_and_deny_first() {
    // The mapping derives risk from the server-owned operation contract and can only be raised by
    // the declared contract risk.
    assert!(DOMAIN_SOURCE.contains("pub struct ConnectorRiskMapping"));
    assert!(DOMAIN_SOURCE.contains("pub fn connector_risk_mapping"));
    assert!(DOMAIN_SOURCE.contains("connector_operation_risk_with_declared"));
    assert!(DOMAIN_SOURCE.contains("mapping.required_risk_level"));

    // R0/R1 allow, R2 grants, R3 once-approval, R4 default deny: every class has a requirement.
    for marker in [
        "ConnectorPolicyRequirement::ReadOnly",
        "ConnectorPolicyRequirement::DataGrant",
        "ConnectorPolicyRequirement::OnceApproval",
        "ConnectorPolicyRequirement::DefaultDeny",
        "connector_r4_default_denied",
        "connector_final_payload_required",
        "connector_final_payload_digest_mismatch",
        "connector_data_grant_required",
        "connector_data_grant_revoked",
        "connector_data_grant_expired",
        "connector_data_grant_scope_insufficient",
        "connector_once_approval_consumed",
        "connector_once_approval_payload_digest_mismatch",
        "connector_binding_scope_mismatch",
        "connector_binding_id_mismatch",
        "broker_calls: 0",
        "is_zero_effect",
    ] {
        assert!(
            DOMAIN_SOURCE.contains(marker) || POLICY_SOURCE.contains(marker),
            "missing INT-15 deny marker: {marker}"
        );
    }

    // The final payload digest is server-derived from the canonical payload; a caller supplied
    // wire digest can only be rejected as a mismatch.
    assert!(POLICY_SOURCE.contains("payload.map(kiana_domain::connector_payload_sha256)"));
    assert!(POLICY_SOURCE.contains("connector_final_payload_digest_mismatch"));

    // Server-owned narrowing objects are dropped from every prepared action, so a caller cannot
    // stand in for the ControlPlane approval gate or a server-resolved data grant.
    assert!(CAPABILITIES_SOURCE.contains("\"data_grant\""));
    assert!(CAPABILITIES_SOURCE.contains("\"connector_approval\""));

    // The daemon dispatch handler still re-checks the server-derived risk before any adapter call.
    assert!(DAEMON_SOURCE.contains("connector_final_payload_approval_required"));
    assert!(DAEMON_SOURCE.contains("kiana_domain::RiskLevel::ExternalSideEffect"));
    assert!(!DAEMON_SOURCE.contains("reqwest::Client"));
    assert!(!DAEMON_SOURCE.contains("ProviderGateway"));
}

#[test]
fn connector_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shards() {
    for fixture in [
        "r0_and_r1_are_read_only_and_stable",
        "r2_requires_a_current_data_grant_and_never_widens_scope",
        "r3_is_one_shot_and_binds_the_final_payload_digest",
        "r3_payload_digest_accepts_bare_and_prefixed_sha256_but_rejects_malformed_values",
        "r4_is_default_deny_even_with_critical_risk_or_approval",
        "declared_risk_can_only_raise_the_operation_class",
        "stale_authority_and_policy_epochs_are_zero_broker_calls",
    ] {
        assert!(
            DOMAIN_FIXTURES.contains(fixture),
            "missing INT-15 domain fixture: {fixture}"
        );
    }
    for fixture in [
        "r0_r1_are_allow_and_r3_is_ask_on_the_existing_policy_engine",
        "r4_is_default_deny_and_never_becomes_an_approval_gate",
        "r2_requires_a_current_data_grant_and_denies_the_wire_shortcut",
        "r3_final_payload_digest_is_derived_and_the_payload_cannot_be_swapped",
        "unregistered_and_missing_binding_material_fail_closed",
        "policy_engine_and_connector_decision_agree_on_every_operation_class",
    ] {
        assert!(
            POLICY_FIXTURES.contains(fixture),
            "missing INT-15 policy fixture: {fixture}"
        );
    }
    for fixture in [
        "read_only_risk_is_allowed_and_denials_never_dispatch",
        "final_payload_approval_is_one_shot_and_payload_bound",
    ] {
        assert!(
            DAEMON_FIXTURES.contains(fixture),
            "missing INT-15 daemon fixture: {fixture}"
        );
    }
    for fixtures in [DOMAIN_FIXTURES, POLICY_FIXTURES, DAEMON_FIXTURES] {
        assert!(!fixtures.contains("#[ignore"));
        assert!(!fixtures.contains("ignore ="));
    }

    for step in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-domain --test int15_connector_policy --locked -- --test-threads=1",
        "cargo test -p kiana-policy --test int15_connector_policy --locked -- --test-threads=1",
        "cargo test -p kiana-daemon --test int15_connector_policy --locked -- --test-threads=1",
        "cargo test -p kiana-core --test int15_connector_policy_guard --locked -- --test-threads=1",
    ] {
        assert!(
            MANUAL_LANE.contains(step),
            "missing INT-15 lane step: {step}"
        );
    }
    assert_eq!(MANUAL_LANE.matches("--no-run").count(), 4);
    assert!(MANUAL_LANE.contains("workflow_dispatch:"));
    assert!(MANUAL_LANE.contains("permissions:\n  contents: read"));
    for forbidden in [
        "push:",
        "pull_request:",
        "schedule:",
        "continue-on-error",
        "|| true",
    ] {
        assert!(
            !MANUAL_LANE.contains(forbidden),
            "INT-15 lane must not contain {forbidden}"
        );
    }

    // Automatic coverage stays on the unified ci.yml shards already registered for this Step.
    assert!(SHARDS.contains("\"int15_connector_policy\""));
    assert!(SHARDS.contains("\"int15_connector_policy_guard\""));
    assert!(SHARDS.contains("\"kiana-policy\""));
    assert!(BASELINE.contains("proof_level: source"));
}
