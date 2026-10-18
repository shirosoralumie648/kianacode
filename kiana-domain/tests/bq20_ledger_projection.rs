//! BQ-20 projection contract fixtures: cursor/fence, freshness, quarantine and rollup separation.
//!
//! Deny first: every drift, forged digest and stale/fresh claim is refused before the success
//! column. All facts are synthesized in memory; nothing here appends to an EventLog.

use kiana_domain::*;
use serde_json::json;
use uuid::Uuid;

fn digest(seed: char) -> String {
    format!(
        "sha256:{}",
        format!("{:04x}", (seed as u32) & 0xffff).repeat(16)
    )
}

fn cursor_at(epoch: u64, source_cursor: EventCursor, ids: Vec<EventId>) -> BillingProjectionCursor {
    BillingProjectionCursor::new(
        epoch,
        source_cursor,
        BILLING_PROJECTION_NUMBER,
        digest('f'),
        ids,
    )
    .expect("cursor")
}

fn snapshot_at(source: BillingProjectionCursor) -> BillingProjectionSnapshot {
    BillingProjectionSnapshot::new(
        source,
        BillingRollupTotals::default(),
        BillingRollupTotals::default(),
        BillingRollupTotals::default(),
        Default::default(),
        Default::default(),
        Vec::new(),
    )
    .expect("snapshot")
}

fn quarantine_record(
    source_cursor: EventCursor,
    event_id: EventId,
    reason: &str,
    payload_seed: char,
) -> BillingQuarantineRecord {
    BillingQuarantineRecord::new(
        source_cursor,
        event_id,
        COST_EVENT_ESTIMATED,
        reason,
        digest(payload_seed),
    )
    .expect("quarantine record")
}

#[test]
fn cursor_binds_epoch_projection_version_and_replay_fence() {
    let cursor = cursor_at(7, 12, vec![EventId::new()]);
    cursor.validate().expect("valid cursor");

    // 逐字段漂移都必须以稳定码拒绝：epoch 漂移先于摘要重算被发现。
    let mut epoch_drift = cursor.clone();
    epoch_drift.source_epoch = 8;
    assert_eq!(
        epoch_drift.validate().unwrap_err(),
        "billing_projection_cursor_digest_mismatch"
    );
    let mut schema_drift = cursor.clone();
    schema_drift.schema = "kiana.other.v1".to_owned();
    assert_eq!(
        schema_drift.validate().unwrap_err(),
        "billing_projection_cursor_header_invalid"
    );
    let mut version_drift = cursor.clone();
    version_drift.projection_version = 0;
    assert_eq!(
        version_drift.validate().unwrap_err(),
        "billing_projection_cursor_header_invalid"
    );
    let mut fence_drift = cursor.clone();
    fence_drift.replay_fence = "not-a-digest".to_owned();
    assert_eq!(
        fence_drift.validate().unwrap_err(),
        "billing_projection_cursor_header_invalid"
    );
    let mut nil_event = cursor.clone();
    nil_event.source_event_ids = vec![EventId::from_uuid(Uuid::nil())];
    assert_eq!(
        nil_event.validate().unwrap_err(),
        "billing_projection_cursor_header_invalid"
    );
    let event_id = EventId::new();
    let mut duplicate_event = cursor.clone();
    duplicate_event.source_event_ids = vec![event_id, event_id];
    assert_eq!(
        duplicate_event.validate().unwrap_err(),
        "billing_projection_cursor_event_duplicate"
    );
    let mut oversized = cursor.clone();
    oversized.source_event_ids = (0..=MAX_BILLING_SOURCE_EVENT_IDS)
        .map(|_| EventId::new())
        .collect();
    assert_eq!(
        oversized.validate().unwrap_err(),
        "billing_projection_cursor_header_invalid"
    );
    assert!(BillingProjectionCursor::initial(0).is_err());

    // 合法游标必须可往返序列化，且来源游标是全局位置而不是请求内 sequence。
    let json = serde_json::to_value(&cursor).expect("cursor json");
    let decoded: BillingProjectionCursor = serde_json::from_value(json).expect("cursor decode");
    assert_eq!(decoded, cursor);
    assert_eq!(decoded.source_cursor, 12);
}

#[test]
fn stale_projection_cannot_be_claimed_fresh() {
    let snapshot = snapshot_at(cursor_at(7, 3, vec![EventId::new()]));
    assert_eq!(
        snapshot.freshness_against(3),
        BillingProjectionFreshness::Current
    );
    snapshot
        .require_fresh_at(3)
        .expect("exact cursor is current");
    // 落后于 live cursor 的投影是 stale；把它标成 fresh 必须被拒绝。
    assert_eq!(
        snapshot.freshness_against(4),
        BillingProjectionFreshness::Stale
    );
    assert_eq!(
        snapshot.require_fresh_at(4).unwrap_err(),
        "billing_projection_stale"
    );
    // 用更旧的 cursor 反推“当前”状态同样拒绝。
    assert_eq!(
        snapshot.freshness_against(2),
        BillingProjectionFreshness::Ahead
    );
    assert_eq!(
        snapshot.require_fresh_at(2).unwrap_err(),
        "billing_projection_cursor_ahead"
    );
    // 空快照只对 cursor 0 是 current。
    let empty = BillingProjectionSnapshot::empty(7).expect("empty snapshot");
    empty.require_fresh_at(0).expect("empty is current at 0");
    assert_eq!(
        empty.require_fresh_at(1).unwrap_err(),
        "billing_projection_stale"
    );
    let freshness = serde_json::to_value(BillingProjectionFreshness::Stale).expect("freshness");
    assert_eq!(freshness, json!("stale"));
}

