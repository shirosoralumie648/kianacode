//! CO-13 source guards: versioned department work contracts stay non-executing on the single
//! Company aggregate and are admitted only through the ControlPlane transaction.
//!
//! The first guard pins the contract surface: versioned kinds, typed input basis and result
//! contract, plan binding, appointment evidence and the explicit refusal of smuggled grants and
//! leases. The second guard pins the wiring: the admission verdict runs before the state
//! transition, the manual lane compiles the same three targets the automatic shards discover, and
//! the card-named fixtures keep their deny-first order.
#[test]
fn department_packets_are_versioned_and_cannot_grant_runtime_authority() {
    let packets = include_str!("../../kiana-domain/src/department_packets.rs");
    let legacy = include_str!("../../kiana-domain/src/work_packets.rs");
    let core = include_str!("../src/company.rs");

    for marker in [
        "DEPARTMENT_PACKET_SCHEMA",
        "RESULT_CONTRACT_SCHEMA",
        "DEPARTMENT_STAGE_FACTS_SCHEMA",
        "DEPARTMENT_ROLE_APPOINTMENT_SCHEMA",
        "DEPARTMENT_PACKET_ADMISSION_SCHEMA",
        "DepartmentPacketKind",
        "PacketInputBasis",
        "ResultContract",
        "DepartmentPlanBinding",
        "DepartmentStageFacts",
        "DepartmentRoleAppointment",
        "DepartmentPacketAdmission",
        "runtime_grant",
        "budget_lease",
        "department_packet_runtime_authority_forbidden",
        "department_packet_plan_required",
        "department_packet_write_scope_denied",
        "department_packet_role_department_mismatch",
        "department_packet_charter_baseline_required",
        "department_packet_plan_version_mismatch",
        "department_packet_plan_node_unknown",
        "department_packet_appointment_inactive",
        "runtime_authority",
        "packet_digest",
        "admission_digest",
        "packet_role_must_be_builder",
        "input_basis",
        "output_schema",
    ] {
        assert!(
            packets.contains(marker) || legacy.contains(marker) || core.contains(marker),
            "CO-13 marker missing: {marker}"
        );
    }
    assert!(legacy.contains("pub fn validate"));
    assert!(legacy.contains("packet_role_must_be_builder"));
    assert!(!packets.contains("CapabilityBroker"));
    assert!(!packets.contains("EventStorePort"));
    assert!(!core.contains("DepartmentPacket::execute"));
}

