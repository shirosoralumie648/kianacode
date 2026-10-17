//! CP-26 behavioral evidence chains: one allow, one hard denial and one Unknown.
//!
//! The card's deliverable asks for a complete evidence-chain example per outcome. These fixtures
//! fold committed `RuntimeEvent`s through the server-owned audit reducer and projection, then
//! assert that each derived row still answers *who / what action / which authority||rule version /
//! why / which attempt / whether the effect was confirmed* (CP-26 Step 1) with the correlation and
//! causation links intact (Step 2) and no recoverable secret text (Step 3). The read-only
//! explanation path is the audit projection/query surface; this file never issues a permit,
//! consumes an approval or invokes a broker.

use kiana_core::rebuild_audit_projection;
use kiana_domain::{AuditActionKind, AuditDecision, AuditRecord, RequestId, RuntimeEvent};
use serde_json::{json, Value};

const RUN_ID: &str = "11111111-1111-4111-8111-111111111111";
const COMMAND_ID: &str = "22222222-2222-4222-8222-222222222222";
const CAPABILITY_ID: &str = "33333333-3333-4333-8333-333333333333";
const RECOVERY_ID: &str = "44444444-4444-4444-8444-444444444444";
const INPUT_DIGEST: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SECRET_SENTINEL: &str = "cp26-sentinel-secret";

/// Build a committed event with the server-owned epochs and a contiguous source cursor. `extra`
/// merges diagnostic payload fields (targets, correlation, reason codes) on top of the envelope.
fn event(sequence: u64, cursor: u64, kind: &str, extra: Value) -> RuntimeEvent {
    let mut data = json!({"authority_epoch": 7, "data_epoch": 3, "source_cursor": cursor});
    if let Some(fields) = extra.as_object() {
        for (key, value) in fields {
            data[key.as_str()] = value.clone();
        }
    }
    RuntimeEvent::new(RequestId::new(), sequence, kind, data).unwrap()
}

/// Fold events into projected audit rows. Any reducer/projection rejection fails the test.
fn reduce(events: &[RuntimeEvent]) -> Vec<AuditRecord> {
    let snapshot = rebuild_audit_projection(events, 1).unwrap();
    snapshot.records
}

/// Every effect must resolve back to the authority fact that produced it (CP-26 Step 1/2).
fn assert_authority_links(events: &[RuntimeEvent], records: &[AuditRecord]) {
    assert_eq!(records.len(), events.len());
    for (event, record) in events.iter().zip(records) {
        assert!(record.validate().is_ok());
        assert_eq!(record.source_event_ids, vec![event.event_id]);
        assert_eq!(record.request_id, Some(event.request_id));
        assert_eq!(record.authority_epoch, 7);
        assert_eq!(record.data_epoch, 3);
        assert_eq!(record.actor_ref, "server:control-plane");
        let digest = record.action_digest.as_deref().unwrap_or_default();
        assert!(digest.starts_with("sha256:"));
    }
}

#[test]
fn cp26_success_chain_links_each_effect_to_its_authority() {
    let events = vec![
        event(
            1,
            1,
            "run.authorized",
            json!({
                "run_id": RUN_ID,
                "command_id": COMMAND_ID,
                "correlation_ref": "corr-success",
                "input_digest": INPUT_DIGEST,
                "reason_code": "policy_allow",
            }),
        ),
        event(
            2,
            2,
            "run.capability_requested",
            json!({
                "target_ref": CAPABILITY_ID,
                "command_id": COMMAND_ID,
                "correlation_ref": "corr-success",
                "causation_ref": RUN_ID,
            }),
        ),
        event(
            3,
            3,
            "capability.decision",
            json!({
                "target_ref": CAPABILITY_ID,
                "command_id": COMMAND_ID,
                "decision": "allowed",
                "correlation_ref": "corr-success",
                "causation_ref": RUN_ID,
                "reason_code": "policy_allow",
            }),
        ),
        event(
            4,
            4,
            "capability.completed",
            json!({
                "target_ref": CAPABILITY_ID,
                "command_id": COMMAND_ID,
                "correlation_ref": "corr-success",
                "causation_ref": RUN_ID,
                "reason_code": "effect_confirmed",
            }),
        ),
    ];

    let records = reduce(&events);
    assert_authority_links(&events, &records);

    assert_eq!(records[0].action_kind, AuditActionKind::Authorization);
    assert_eq!(records[0].decision, AuditDecision::Accepted);
    assert_eq!(records[1].action_kind, AuditActionKind::Capability);
    assert_eq!(records[1].decision, AuditDecision::Staged);
    assert_eq!(records[2].action_kind, AuditActionKind::Capability);
    assert_eq!(records[2].decision, AuditDecision::Accepted);
    assert_eq!(records[3].action_kind, AuditActionKind::Capability);
    assert_eq!(records[3].decision, AuditDecision::Consumed);

    for record in &records {
        assert_eq!(record.correlation_ref.as_deref(), Some("corr-success"));
    }
    assert_eq!(records[0].input_digest.as_deref(), Some(INPUT_DIGEST));
    assert_eq!(records[0].reason_code.as_deref(), Some("policy_allow"));
    assert_eq!(records[2].reason_code.as_deref(), Some("policy_allow"));
    assert_eq!(records[3].reason_code.as_deref(), Some("effect_confirmed"));
    for record in &records[1..] {
        assert_eq!(record.command_id, RequestId::parse_str(COMMAND_ID));
    }
}

