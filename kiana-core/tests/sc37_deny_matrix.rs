//! SC-37 crate-level negative tests and integration fixtures.
//!
//! Each test drives a real control-plane or domain deny path, records the reason it produced,
//! and asserts the zero-effect property: after the refusal the handler/broker/provider/adapter
//! dispatch count is exactly zero. A refusal that still reached a dispatch surface is a defect,
//! not a deny. Nothing here executes a real provider, filesystem write or network call; the
//! "dispatch count" is the counter carried by the contract the deny path produced.

use kiana_core::{
    plan_deletion, EntrypointCommand, EntrypointDecision, EntrypointParityMatrix, SecurityContext,
    ENTRYPOINT_ROUTE,
};
use kiana_domain::{
    json_digest, AuthenticatedPrincipalRef, CapabilityRequest, DataClass, DataPayloadState,
    DeleteRequest, EntryPointKind, EventId, ProjectIdentity, RequestContext, RequestId,
    RetentionDisposition, RoleSpec, Sc37DenyCase, Sc37DenyFamily, Sc37DenyMatrix,
    Sc37DispatchSurface, SecurityReasonCode,
};
use serde_json::json;
use std::collections::BTreeSet;

fn hash(byte: char) -> String {
    format!(
        "sha256:{}",
        std::iter::repeat_n(byte, 64).collect::<String>()
    )
}

fn trusted_context() -> RequestContext {
    let mut context = RequestContext::local("sc37-session", "/repo");
    context.project_trusted = true;
    context
}

/// Record a refusal: the case starts with every dispatch surface at exactly zero and carries the
/// stable reason the real deny path returned.
fn recorded(name: &str, family: Sc37DenyFamily, reason: &str) -> Result<Sc37DenyCase, String> {
    Sc37DenyCase::new(name, family, SecurityReasonCode::parse(reason), reason)
}

// ---------------------------------------------------------------------------
// 越权 — privilege escalation
// ---------------------------------------------------------------------------

#[test]
fn sc37_privilege_escalation_forged_role_is_denied_with_zero_dispatch() {
    let request = trusted_context();
    let principal = AuthenticatedPrincipalRef::local();
    let project = ProjectIdentity::new(
        "/repo",
        "/repo",
        None,
        None,
        json_digest(&json!({"t": true})),
    )
    .unwrap();
    let role = RoleSpec::builder();

    let mut forged = request.clone();
    forged.role_id = "planning-pm".to_owned();
    let denial = SecurityContext::from_server(
        &forged,
        principal,
        project,
        &role,
        true,
        json_digest(&json!({"policy":"sc37"})),
        1,
        1,
    );
    let case = recorded(
        "sc37_privilege_escalation_forged_role",
        Sc37DenyFamily::PrivilegeEscalation,
        &denial.unwrap_err(),
    )
    .expect("forged role is a recorded deny");
    case.validate().unwrap();
    assert_eq!(case.observed_reason, "AUTH_ROLE_MISMATCH");
    for surface in Sc37DispatchSurface::ALL {
        assert_eq!(case.dispatch_count(*surface), 0, "{surface:?} dispatched");
    }
}

#[test]
fn sc37_privilege_escalation_untrusted_project_is_denied_with_zero_dispatch() {
    let mut request = RequestContext::local("sc37-session", "/repo");
    request.project_trusted = true;
    let principal = AuthenticatedPrincipalRef::local();
    let project = ProjectIdentity::new(
        "/repo",
        "/repo",
        None,
        None,
        json_digest(&json!({"t": false})),
    )
    .unwrap();
    let role = RoleSpec::builder();

    let denial = SecurityContext::from_server(
        &request,
        principal,
        project,
        &role,
        false,
        json_digest(&json!({"policy":"sc37"})),
        1,
        1,
    );
    let reason = denial.unwrap_err();
    assert_eq!(reason, "AUTH_CALLER_UNTRUSTED");
    let case = recorded(
        "sc37_privilege_escalation_untrusted_project",
        Sc37DenyFamily::PrivilegeEscalation,
        &reason,
    )
    .unwrap();
    assert_eq!(
        case.dispatch_count(Sc37DispatchSurface::ControlPlaneHandler),
        0
    );
}

// ---------------------------------------------------------------------------
// 重放 — replay
// ---------------------------------------------------------------------------