/// CO-13 completion slice: the admission is one Company transaction, the daemon spine proves the
/// deny-first boundary creates no related object, and the manual lane stays dispatch-only while
/// the automatic shards discover the same targets.
#[test]
fn department_packet_admission_is_wired_into_the_company_transaction_and_the_ci_lane() {
    let packets = include_str!("../../kiana-domain/src/department_packets.rs");
    let domain = include_str!("../../kiana-domain/src/company.rs");
    let policy = include_str!("../../kiana-domain/src/company_policy.rs");
    let core = include_str!("../src/company.rs");
    let fixture = include_str!("../../kiana-domain/tests/co13_department_packets.rs");
    let spine = include_str!("../../kiana-daemon/tests/co13_department_packet_transactions.rs");
    let workflow = include_str!("../../.github/workflows/co13-department-packets.yml");
    let shards = include_str!("../../scripts/ci/test-shards.json");

    // The command, its event name and the state projection live on the single Company aggregate.
    for marker in [
        "AdmitDepartmentPacket",
        "DepartmentPacketAdmitted",
        "department_packets",
        "department_packet_admission_verdict",
        "admit_department_packet",
        "department_stage_facts",
        "department_role_appointment",
        "DepartmentStageFacts",
        "DepartmentRoleAppointment",
        "DepartmentPacketAdmission",
    ] {
        assert!(
            domain.contains(marker),
            "CO-13 company marker missing: {marker}"
        );
    }
    assert!(
        policy.contains("CompanyCommand::AdmitDepartmentPacket { .. } => &[\"pm\", \"sponsor\"]")
    );
    assert!(policy.contains("CompanyMutationClass::Project => &[\"sponsor\", \"pm\", \"closer\"]"));
    assert!(policy.contains("\"department_packet_project_terminal\""));
    assert!(policy.contains("\"department_packet_appointment_required\""));
    assert!(policy.contains("\"department_packet_already_admitted\""));

    // The ControlPlane revalidates the contract, the server-derived stage facts and the admitted
    // appointment before the transition, so a forged packet changes no related object.
    let gate = core
        .find("state.department_packet_admission_verdict(packet, company_now())")
        .expect("the department packet admission gate must exist in the handler");
    let transition = core
        .find("state.transition(&request.command")
        .expect("the Company transition must stay in ControlPlane");
    assert!(
        gate < transition,
        "the department packet verdict must run before the transition"
    );
    assert!(!core.contains("DepartmentPacket::admit"));
    assert!(!core.contains("department_stage_facts(packet"));

    // The rejection codes are a closed, stable set shared by the domain contract and the Company
    // aggregate boundary.
    for code in [
        "department_packet_schema_or_version_invalid",
        "department_packet_field_required_or_too_large",
        "department_packet_input_refs_required",
        "result_contract_invalid",
        "result_contract_reference_invalid",
        "department_packet_output_schema_mismatch",
        "department_packet_runtime_authority_forbidden",
        "department_packet_role_unknown",
        "department_packet_role_department_mismatch",
        "department_packet_input_basis_invalid",
        "department_packet_plan_required",
        "department_packet_plan_binding_invalid",
        "department_packet_plan_reference_mismatch",
        "department_packet_charter_baseline_required",
        "department_packet_write_scope_required",
        "department_packet_write_scope_invalid",
        "department_packet_write_scope_duplicate",
        "department_packet_write_scope_denied",
        "department_packet_stage_project_mismatch",
        "department_packet_appointment_mismatch",
        "department_packet_appointment_inactive",
        "department_packet_stage_not_admitted",
        "department_packet_charter_not_approved",
        "department_packet_charter_baseline_mismatch",
        "department_packet_plan_not_approved",
        "department_packet_plan_version_mismatch",
        "department_packet_plan_baseline_mismatch",
        "department_packet_plan_mismatch",
        "department_packet_plan_node_unknown",
        "department_stage_facts_invalid",
        "department_appointment_invalid",
        "department_appointment_role_unknown",
        "department_appointment_department_mismatch",
        "department_appointment_digest_invalid",
        "department_appointment_digest_mismatch",
        "department_packet_admission_invalid",
        "department_packet_project_terminal",
        "department_packet_appointment_required",
        "department_packet_already_admitted",
    ] {
        assert!(
            packets.contains(code) || domain.contains(code) || policy.contains(code),
            "CO-13 rejection code missing: {code}"
        );
    }
    assert!(!packets.contains("department_packet_project_terminal"));
    assert!(!packets.contains("department_packet_already_admitted"));

    // The card-named fixtures stay deny-first: the refusal fixture is declared before the card
    // success fixture, which stays the final test in the file.
    for fixture_name in [
        "department_packet_cannot_smuggle_builder_write_scope_or_runtime_grants",
        "department_packet_contract_refuses_role_department_and_input_basis_drift",
        "department_packet_admission_refuses_unapproved_stage_and_inactive_appointment",
        "department_packet_admission_refuses_drifted_plan_binding",
        "admitted_department_packet_returns_digest_bound_evidence_without_runtime_authority",
        "analysis_and_implementation_packets_validate_through_versioned_contracts",
    ] {
        assert!(
            fixture.contains(fixture_name),
            "CO-13 domain fixture missing: {fixture_name}"
        );
    }
    let deny_first = fixture
        .find("department_packet_cannot_smuggle_builder_write_scope_or_runtime_grants")
        .expect("deny fixture");
    let card_success = fixture
        .find("analysis_and_implementation_packets_validate_through_versioned_contracts")
        .expect("success fixture");
    assert!(
        deny_first < card_success,
        "the deny-first fixture must precede the card success fixture"
    );
    assert_eq!(
        fixture.rfind("#[test]"),
        fixture
            .find("fn analysis_and_implementation_packets_validate_through_versioned_contracts")
            .map(|position| position.saturating_sub("#[test]\n".len())),
        "the card success fixture must stay the final test"
    );
    assert!(!fixture.contains("#[ignore]"));
    for fixture_name in [
        "co13_department_packet_admission_is_one_control_plane_transaction",
        "co13_department_packets_are_idempotent_and_refuse_a_changed_contract",
    ] {
        assert!(
            spine.contains(fixture_name),
            "CO-13 spine fixture missing: {fixture_name}"
        );
    }
    assert!(!spine.contains("#[ignore]"));
    assert!(!spine.contains("CapabilityBroker"));
    assert!(!spine.contains("KianaHarness::run"));

    // The manual lane stays workflow_dispatch-only, runs three targets serially and compiles them
    // before the automatic shards pick the targets up.
    for target in [
        "cargo test -p kiana-core --test co13_department_packets_guard",
        "cargo test -p kiana-domain --test co13_department_packets",
        "cargo test -p kiana-daemon --test co13_department_packet_transactions",
    ] {
        assert!(
            workflow.contains(target),
            "CO-13 lane target missing: {target}"
        );
    }
    for marker in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "--test-threads=1",
        "--no-run",
        "permissions:\n  contents: read",
    ] {
        assert!(
            workflow.contains(marker),
            "CO-13 lane step missing: {marker}"
        );
    }
    for forbidden in [
        "push:",
        "pull_request:",
        "continue-on-error",
        "--ignored",
        "|| true",
    ] {
        assert!(
            !workflow.contains(forbidden),
            "CO-13 lane must not contain: {forbidden}"
        );
    }

    let shards: serde_json::Value =
        serde_json::from_str(shards).expect("the shard ledger must stay valid JSON");
    for (shard, target) in [
        ("kiana-domain-s2/4", "co13_department_packets"),
        ("kiana-core-s2/6", "co13_department_packets_guard"),
    ] {
        assert!(
            shards[shard]["targets"]
                .as_array()
                .is_some_and(|targets| targets.iter().any(|entry| entry.as_str() == Some(target))),
            "CO-13 target {target} must stay in {shard}"
        );
    }
    assert!(
        shards["kiana-daemon"]["targets"].is_null(),
        "the daemon spine is discovered by the whole-crate shard"
    );
}