#[test]
fn cp26_hard_denial_chain_never_records_a_confirmed_effect() {
    let events = vec![
        event(
            1,
            1,
            "authorization.denied",
            json!({
                "target_ref": RUN_ID,
                "command_id": COMMAND_ID,
                "correlation_ref": "corr-deny",
                "reason_code": "policy_denied_capability",
            }),
        ),
        event(
            2,
            2,
            "capability.blocked",
            json!({
                "target_ref": CAPABILITY_ID,
                "correlation_ref": "corr-deny",
                "reason_code": "policy_denied_capability",
            }),
        ),
    ];

    let records = reduce(&events);
    assert_authority_links(&events, &records);

    assert_eq!(records[0].action_kind, AuditActionKind::Authorization);
    assert_eq!(records[1].action_kind, AuditActionKind::Capability);
    for record in &records {
        assert_eq!(record.decision, AuditDecision::Denied);
        assert_eq!(record.reason_code.as_deref(), Some("policy_denied_capability"));
    }
}

#[test]
fn cp26_unknown_chain_stays_unknown_until_reconciled() {
    let events = vec![
        event(
            1,
            1,
            "capability.result_unknown",
            json!({
                "target_ref": CAPABILITY_ID,
                "command_id": COMMAND_ID,
                "correlation_ref": "corr-unknown",
                "reason_code": "handler_result_unknown",
            }),
        ),
        event(
            2,
            2,
            "recovery.requested",
            json!({
                "target_ref": RECOVERY_ID,
                "correlation_ref": "corr-unknown",
                "causation_ref": CAPABILITY_ID,
            }),
        ),
        event(
            3,
            3,
            "recovery.reconciled",
            json!({
                "target_ref": RECOVERY_ID,
                "correlation_ref": "corr-unknown",
                "causation_ref": CAPABILITY_ID,
                "reason_code": "reconciled_from_evidence",
            }),
        ),
    ];

    let records = reduce(&events);
    assert_authority_links(&events, &records);

    assert_eq!(records[0].action_kind, AuditActionKind::Capability);
    assert_eq!(records[0].decision, AuditDecision::Unknown);
    assert_eq!(records[0].reason_code.as_deref(), Some("handler_result_unknown"));
    assert_eq!(records[1].action_kind, AuditActionKind::Recovery);
    assert_eq!(records[1].decision, AuditDecision::Staged);
    assert_eq!(records[2].action_kind, AuditActionKind::Recovery);
    assert_eq!(records[2].decision, AuditDecision::Consumed);
    for record in &records {
        if record.action_kind == AuditActionKind::Capability {
            assert_eq!(record.decision, AuditDecision::Unknown);
        }
    }
}

#[test]
fn cp26_self_submitted_audit_row_is_rejected_before_persist() {
    // A payload cannot submit an `audit.*` fact, nor smuggle a pre-baked record.
    let forged_reason = format!("denied api_key={}", SECRET_SENTINEL);
    let forged = event(
        1,
        1,
        "audit.approved",
        json!({"target_ref": CAPABILITY_ID, "reason_code": forged_reason}),
    );
    assert!(rebuild_audit_projection(&[forged], 1).is_err());

    let smuggled = event(
        1,
        1,
        "capability.decision",
        json!({
            "target_ref": CAPABILITY_ID,
            "decision": "allowed",
            "audit_record": { "decision": "accepted" },
        }),
    );
    assert!(rebuild_audit_projection(&[smuggled], 1).is_err());
}

#[test]
fn cp26_secret_never_appears_in_projected_explain_row() {
    let denied_reason = format!("denied api_key={}", SECRET_SENTINEL);
    let events = vec![event(
        1,
        1,
        "capability.blocked",
        json!({
            "target_ref": CAPABILITY_ID,
            "correlation_ref": "corr-deny",
            "reason_code": denied_reason,
            "api_key": SECRET_SENTINEL,
        }),
    )];

    let records = reduce(&events);
    assert_eq!(records.len(), 1);
    let explain = serde_json::to_string(&records[0]).unwrap();

    assert!(!explain.contains(SECRET_SENTINEL));
    assert!(explain.contains("[REDACTED]"));
    assert_eq!(records[0].reason_code.as_deref(), Some("denied api_key=[REDACTED]"));
}
