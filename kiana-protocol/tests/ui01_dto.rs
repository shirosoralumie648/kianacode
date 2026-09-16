use kiana_protocol::*;
use serde_json::json;

fn digest(value: &serde_json::Value) -> String {
    kiana_domain::json_digest(value)
}

fn snapshot() -> UiSnapshotV1 {
    let payload = json!({"command":"approve"});
    UiSnapshotV1 {
        schema: UI_SNAPSHOT_SCHEMA.to_owned(),
        instance_id: "instance-1".to_owned(),
        authority_epoch: 1,
        snapshot_cursor: UiCursorV1 {
            epoch: "instance-1".to_owned(),
            sequence: 1,
        },
        generated_at_unix_ms: 1,
        principal: AuthenticatedPrincipalRef::local(),
        workspace_id: ProjectId::new(),
        capabilities: vec![UiCapability {
            schema: UI_CAPABILITY_SCHEMA.to_owned(),
            capability_id: "run".to_owned(),
            enabled: true,
            actions: vec!["run.turn".to_owned()],
            reason: None,
            scope_digest: digest(&json!({"scope":"run"})),
        }],
        sessions: vec![SessionSummary {
            session_id: SessionId::new("session-1"),
            title: "Review".to_owned(),
            status: "active".to_owned(),
            revision: 1,
        }],
        active_session_id: Some(SessionId::new("session-1")),
        runs: vec![RunSummary {
            run_id: RunId::new(),
            status: ExecutionStatus::Completed,
            revision: 1,
            final_assistant_text: Some("done".to_owned()),
            receipt: Some(ReceiptRef {
                receipt_id: ReceiptId::new(),
                receipt_digest: digest(&json!({"receipt":1})),
            }),
        }],
        pending_actions: vec![HumanActionCard {
            action_id: "action-1".to_owned(),
            command: "approval.decide".to_owned(),
            target_id: "approval-1".to_owned(),
            expected_revision: Some(1),
            expires_at_unix_ms: Some(2),
            allowed_decisions: vec!["approve".to_owned(), "deny".to_owned()],
            payload_digest: digest(&payload),
        }],
        artifacts: vec![ArtifactSummary {
            artifact_id: ArtifactId::new(),
            digest: digest(&json!({"artifact":1})),
            mime: "text/plain".to_owned(),
            size_bytes: 4,
        }],
        notices: vec![UiNotice {
            notice_id: "notice-1".to_owned(),
            severity: UiSeverity::Info,
            title: "Ready".to_owned(),
            summary: "The run is complete".to_owned(),
            action_refs: vec!["action-1".to_owned()],
        }],
        next_page: None,
        limitations: vec![EvidenceLimitation {
            code: "ci_only".to_owned(),
            detail: "Runtime tests execute remotely".to_owned(),
        }],
    }
}

#[test]
fn ui_snapshot_feed_and_action_round_trip_with_unknown_field_guard() {
    let snapshot = snapshot();
    snapshot.validate().unwrap();
    let encoded = serde_json::to_value(&snapshot).unwrap();
    let decoded: UiSnapshotV1 = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(decoded, snapshot);
    let mut forged = encoded;
    forged["unexpected"] = json!(true);
    assert!(serde_json::from_value::<UiSnapshotV1>(forged).is_err());

    let feed = UiFeedEnvelope {
        schema: UI_FEED_SCHEMA.to_owned(),
        instance_id: "instance-1".to_owned(),
        authority_epoch: 1,
        feed_sequence: 2,
        snapshot_cursor: UiCursorV1 {
            epoch: "instance-1".to_owned(),
            sequence: 1,
        },
        event_id: "event-1".to_owned(),
        aggregate_type: Some("run".to_owned()),
        aggregate_id: Some(snapshot.runs[0].run_id.to_string()),
        object_revision: 2,
        event: json!({"kind":"run.finished"}),
    };
    feed.validate().unwrap();

    let payload = json!({"decision":"approve"});
    let action = UiActionV1 {
        schema: UI_ACTION_SCHEMA.to_owned(),
        command_id: RequestId::new(),
        idempotency_key: "ui-action-1".to_owned(),
        target_id: "approval-1".to_owned(),
        expected_epoch: "instance-1".to_owned(),
        expected_cursor: 1,
        expected_revision: Some(1),
        payload_digest: digest(&payload),
        payload,
        submitted_by: "local-user".to_owned(),
        deadline_unix_ms: Some(10),
    };
    action.validate().unwrap();
    let mut bad_action = serde_json::to_value(&action).unwrap();
    bad_action["payload_digest"] = json!(digest(&json!({"different":true})));
    let bad_action: UiActionV1 = serde_json::from_value(bad_action).unwrap();
    assert_eq!(
        bad_action.validate().unwrap_err(),
        "ui_action_payload_digest_mismatch"
    );
}

#[test]
fn ui_action_result_keeps_unknown_and_rejected_distinct() {
    let rejected = UiActionResult {
        schema: UI_ACTION_RESULT_SCHEMA.to_owned(),
        command_id: RequestId::new(),
        disposition: UiActionDisposition::Rejected,
        resulting_cursor: None,
        resulting_revision: None,
        receipt: None,
        error: Some(UiError::new(
            UiErrorCode::PermissionDenied,
            "not allowed",
            UiRetryDisposition::DoNotRetry,
        )),
        retry: UiRetryDisposition::DoNotRetry,
    };
    rejected.validate().unwrap();

    let unknown = UiActionResult {
        schema: UI_ACTION_RESULT_SCHEMA.to_owned(),
        command_id: RequestId::new(),
        disposition: UiActionDisposition::Unknown,
        resulting_cursor: None,
        resulting_revision: None,
        receipt: None,
        error: None,
        retry: UiRetryDisposition::QueryOriginal,
    };
    unknown.validate().unwrap();
    let mut invalid = unknown;
    invalid.retry = UiRetryDisposition::SafeRetry;
    assert_eq!(
        invalid.validate().unwrap_err(),
        "ui_action_unknown_retry_invalid"
    );
}
