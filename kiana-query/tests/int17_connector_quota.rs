//! INT-17 Query quota projector fixtures.
//!
//! `include_str!` keeps the static source boundary guard, and the behaviour fixtures rebuild the
//! reservation/settlement states from committed `RuntimeEvent`s: a settled-then-released stream
//! still projects its settlement (the settlement binds the identity carried by the settled event,
//! not the latest CAS snapshot digest), Unknown usage stays visible without a numeric zero, and
//! tampered payloads, schema drift and duplicate/oversized projections are refused. The projector
//! never authorizes, settles or releases anything.

use kiana_domain::{
    json_digest, AttemptId, ClockObservation, ConnectorQuotaDimensions, ConnectorQuotaKey,
    ConnectorQuotaLimit, ConnectorQuotaPolicy, ConnectorQuotaReservation,
    ConnectorQuotaReservationState, ConnectorQuotaSettlement, InvocationId, QuotaReservationId,
    QuotaWindow, RateCardId, RequestId, RunId, RuntimeEvent, CONNECTOR_QUOTA_EVENT_CLAIMED,
    CONNECTOR_QUOTA_EVENT_RELEASED, CONNECTOR_QUOTA_EVENT_RESERVED, CONNECTOR_QUOTA_EVENT_SETTLED,
    CONNECTOR_QUOTA_RESERVATION_SCHEMA, CONNECTOR_QUOTA_SETTLEMENT_SCHEMA, CONNECTOR_QUOTA_STREAM,
};
use kiana_query::{
    project_connector_quota, ConnectorQuotaProjection, ConnectorQuotaProjectionEntry,
};
use serde_json::{json, Value};

#[test]
fn quota_projection_is_replayable_and_never_an_authorizer() {
    let source = include_str!("../src/connector_quota.rs");
    // 这些 marker 证明状态种类和投影字段仍留在源码中，不证明事件时序处理正确。
    for marker in [
        "project_connector_quota",
        "ConnectorQuotaProjection",
        "CONNECTOR_QUOTA_EVENT_RESERVED",
        "CONNECTOR_QUOTA_EVENT_CLAIMED",
        "CONNECTOR_QUOTA_EVENT_SETTLED",
        "CONNECTOR_QUOTA_EVENT_RELEASED",
        "replayable",
        "projection_is_not_authorization",
        "source_cursor",
    ] {
        assert!(
            source.contains(marker),
            "INT-17 query marker missing: {marker}"
        );
    }
    // 禁止词约束只搜索此单一源码文件；它不是整个 workspace 的权限或依赖审计。
    for forbidden in [
        "CapabilityBroker",
        "authorize_and_execute",
        "resolve_secret",
        "ConnectorQuotaLedger",
    ] {
        assert!(
            !source.contains(forbidden),
            "quota query must stay read-only: {forbidden}"
        );
    }
}

fn window() -> QuotaWindow {
    let clock = ClockObservation::observe("int17-query", 125_000, 125_000, None, 1).unwrap();
    QuotaWindow::from_clock(&clock, 60_000).unwrap()
}

fn policy() -> ConnectorQuotaPolicy {
    let limit = ConnectorQuotaLimit {
        max_requests: 2,
        max_tokens: 100,
        max_budget_micros: 1_000,
        max_concurrency: 1,
    };
    ConnectorQuotaPolicy::new(
        ConnectorQuotaKey::new(
            "connector-int17-query",
            "v1",
            "account-a",
            "project-a",
            "main",
            3,
        )
        .unwrap(),
        window(),
        limit.clone(),
        limit.clone(),
        limit,
        9,
        "config:9",
        Some(RateCardId::new()),
        Some(json_digest(&json!({"capacity_lease": "fixture"}))),
    )
    .unwrap()
}

fn reserved(policy: &ConnectorQuotaPolicy) -> ConnectorQuotaReservation {
    ConnectorQuotaReservation::new(
        QuotaReservationId::new(),
        InvocationId::new(),
        1,
        RunId::new(),
        AttemptId::new(),
        json_digest(&json!({"owner": "project-a"})),
        json_digest(&json!({"command": "connector.invoke"})),
        policy,
        ConnectorQuotaDimensions::invocation(20, 100),
        125_000,
        179_000,
    )
    .unwrap()
}