#[test]
fn quarantine_record_is_queryable_and_secret_free() {
    let event_id = EventId::new();
    let shared_event_id = EventId::new();
    let record = quarantine_record(3, event_id, "usage_cost_decode_failed", 'q');
    record.validate().expect("valid record");
    let shared = quarantine_record(4, shared_event_id, "usage_cost_kind_mismatch", 'q');
    let snapshot = BillingProjectionSnapshot::new(
        cursor_at(7, 4, vec![event_id, shared_event_id]),
        BillingRollupTotals::default(),
        BillingRollupTotals::default(),
        BillingRollupTotals::default(),
        Default::default(),
        Default::default(),
        vec![record.clone(), shared.clone()],
    )
    .expect("snapshot");

    // 失败 cursor、事件 ID 与载荷摘要都必须可查询；未知摘要返回空。
    assert_eq!(snapshot.quarantine_for_cursor(3), vec![&record]);
    assert!(snapshot.quarantine_for_cursor(99).is_empty());
    assert_eq!(
        snapshot
            .quarantine_for_event(event_id)
            .map(|found| &found.reason),
        Some(&"usage_cost_decode_failed".to_owned())
    );
    assert_eq!(snapshot.quarantine_for_event(EventId::new()), None);
    assert_eq!(snapshot.quarantine_by_payload_digest(&digest('q')).len(), 2);
    assert!(snapshot
        .quarantine_by_payload_digest(&digest('z'))
        .is_empty());

    // 隔离记录只保留身份、原因与载荷摘要，不复制原始载荷。
    let json = serde_json::to_value(&record).expect("json");
    assert!(json.get("payload_digest").is_some());
    assert!(json.get("raw_payload").is_none());
    assert!(json.get("data").is_none());
    assert!(json.get("reason").is_some());
}

#[test]
fn snapshot_validate_rejects_header_rollup_and_quarantine_drift() {
    let base = snapshot_at(cursor_at(7, 2, vec![EventId::new()]));
    base.validate().expect("valid base snapshot");

    let mut schema_drift = base.clone();
    schema_drift.schema = "kiana.other.v1".to_owned();
    assert_eq!(
        schema_drift.validate().unwrap_err(),
        "billing_projection_snapshot_header_invalid"
    );
    let mut version_drift = base.clone();
    version_drift.projection_version = 2;
    assert_eq!(
        version_drift.validate().unwrap_err(),
        "billing_projection_snapshot_header_invalid"
    );
    let mut window_drift = base.clone();
    window_drift
        .window_rollups
        .insert(1, BillingRollupTotals::default());
    assert_eq!(
        window_drift.validate().unwrap_err(),
        "billing_projection_window_key_invalid"
    );
    let mut day_drift = base.clone();
    day_drift
        .daily_rollups
        .insert(String::new(), BillingRollupTotals::default());
    assert_eq!(
        day_drift.validate().unwrap_err(),
        "billing_projection_day_key_invalid"
    );
    let mut quarantine_overflow = base.clone();
    quarantine_overflow.quarantine =
        vec![quarantine_record(1, EventId::new(), "overflow", 'q'); MAX_BILLING_QUARANTINE + 1];
    assert_eq!(
        quarantine_overflow.validate().unwrap_err(),
        "billing_projection_snapshot_header_invalid"
    );
    // 重算摘要不匹配时，即使每个字段单看都合法也必须拒绝。
    let mut total_drift = base.clone();
    total_drift.usage.estimated = Some(Money::new("USD", 5).expect("money"));
    assert_eq!(
        total_drift.validate().unwrap_err(),
        "billing_projection_snapshot_digest_mismatch"
    );
    // 摘要篡改不能通过重签遮盖：改成合法形状的另一个摘要仍失配。
    let mut digest_drift = base.clone();
    digest_drift.projection_digest = digest('x');
    assert_eq!(
        digest_drift.validate().unwrap_err(),
        "billing_projection_snapshot_digest_mismatch"
    );
    assert!(BillingProjectionSnapshot::empty(0).is_err());
}

