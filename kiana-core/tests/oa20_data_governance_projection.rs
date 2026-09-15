use kiana_core::project_data_governance_snapshot;
use kiana_domain::{
    DataClass, DataPayloadState, DataPolicy, EventId, ProcessingGrant, Purpose, Retention,
    RuntimeEvent,
};
use serde_json::json;

fn hash(byte: char) -> String {
    format!(
        "sha256:{}",
        std::iter::repeat(byte).take(64).collect::<String>()
    )
}

fn policy() -> DataPolicy {
    let mut policy = DataPolicy::default();
    policy
        .register(ProcessingGrant {
            id: "grant-1".to_owned(),
            source_path: "src/input.txt".to_owned(),
            content_hash: hash('a'),
            class: DataClass::Restricted,
            purpose: Purpose {
                id: "review".to_owned(),
                description: "security review".to_owned(),
            },
            retention: Retention {
                expires_at_ms: None,
                retain_audit_metadata: true,
            },
            parent_ids: Vec::new(),
            created_by: "principal:operator".to_owned(),
            revoked: false,
        })
        .unwrap();
    policy
}

fn event(sequence: u64, source_cursor: u64, kind: &str, data: serde_json::Value) -> RuntimeEvent {
    let mut event =
        RuntimeEvent::new(kiana_domain::RequestId::new(), sequence, kind, data).unwrap();
    event.data["source_cursor"] = json!(source_cursor);
    event
}

#[test]
fn pending_revocation_fences_payload_and_all_derived_store_projections() {
    let events = vec![event(
        1,
        1,
        "data.revocation_requested",
        json!({"project_root":"/repo","grant_id":"grant-1"}),
    )];
    let snapshot = project_data_governance_snapshot(&policy(), "/repo", &events, 1).unwrap();
    assert!(snapshot
        .observations
        .iter()
        .all(|observation| observation.payload == DataPayloadState::Unknown));
    assert!(snapshot
        .derived_store_states
        .values()
        .all(|state| *state == DataPayloadState::Unknown));
    snapshot.validate().unwrap();
}

#[test]
fn committed_revocation_propagates_revoke_without_erasing_source_event_ids() {
    let events = vec![event(
        1,
        1,
        "execution.result_committed",
        json!({
            "result": {
                "output": {
                    "schema":"kiana.data-governance-result.v1",
                    "project_root":"/repo",
                    "policy":{"revoked_sources":["src/input.txt"]}
                }
            }
        }),
    )];
    let event_id = events[0].event_id;
    let snapshot = project_data_governance_snapshot(&policy(), "/repo", &events, 1).unwrap();
    assert_eq!(snapshot.source_event_ids, vec![event_id]);
    assert_eq!(snapshot.observations[0].payload, DataPayloadState::Revoked);
    assert!(snapshot
        .derived_store_states
        .values()
        .all(|state| *state == DataPayloadState::Revoked));
}

#[test]
fn cursor_gap_and_duplicate_source_fail_closed() {
    let gap = vec![
        event(1, 1, "data.revocation_requested", json!({})),
        event(2, 3, "workspace.restored", json!({})),
    ];
    assert_eq!(
        project_data_governance_snapshot(&policy(), "/repo", &gap, 1).unwrap_err(),
        "data_governance_source_cursor_gap"
    );
    let duplicate = event(1, 1, "data.revocation_requested", json!({}));
    let duplicate_id = duplicate.clone();
    assert_eq!(
        project_data_governance_snapshot(&policy(), "/repo", &[duplicate, duplicate_id], 1)
            .unwrap_err(),
        "data_governance_source_event_duplicate"
    );
}

#[test]
fn governance_snapshot_does_not_expose_raw_payload_content() {
    let events = vec![event(
        1,
        1,
        "data.revocation_requested",
        json!({"project_root":"/repo","payload":"raw-secret"}),
    )];
    let snapshot = project_data_governance_snapshot(&policy(), "/repo", &events, 1).unwrap();
    let encoded = serde_json::to_string(&snapshot).unwrap();
    assert!(!encoded.contains("raw-secret"));
    assert!(!encoded.contains("payload"));
    assert_ne!(snapshot.source_event_ids, vec![EventId::new()]);
}
