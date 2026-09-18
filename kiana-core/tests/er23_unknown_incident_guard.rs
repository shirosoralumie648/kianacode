//! ER-23 source guard for Unknown incidents and RecoveryPlan transitions.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "ER-23 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn unknown_effects_project_to_a_stateful_recovery_plan() {
    let domain = include_str!("../../kiana-domain/src/platform.rs");
    let platform = include_str!("../src/platform.rs");
    let commands = include_str!("../src/commands.rs");
    let inbox = include_str!("../../kiana-domain/src/platform.rs");
    let reliability = include_str!("p2_k6_01_reliability.rs");
    let reconcile = include_str!("cp20_unknown_reconcile_guard.rs");

    require(
        domain,
        &[
            "pub enum RecoveryPlanState",
            "Proposed",
            "Approved",
            "Executing",
            "Verified",
            "Failed",
            "Abandoned",
            "pub fn transition(self, next: Self)",
            "transition_with_evidence",
            "safe_actions",
            "forbidden_actions",
            "evidence_refs",
            "automatic_retry_allowed",
        ],
        "RecoveryPlan contract",
    );
    require(
        platform,
        &[
            "failure.incidents",
            "failure.recovery",
            "failure.recovery.transition",
            "project_recovery_plan",
            "advance_recovery_plan",
            "recovery_plan_cannot_self_approve",
            "recovery_plan_evidence_required",
            "failure_incident_not_found",
            "platform_replay",
            "commit_platform",
            "source_event_id",
            "evidence_exists",
            "expected_revision",
            "\"automatic_retry_allowed\": false",
        ],
        "incident projection and transition",
    );
    require(
        commands,
        &["failure.incidents", "failure.recovery", "failure.reconcile"],
        "ControlPlane command routing",
    );
    require(
        inbox,
        &["HumanInboxKind", "Incident", "HumanAction"],
        "Human Inbox action card",
    );
    require(
        reliability,
        &[
            "FailureIncident",
            "RecoveryPlan",
            "requires_reconciliation",
            "reconciliation",
        ],
        "reliability fixture",
    );
    require(
        reconcile,
        &[
            "failure.incidents",
            "failure.reconcile",
            "runtime_outcome_unchanged",
            "automatic_retry_allowed:false",
        ],
        "Unknown reconciliation fixture",
    );
}

#[test]
fn recovery_plan_never_self_approves_or_retries_unknown_effects() {
    let platform = include_str!("../src/platform.rs");
    let dispatch = include_str!("../src/dispatch.rs");
    let capabilities = include_str!("../src/capabilities.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");

    require(
        platform,
        &[
            "requested == RecoveryPlanState::Approved",
            "matches!(context.role_id.as_str(), \"reviewer\" | \"sponsor\")",
            "source_actor == context.actor_id.as_deref()",
            "\"automatic_retry_allowed\": false",
            "state.transition(requested)",
        ],
        "approval/transition fence",
    );
    require(
        dispatch,
        &[
            "CapabilityErrorCode::ResultUnknown",
            "fenced",
            "commit_confirmed",
            "result_unknown:result_delivery_unconfirmed",
        ],
        "effect boundary",
    );
    require(
        capabilities,
        &[
            "FinalizedCapabilityAction::unknown",
            "CapabilityOutcome::Unknown",
        ],
        "capability Unknown",
    );
    require(
        daemon,
        &["RequestBody::Command(command)", "handle_command(context"],
        "single daemon command spine",
    );

    for source in [platform, dispatch, capabilities] {
        for forbidden in [
            "automatic_retry_unknown",
            "retry_unknown_effect",
            "close_unknown_without_evidence",
        ] {
            assert!(
                !source.contains(forbidden),
                "ER-23 recovery bypass marker present: {forbidden}"
            );
        }
    }
}