#[test]
fn rollup_keeps_estimated_measured_and_correction_separate() {
    let mut totals = BillingRollupTotals::default();
    totals
        .add_estimated(&Money::new("USD", 10).expect("money"))
        .expect("estimate");
    totals
        .add_measured(&Money::new("USD", 11).expect("money"))
        .expect("measured");
    totals
        .add_correction_estimated(&Money::new("USD", -2).expect("money"))
        .expect("correction");
    assert_eq!(totals.estimated.expect("estimate").micros, 10);
    assert_eq!(totals.measured.expect("measured").micros, 11);
    assert_eq!(totals.correction_estimated.expect("correction").micros, -2);
    assert!(totals.correction_measured.is_none());

    // unknown 只计数：它不能伪造一个零金额，也不能被当作已观测金额。
    let unknown_only = BillingRollupTotals {
        unknown_count: 2,
        ..BillingRollupTotals::default()
    };
    assert!(unknown_only.estimated.is_none());
    assert!(unknown_only.measured.is_none());
    assert!(unknown_only.validate().is_ok());

    // 币种漂移与溢出在 checked-add 处拒绝，而不是静默相加。
    let mut mixed = BillingRollupTotals::default();
    mixed
        .add_estimated(&Money::new("USD", 1).expect("money"))
        .expect("first");
    assert_eq!(
        mixed.add_estimated(&Money::new("EUR", 1).expect("money")),
        Err("money_currency_mismatch".to_owned())
    );
    let mut overflow = BillingRollupTotals::default();
    overflow
        .add_measured(&Money::new("USD", i128::MAX).expect("money"))
        .expect("first");
    assert_eq!(
        overflow.add_measured(&Money::new("USD", 1).expect("money")),
        Err("money_arithmetic_overflow".to_owned())
    );
    // 非法 Money（错误 schema）在 totals 校验时拒绝。
    let invalid_totals = BillingRollupTotals {
        estimated: Some(Money {
            schema: "kiana.other-money.v1".to_owned(),
            currency: "USD".to_owned(),
            micros: 1,
        }),
        ..BillingRollupTotals::default()
    };
    assert_eq!(invalid_totals.validate(), Err("money_invalid".to_owned()));
}

#[test]
fn replay_fence_and_event_id_tail_are_bounded_and_deterministic() {
    let first = EventId::new();
    let second = EventId::new();
    let start = digest('f');
    let fence_one = billing_replay_fence(&start, 1, first);
    let fence_two = billing_replay_fence(&fence_one, 2, second);
    assert_eq!(
        billing_replay_fence(&start, 1, first),
        fence_one,
        "same inputs produce the same fence"
    );
    assert_ne!(
        billing_replay_fence(&start, 2, first),
        fence_one,
        "a different global cursor changes the fence"
    );
    assert_ne!(
        billing_replay_fence(&fence_one, 2, first),
        fence_two,
        "a different event changes the fence"
    );
    assert!(fence_two.starts_with("sha256:"));

    assert!(billing_source_event_id_tail(&[], &[]).is_empty());
    assert_eq!(
        billing_source_event_id_tail(&[], &[first, second]),
        vec![first, second]
    );
    let existing: Vec<EventId> = (0..MAX_BILLING_SOURCE_EVENT_IDS)
        .map(|_| EventId::new())
        .collect();
    let tail = billing_source_event_id_tail(&existing, &[second]);
    assert_eq!(tail.len(), MAX_BILLING_SOURCE_EVENT_IDS);
    assert_eq!(tail.last(), Some(&second));
    assert_eq!(tail.first(), Some(&existing[1]));
}

#[test]
fn model_transcript_cannot_decode_into_a_projection_contract() {
    let cursor = cursor_at(7, 1, vec![EventId::new()]);
    let mut cursor_json = serde_json::to_value(&cursor).expect("cursor json");
    cursor_json["transcript"] = json!("model text must not decode");
    assert!(serde_json::from_value::<BillingProjectionCursor>(cursor_json).is_err());

    let snapshot = snapshot_at(cursor);
    let mut snapshot_json = serde_json::to_value(&snapshot).expect("snapshot json");
    snapshot_json["model_text"] = json!("not part of the read model");
    assert!(serde_json::from_value::<BillingProjectionSnapshot>(snapshot_json).is_err());

    let record = quarantine_record(1, EventId::new(), "decode_failed", 'q');
    let mut record_json = serde_json::to_value(&record).expect("record json");
    record_json["raw_payload"] = json!({"prompt": "forbidden"});
    assert!(serde_json::from_value::<BillingQuarantineRecord>(record_json).is_err());
}

#[test]
fn projection_contracts_are_read_only() {
    let source = include_str!("../src/billing_projection.rs");
    for marker in [
        "BillingProjectionCursor",
        "replay_fence",
        "BillingQuarantineRecord",
        "BillingProjectionFreshness",
        "billing_replay_fence",
        "billing_source_event_id_tail",
        "quarantine_for_cursor",
        "BILLING_WINDOW_MILLIS",
        "MAX_BILLING_QUARANTINE",
        "billing_projection_stale",
        "billing_projection_cursor_ahead",
    ] {
        assert!(
            source.contains(marker),
            "BQ-20 domain marker missing: {marker}"
        );
    }
    for forbidden in [
        "EventStorePort",
        "CapabilityBroker",
        "commit_transition",
        "append(",
        "release_reservation",
        "ApprovalStore",
        "std::fs",
        "std::process",
        "tokio::spawn",
        "unsafe",
        "#[ignore",
    ] {
        assert!(
            !source.contains(forbidden),
            "BQ-20 domain boundary widened: {forbidden}"
        );
    }
}