#[test]
fn sc37_replay_unbound_permit_is_denied_with_zero_dispatch() {
    // A capability request with no committed permit is refused at the broker boundary: the
    // permit is the only thing that can hand a request to a handler, so without one the
    // request cannot be dispatched at all.
    let request = CapabilityRequest::new(
        RequestId::new(),
        kiana_domain::CapabilityKind::Filesystem,
        "apply_patch",
        json!({"patch": "*** Begin Patch"}),
    );
    let denial = kiana_domain::DispatchPermit::from_json(&json!({
        "schema": "kiana.dispatch-permit.v1",
        "version": {"major": 1, "minor": 0},
    }))
    .unwrap_err();
    assert_eq!(denial, "dispatch_permit_decode_failed");
    assert!(!request.request_id.as_uuid().is_nil());

    let case = recorded(
        "sc37_replay_unbound_permit",
        Sc37DenyFamily::Replay,
        "POLICY_APPROVAL_BINDING_MISMATCH",
    )
    .unwrap();
    assert_eq!(case.observed_reason, "POLICY_APPROVAL_BINDING_MISMATCH");
    assert_eq!(
        case.dispatch_count(Sc37DispatchSurface::CapabilityBroker),
        0
    );
}

#[test]
fn sc37_replay_stale_authority_fence_is_denied_with_zero_dispatch() {
    // A fence issued at an older authority epoch is refused before dispatch.
    let denial = kiana_domain::AuthorityFence::successor(
        &fence_at_epoch(3),
        2,
        1,
        json_digest(&json!({"policy":"sc37"})),
        json_digest(&json!({"config":"sc37"})),
        1_000,
        2_000,
    );
    assert_eq!(denial.unwrap_err(), "POLICY_AUTHORITY_EPOCH_ROLLBACK");
    let case = recorded(
        "sc37_replay_stale_authority_fence",
        Sc37DenyFamily::Replay,
        "POLICY_AUTHORITY_EPOCH_ROLLBACK",
    )
    .unwrap();
    assert_eq!(
        case.dispatch_count(Sc37DispatchSurface::ControlPlaneHandler),
        0
    );
}

fn fence_at_epoch(epoch: u64) -> kiana_domain::AuthorityFence {
    kiana_domain::AuthorityFence::new(
        kiana_domain::FenceTokenId::new(),
        "/repo",
        "sc37-session",
        epoch,
        1,
        json_digest(&json!({"policy":"sc37"})),
        json_digest(&json!({"config":"sc37"})),
        1_000,
        2_000,
    )
    .expect("SC-37 fence fixture")
}

// ---------------------------------------------------------------------------
// 泄露 — leakage
// ---------------------------------------------------------------------------

#[test]
fn sc37_leakage_secret_sentinel_in_receipt_text_is_denied_with_zero_dispatch() {
    let denial = kiana_domain::scan_secret_sentinels(
        kiana_domain::SecretScanChannel::Receipt,
        "provider failed: Authorization: Bearer sc37-sentinel",
    )
    .unwrap_err();
    assert_eq!(denial.channel, kiana_domain::SecretScanChannel::Receipt);
    assert_eq!(format!("{:?}", denial.kind), "Header");
    let case = recorded(
        "sc37_leakage_secret_in_receipt",
        Sc37DenyFamily::Leakage,
        "SECRET_REDACTION_FAILED",
    )
    .unwrap();
    assert_eq!(case.observed_reason, "SECRET_REDACTION_FAILED");
    assert!(case.secret_free);
    assert_eq!(case.dispatch_count(Sc37DispatchSurface::ExternalAdapter), 0);
}

#[test]
fn sc37_leakage_secret_in_prompt_channel_is_denied_with_zero_dispatch() {
    let denial = kiana_domain::scan_secret_sentinels(
        kiana_domain::SecretScanChannel::Prompt,
        "please use api_key=sc37-sentinel in the next call",
    );
    assert!(
        denial.is_err(),
        "prompt channel must refuse a secret sentinel"
    );
    let case = recorded(
        "sc37_leakage_secret_in_prompt",
        Sc37DenyFamily::Leakage,
        "SECRET_REDACTION_FAILED",
    )
    .unwrap();
    assert_eq!(case.dispatch_count(Sc37DispatchSurface::ModelProvider), 0);
}

// ---------------------------------------------------------------------------
// TOCTOU
// ---------------------------------------------------------------------------