fn with_state(
    reservation: &ConnectorQuotaReservation,
    state: ConnectorQuotaReservationState,
    revision: u64,
) -> ConnectorQuotaReservation {
    let mut next = reservation.clone();
    next.state = state;
    next.claimed_by =
        (state == ConnectorQuotaReservationState::Claimed).then(|| "worker-a".to_owned());
    next.revision = revision;
    next.reservation_digest = next.digest();
    next
}

fn event(kind: &str, stream_version: u64, data: Value) -> RuntimeEvent {
    RuntimeEvent::new(RequestId::new(), 1, kind, data)
        .unwrap()
        .with_stream_metadata(CONNECTOR_QUOTA_STREAM, "quota-aggregate", stream_version)
}

fn reservation_event(
    kind: &str,
    stream_version: u64,
    reservation: &ConnectorQuotaReservation,
) -> RuntimeEvent {
    event(
        kind,
        stream_version,
        json!({
            "schema": "kiana.connector-quota-event.v1",
            "reservation": reservation,
            "reservation_id": reservation.reservation_id,
            "state": reservation.state,
        }),
    )
}

fn settled_event(
    stream_version: u64,
    snapshot: &ConnectorQuotaReservation,
    settlement: &ConnectorQuotaSettlement,
) -> RuntimeEvent {
    event(
        CONNECTOR_QUOTA_EVENT_SETTLED,
        stream_version,
        json!({
            "schema": "kiana.connector-quota-event.v1",
            "reservation": snapshot,
            "settlement": settlement,
            "reservation_id": snapshot.reservation_id,
            "state": snapshot.state,
        }),
    )
}

#[test]
fn settled_then_released_projection_replays_states_and_settlement() {
    let policy = policy();
    let reserved_state = reserved(&policy);
    let claimed = with_state(&reserved_state, ConnectorQuotaReservationState::Claimed, 2);
    let released = with_state(&reserved_state, ConnectorQuotaReservationState::Released, 3);
    let settlement = ConnectorQuotaSettlement::new(
        &claimed,
        "worker-a",
        Some("receipt-a".to_owned()),
        Some(10),
        Some(50),
        125_010,
    )
    .unwrap();

    let events = vec![
        reservation_event(CONNECTOR_QUOTA_EVENT_RESERVED, 1, &reserved),
        reservation_event(CONNECTOR_QUOTA_EVENT_CLAIMED, 2, &claimed),
        settled_event(3, &claimed, &settlement),
        reservation_event(CONNECTOR_QUOTA_EVENT_RELEASED, 4, &released),
    ];
    let projection = project_connector_quota(&events).expect("projection");
    projection.validate().expect("valid projection");
    assert_eq!(projection.source_cursor, 4);
    assert_eq!(projection.entries.len(), 1);
    let entry = &projection.entries[0];
    assert_eq!(
        entry.reservation.reservation_id,
        reserved_state.reservation_id
    );
    // The latest snapshot wins for state: release after settlement stays visible.
    assert_eq!(
        entry.reservation.state,
        ConnectorQuotaReservationState::Released
    );
    assert_eq!(entry.source_cursor, 4);
    // The settlement is preserved even though the released snapshot advanced its digest.
    let projected = entry.settlement.as_ref().expect("settlement retained");
    assert_eq!(projected.settlement_digest, settlement.settlement_digest);
    assert_eq!(projected.reservation_id, reserved_state.reservation_id);
    assert_eq!(
        projection.limitations,
        vec!["projection_is_not_authorization".to_owned()]
    );

    // Replay is deterministic: the same committed facts rebuild byte-identical evidence.
    let replayed = project_connector_quota(&events).expect("replay");
    assert_eq!(replayed, projection);

    // A prefix of the stream shows the earlier state without inventing a settlement.
    let partial = project_connector_quota(&events[..2]).expect("partial projection");
    assert_eq!(partial.entries.len(), 1);
    assert_eq!(
        partial.entries[0].reservation.state,
        ConnectorQuotaReservationState::Claimed
    );
    assert!(partial.entries[0].settlement.is_none());
    assert_eq!(partial.entries[0].source_cursor, 2);
}