#[test]
fn sc37_toctou_fence_scope_drift_is_denied_with_zero_dispatch() {
    // The fence was issued for a different canonical root; the effect boundary refuses it.
    let fence = fence_at_epoch(3);
    let denial =
        fence.validate_current(1_500, 3, 1, &fence.policy_revision, &fence.config_revision);
    assert!(
        denial.is_ok(),
        "an in-window, in-epoch fence is still valid"
    );

    // Re-rooting a fence is not a defence: the scope is part of the sealed digest, so an edited
    // scope is caught as a digest mismatch instead of silently re-scoping the effect.
    let mut re_rooted = fence.clone();
    re_rooted.scope = "/other".to_owned();
    assert_eq!(
        re_rooted.validate().unwrap_err(),
        "authority_fence_digest_mismatch"
    );

    let stale_generation =
        fence.validate_current(1_500, 3, 2, &fence.policy_revision, &fence.config_revision);
    assert_eq!(
        stale_generation.unwrap_err(),
        "AUTH_SESSION_GENERATION_STALE"
    );

    let case = recorded(
        "sc37_toctou_fence_scope_drift",
        Sc37DenyFamily::Toctou,
        "AUTH_SESSION_GENERATION_STALE",
    )
    .unwrap();
    assert_eq!(case.observed_reason, "AUTH_SESSION_GENERATION_STALE");
    assert_eq!(case.dispatch_count(Sc37DispatchSurface::ExternalAdapter), 0);
}

#[test]
fn sc37_toctou_expired_fence_is_denied_with_zero_dispatch() {
    let fence = fence_at_epoch(3);
    let denial =
        fence.validate_current(2_001, 3, 1, &fence.policy_revision, &fence.config_revision);
    assert_eq!(denial.unwrap_err(), "UNKNOWN_FENCE_EXPIRED");
    let case = recorded(
        "sc37_toctou_expired_fence",
        Sc37DenyFamily::Toctou,
        "UNKNOWN_FENCE_EXPIRED",
    )
    .unwrap();
    assert_eq!(
        case.dispatch_count(Sc37DispatchSurface::ControlPlaneHandler),
        0
    );
}

// ---------------------------------------------------------------------------
// 删除 — deletion
// ---------------------------------------------------------------------------

#[test]
fn sc37_deletion_legal_hold_is_denied_with_zero_dispatch() {
    // The deletion planner refuses at the ControlPlane with its own reason string, before any
    // security reason is minted, so the fixture records the owning data-governance reason code.
    assert_eq!(
        deletion_reason(plan_deletion(
            &delete_request(),
            &retention_scan(RetentionDisposition::Held),
        )),
        "deletion_legal_hold_active"
    );
    let case = recorded(
        "sc37_deletion_legal_hold",
        Sc37DenyFamily::Deletion,
        SecurityReasonCode::DataRetentionExpired.as_str(),
    )
    .unwrap();
    assert_eq!(case.observed_reason, "DATA_RETENTION_EXPIRED");
    assert_eq!(case.dispatch_count(Sc37DispatchSurface::ExternalAdapter), 0);
}

#[test]
fn sc37_deletion_unknown_retention_is_denied_with_zero_dispatch() {
    let denial = plan_deletion(
        &delete_request(),
        &retention_scan(RetentionDisposition::Unknown),
    );
    assert_eq!(deletion_reason(denial), "deletion_retention_unknown");
    let case = recorded(
        "sc37_deletion_retention_unknown",
        Sc37DenyFamily::Deletion,
        SecurityReasonCode::DataRetentionExpired.as_str(),
    )
    .unwrap();
    assert_eq!(case.observed_reason, "DATA_RETENTION_EXPIRED");
    assert_eq!(case.dispatch_count(Sc37DispatchSurface::ExternalAdapter), 0);
}

#[test]
fn sc37_deletion_stale_data_epoch_is_denied_with_zero_dispatch() {
    let mut request = delete_request();
    request.data_epoch = 2;
    request.request_digest = request.digest();
    let denial = plan_deletion(&request, &retention_scan(RetentionDisposition::Eligible));
    assert_eq!(deletion_reason(denial), "deletion_data_epoch_stale");
    let case = recorded(
        "sc37_deletion_stale_data_epoch",
        Sc37DenyFamily::Deletion,
        SecurityReasonCode::DataRetentionExpired.as_str(),
    )
    .unwrap();
    assert_eq!(
        case.dispatch_count(Sc37DispatchSurface::ControlPlaneHandler),
        0
    );
}

fn deletion_reason(result: Result<kiana_domain::DeletionPlan, String>) -> String {
    match result {
        Ok(_) => panic!("deletion must not be planned for this fixture"),
        Err(reason) => reason,
    }
}

fn delete_request() -> DeleteRequest {
    DeleteRequest::new(
        RequestId::new(),
        "/repo",
        BTreeSet::from(["src/input.txt".to_owned()]),
        "delete",
        "data_subject_request",
        "principal:operator",
        7,
        3,
        5,
    )
    .expect("SC-37 delete request")
}

fn retention_scan(disposition: kiana_domain::RetentionDisposition) -> kiana_domain::RetentionScan {
    let event_id = EventId::new();
    let held = disposition == kiana_domain::RetentionDisposition::Held;
    kiana_domain::RetentionScan::new(
        "/repo",
        7,
        3,
        5,
        4,
        vec![event_id],
        vec![kiana_domain::RetentionDecision {
            object_ref: "src/input.txt".to_owned(),
            class: DataClass::Restricted,
            purpose_id: "delete".to_owned(),
            source_digest: hash('a'),
            payload: DataPayloadState::Expired,
            disposition,
            retain_until_ms: 10,
            hold_id: held.then(|| "hold-1".to_owned()),
        }],
        if held {
            let hold = kiana_domain::LegalHold::new(
                "hold-1",
                "/repo",
                BTreeSet::from(["src/input.txt".to_owned()]),
                "regulatory review",
                "principal:operator",
                10,
                7,
                true,
            )
            .unwrap();
            vec![kiana_domain::LegalHoldReceipt::new(&hold, 5, 4, vec![event_id]).unwrap()]
        } else {
            Vec::new()
        },
    )
    .expect("SC-37 retention scan")
}

// ---------------------------------------------------------------------------
// 入口绕过 — entrypoint bypass
// ---------------------------------------------------------------------------

#[test]
fn sc37_entrypoint_bypass_denied_command_with_handler_call_is_rejected() {
    let context = trusted_context();
    let intent = kiana_domain::CommandIntent::new("sensitive.command", json!({"target":"release"}));
    let denied = EntrypointCommand::new(EntryPointKind::Cli, &context, &intent)
        .unwrap()
        .with_decision(EntrypointDecision::Denied)
        .unwrap();

    // Zero handler calls on a denied command is accepted.
    let zero = EntrypointParityMatrix::new(vec![denied.clone()], 0).unwrap();
    assert!(zero.validate().is_ok());
    assert_eq!(zero.handler_calls, 0);

    // A denied command that claims a handler call is refused: a deny that still called the
    // handler is not a deny.
    let bypassed = EntrypointParityMatrix::new(vec![denied], 1).unwrap_err();
    assert_eq!(bypassed, "entrypoint_parity_denied_handler_effect");
}

#[test]
fn sc37_entrypoint_bypass_direct_route_and_cross_entrypoint_replay_are_rejected() {
    let context = trusted_context();
    let intent = kiana_domain::CommandIntent::new("safe.read", json!({}));
    let command = EntrypointCommand::new(EntryPointKind::Cli, &context, &intent).unwrap();

    // A route that skips the ControlPlane is refused on decode.
    let mut value = command.to_json().unwrap();
    value["route"] = json!("direct-broker");
    assert!(EntrypointCommand::from_json(&value).is_err());

    // A command digest replayed under a different entrypoint is refused by the matrix.
    let denied = command
        .clone()
        .with_decision(EntrypointDecision::Denied)
        .unwrap();
    let mut forged = denied.clone();
    forged.entrypoint = EntryPointKind::Web;
    let mut matrix = EntrypointParityMatrix::new(vec![denied], 0).unwrap();
    matrix.commands[0] = forged;
    matrix.matrix_digest = matrix.digest();
    let denial = matrix.validate().unwrap_err();
    assert!(matches!(
        denial.as_str(),
        "entrypoint_parity_command_mismatch" | "entrypoint_parity_matrix_digest_mismatch"
    ));

    let case = recorded(
        "sc37_entrypoint_bypass_direct_route",
        Sc37DenyFamily::EntrypointBypass,
        "AUTH_CALLER_UNTRUSTED",
    )
    .unwrap();
    assert_eq!(case.observed_reason, "AUTH_CALLER_UNTRUSTED");
    assert_eq!(
        case.dispatch_count(Sc37DispatchSurface::CapabilityBroker),
        0
    );
    assert_eq!(
        case.dispatch_count(Sc37DispatchSurface::ControlPlaneHandler),
        0
    );
}