#[test]
fn unknown_usage_stays_visible_and_unrelated_events_only_advance_the_cursor() {
    let policy = policy();
    let reserved_state = reserved(&policy);
    let claimed = with_state(&reserved_state, ConnectorQuotaReservationState::Claimed, 2);
    let unknown =
        ConnectorQuotaSettlement::new(&claimed, "worker-a", None, None, None, 125_020).unwrap();
    assert!(!unknown.usage_known);

    // A noise event of an unrelated kind is skipped for entries but still counts as a cursor.
    let noise = event(
        "connector.invoked",
        7,
        json!({"receipt": {"outcome": "succeeded"}}),
    );
    let events = vec![
        reservation_event(CONNECTOR_QUOTA_EVENT_RESERVED, 1, &reserved_state),
        settled_event(2, &claimed, &unknown),
        noise,
    ];
    let projection = project_connector_quota(&events).expect("projection");
    assert_eq!(projection.source_cursor, 7);
    assert_eq!(projection.entries.len(), 1);
    let settlement = projection.entries[0]
        .settlement
        .as_ref()
        .expect("unknown settlement visible");
    assert!(!settlement.usage_known);
    assert!(settlement.charged_tokens.is_none());
    assert!(settlement.charged_budget_micros.is_none());

    // An empty stream yields no entries and says so instead of implying free capacity.
    let empty = project_connector_quota(&[]).expect("empty projection");
    assert!(empty.entries.is_empty());
    assert_eq!(empty.source_cursor, 1);
    assert_eq!(
        empty.limitations,
        vec!["connector_quota_not_observed".to_owned()]
    );
}

#[test]
fn tampered_payloads_and_schema_drift_are_refused_without_partial_projection() {
    let policy = policy();
    let reserved_state = reserved(&policy);
    let claimed = with_state(&reserved_state, ConnectorQuotaReservationState::Claimed, 2);
    let settlement = ConnectorQuotaSettlement::new(
        &claimed,
        "worker-a",
        Some("receipt-a".to_owned()),
        Some(10),
        Some(50),
        125_010,
    )
    .unwrap();

    // A settlement bound to another reservation identity is refused at its own event.
    let other = with_state(
        &reserved(&policy),
        ConnectorQuotaReservationState::Claimed,
        2,
    );
    let foreign = ConnectorQuotaSettlement::new(
        &other,
        "worker-a",
        Some("receipt-b".to_owned()),
        Some(10),
        Some(50),
        125_010,
    )
    .unwrap();
    let foreign_events = vec![settled_event(1, &claimed, &foreign)];
    assert_eq!(
        project_connector_quota(&foreign_events).unwrap_err(),
        "connector_quota_settlement_reservation_mismatch"
    );

    // A reservation whose recorded digest no longer matches its content is refused.
    let mut tampered = claimed.clone();
    tampered.reservation_digest = json_digest(&json!({"forged": true}));
    let tampered_events = vec![reservation_event(
        CONNECTOR_QUOTA_EVENT_CLAIMED,
        1,
        &tampered,
    )];
    assert_eq!(
        project_connector_quota(&tampered_events).unwrap_err(),
        "connector_quota_reservation_invalid"
    );

    // Schema drift on the reservation and the settlement is refused separately.
    let mut drifted_schema = serde_json::to_value(&claimed).unwrap();
    drifted_schema["schema"] = json!("kiana.connector-quota-reservation.v2");
    let drifted = event(
        CONNECTOR_QUOTA_EVENT_CLAIMED,
        1,
        json!({"reservation": drifted_schema}),
    );
    assert_eq!(
        project_connector_quota(&[drifted]).unwrap_err(),
        "connector_quota_reservation_schema_invalid"
    );
    let mut drifted_settlement = serde_json::to_value(&settlement).unwrap();
    drifted_settlement["schema"] = json!("kiana.connector-quota-settlement.v2");
    let drifted_settlement = event(
        CONNECTOR_QUOTA_EVENT_SETTLED,
        1,
        json!({"reservation": claimed.clone(), "settlement": drifted_settlement}),
    );
    assert_eq!(
        project_connector_quota(&[drifted_settlement]).unwrap_err(),
        "connector_quota_settlement_schema_invalid"
    );

    // Model text or unknown fields can never be decoded into a quota fact.
    let mut with_transcript = serde_json::to_value(&claimed).unwrap();
    with_transcript["transcript"] = json!("model text");
    let smuggled = event(
        CONNECTOR_QUOTA_EVENT_CLAIMED,
        1,
        json!({"reservation": with_transcript}),
    );
    assert_eq!(
        project_connector_quota(&[smuggled]).unwrap_err(),
        "connector_quota_reservation_invalid"
    );

    // The successful path is the last one: the same events still project after all refusals.
    let accepted = vec![reservation_event(
        CONNECTOR_QUOTA_EVENT_RESERVED,
        1,
        &reserved_state,
    )];
    assert!(project_connector_quota(&accepted).is_ok());
    assert_eq!(
        CONNECTOR_QUOTA_RESERVATION_SCHEMA,
        "kiana.connector-quota-reservation.v1"
    );
    assert_eq!(
        CONNECTOR_QUOTA_SETTLEMENT_SCHEMA,
        "kiana.connector-quota-settlement.v1"
    );
}