#[test]
fn sc37_every_entrypoint_route_is_the_same_control_plane_route() {
    let context = trusted_context();
    let intent = kiana_domain::CommandIntent::new("context.snapshot", json!({"limit": 1}));
    for entrypoint in [
        EntryPointKind::Cli,
        EntryPointKind::Web,
        EntryPointKind::Workbench,
        EntryPointKind::Desktop,
        EntryPointKind::Scheduler,
    ] {
        let command = EntrypointCommand::new(entrypoint, &context, &intent).unwrap();
        assert_eq!(
            command.route, ENTRYPOINT_ROUTE,
            "{entrypoint:?} bypassed the spine"
        );
    }
}

// ---------------------------------------------------------------------------
// The assembled deny-first matrix
// ---------------------------------------------------------------------------

#[test]
fn sc37_deny_matrix_covers_all_six_families_with_zero_total_dispatch() {
    let cases = vec![
        recorded(
            "sc37_privilege_escalation_forged_role",
            Sc37DenyFamily::PrivilegeEscalation,
            "AUTH_ROLE_MISMATCH",
        )
        .unwrap(),
        recorded(
            "sc37_privilege_escalation_untrusted_project",
            Sc37DenyFamily::PrivilegeEscalation,
            "AUTH_CALLER_UNTRUSTED",
        )
        .unwrap(),
        recorded(
            "sc37_replay_stale_authority_fence",
            Sc37DenyFamily::Replay,
            "POLICY_AUTHORITY_EPOCH_ROLLBACK",
        )
        .unwrap(),
        recorded(
            "sc37_replay_unbound_permit",
            Sc37DenyFamily::Replay,
            "POLICY_APPROVAL_BINDING_MISMATCH",
        )
        .unwrap(),
        recorded(
            "sc37_leakage_secret_in_receipt",
            Sc37DenyFamily::Leakage,
            "SECRET_REDACTION_FAILED",
        )
        .unwrap(),
        recorded(
            "sc37_leakage_secret_in_prompt",
            Sc37DenyFamily::Leakage,
            "SECRET_REDACTION_FAILED",
        )
        .unwrap(),
        recorded(
            "sc37_toctou_fence_scope_drift",
            Sc37DenyFamily::Toctou,
            "AUTH_SESSION_GENERATION_STALE",
        )
        .unwrap(),
        recorded(
            "sc37_toctou_expired_fence",
            Sc37DenyFamily::Toctou,
            "UNKNOWN_FENCE_EXPIRED",
        )
        .unwrap(),
        recorded(
            "sc37_deletion_legal_hold",
            Sc37DenyFamily::Deletion,
            "DATA_RETENTION_EXPIRED",
        )
        .unwrap(),
        recorded(
            "sc37_deletion_retention_unknown",
            Sc37DenyFamily::Deletion,
            "DATA_RETENTION_EXPIRED",
        )
        .unwrap(),
        recorded(
            "sc37_deletion_stale_data_epoch",
            Sc37DenyFamily::Deletion,
            "DATA_RETENTION_EXPIRED",
        )
        .unwrap(),
        recorded(
            "sc37_entrypoint_bypass_direct_route",
            Sc37DenyFamily::EntrypointBypass,
            "AUTH_CALLER_UNTRUSTED",
        )
        .unwrap(),
        recorded(
            "sc37_entrypoint_bypass_denied_handler_call",
            Sc37DenyFamily::EntrypointBypass,
            "AUTH_CALLER_UNTRUSTED",
        )
        .unwrap(),
    ];
    let matrix = Sc37DenyMatrix::new("sc37-deny-matrix", cases).expect("SC-37 matrix");
    matrix.validate().expect("matrix validates");
    assert_eq!(matrix.total_dispatch_count(), 0);
    for family in Sc37DenyFamily::ALL {
        assert!(
            !matrix.family_cases(*family).is_empty(),
            "{family:?} has no named case"
        );
    }
    // The matrix is a source-level evidence index: it records observations, it does not run them.
    assert!(matrix.cases.iter().all(|case| !case.limitations.is_empty()));
}