#[test]
fn projection_validate_refuses_duplicate_drifted_and_oversized_headers() {
    let policy = policy();
    let reserved_state = reserved(&policy);
    let entry = ConnectorQuotaProjectionEntry {
        reservation: reserved_state.clone(),
        settlement: None,
        source_cursor: 1,
        projection_version: "connector-quota.v1".to_owned(),
        replayable: true,
        proof_level: "source".to_owned(),
    };
    let projection = ConnectorQuotaProjection {
        schema: "kiana.connector-quota-projection.v1".to_owned(),
        source_cursor: 1,
        projection_version: "connector-quota.v1".to_owned(),
        entries: vec![entry.clone()],
        limitations: vec!["projection_is_not_authorization".to_owned()],
    };
    assert!(projection.validate().is_ok());

    // Duplicate reservation identities are refused even with valid single entries.
    let duplicate = ConnectorQuotaProjection {
        entries: vec![entry.clone(), entry.clone()],
        ..projection.clone()
    };
    assert_eq!(
        duplicate.validate().unwrap_err(),
        "connector_quota_projection_duplicate_reservation"
    );

    // Header drift (schema/version/cursor/limitations) is refused.
    let mut drifted = projection.clone();
    drifted.projection_version = "connector-quota.v2".to_owned();
    assert_eq!(
        drifted.validate().unwrap_err(),
        "connector_quota_projection_invalid"
    );
    let mut zero_cursor = projection.clone();
    zero_cursor.source_cursor = 0;
    assert_eq!(
        zero_cursor.validate().unwrap_err(),
        "connector_quota_projection_invalid"
    );
    let mut empty_limitation = projection.clone();
    empty_limitation.limitations = vec!["".to_owned()];
    assert_eq!(
        empty_limitation.validate().unwrap_err(),
        "connector_quota_projection_invalid"
    );

    // The entry-level proof label and replay flag are pinned to source evidence.
    let mut claimed_replay = entry.clone();
    claimed_replay.replayable = false;
    assert_eq!(
        claimed_replay.validate().unwrap_err(),
        "connector_quota_projection_entry_invalid"
    );

    // The entry bound is enforced before any per-entry validation: 513 entries are refused as an
    // oversized projection even though every entry is otherwise valid.
    let oversized = ConnectorQuotaProjection {
        entries: (0..513).map(|_| entry.clone()).collect(),
        ..projection
    };
    assert_eq!(
        oversized.validate().unwrap_err(),
        "connector_quota_projection_invalid"
    );
}
