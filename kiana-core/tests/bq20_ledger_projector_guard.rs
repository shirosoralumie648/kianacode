//! BQ-20 ControlPlane cursor-fence fixtures and source/wiring guards (deny first).
//!
//! The first half executes the fence with in-memory pages: a candidate projection must be
//! validated, and its replay fence plus bounded event-ID tail must chain exactly from the
//! acknowledged cursor over the committed page, before the cursor may move. The second half is a
//! static guard: it pins markers, refusal codes, fixture names and the CI wiring (manual lane and
//! `scripts/ci/test-shards.json`). `include_str!` hits are text checks, not runtime proof.

use kiana_core::{
    BillingProjectionFence, BillingProjectionFenceError, BILLING_PROJECTION_FENCE_NO_FACT_WRITES,
};
use kiana_domain::*;
use serde_json::json;

const DOMAIN_SOURCE: &str = include_str!("../../kiana-domain/src/billing_projection.rs");
const CORE_SOURCE: &str = include_str!("../src/billing_projection.rs");
const EVENTLOG_SOURCE: &str = include_str!("../../kiana-eventlog/src/ledger_source.rs");
const QUERY_SOURCE: &str = include_str!("../../kiana-query/src/billing_ledger_projector.rs");
const DOMAIN_FIXTURES: &str = include_str!("../../kiana-domain/tests/bq20_ledger_projection.rs");
const EVENTLOG_FIXTURES: &str = include_str!("../../kiana-eventlog/tests/bq20_ledger_source.rs");
const QUERY_FIXTURES: &str = include_str!("../../kiana-query/tests/bq20_ledger_projector.rs");
const QUERY_GUARD: &str = include_str!("../../kiana-query/tests/bq20_ledger_projector_guard.rs");
const MANUAL_LANE: &str = include_str!("../../.github/workflows/bq20-ledger-projector.yml");
const SHARDS: &str = include_str!("../../scripts/ci/test-shards.json");

fn digest(seed: char) -> String {
    format!(
        "sha256:{}",
        format!("{:04x}", (seed as u32) & 0xffff).repeat(16)
    )
}

fn event(kind: &str) -> RuntimeEvent {
    RuntimeEvent::new(RequestId::new(), 1, kind, json!({})).expect("event")
}

fn snapshot_with(source: BillingProjectionCursor) -> BillingProjectionSnapshot {
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

fn page(events: Vec<RuntimeEvent>, cursor: EventCursor) -> JournalPage {
    JournalPage {
        events,
        cursor,
        has_more: false,
    }
}

/// Build the candidate the fence rule documents: chain from the acknowledged cursor over exactly
/// the page events and extend the bounded tail.
fn chained_candidate(
    epoch: u64,
    previous: &BillingProjectionCursor,
    page: &JournalPage,
) -> BillingProjectionSnapshot {
    let mut replay_fence = previous.replay_fence.clone();
    let mut cursor = previous.source_cursor;
    let mut page_ids = Vec::new();
    for event in &page.events {
        cursor += 1;
        replay_fence = billing_replay_fence(&replay_fence, cursor, event.event_id);
        page_ids.push(event.event_id);
    }
    let ids = billing_source_event_id_tail(&previous.source_event_ids, &page_ids);
    snapshot_with(
        BillingProjectionCursor::new(
            epoch,
            page.cursor,
            BILLING_PROJECTION_NUMBER,
            replay_fence,
            ids,
        )
        .expect("cursor"),
    )
}

#[test]
fn fence_requires_candidate_projection_before_cursor_ack() {
    let mut fence = BillingProjectionFence::new(5).expect("fence");
    let started = event("run.started");
    let page = page(vec![started.clone()], 1);
    let candidate = chained_candidate(5, fence.cursor(), &page);
    fence.stage(&page, &candidate, 5).expect("stage");
    assert_eq!(fence.source_cursor(), 1);
    assert_eq!(fence.cursor().source_event_ids, vec![started.event_id]);
    assert!(BILLING_PROJECTION_FENCE_NO_FACT_WRITES);
}

#[test]
fn fence_rejects_epoch_version_and_invalid_candidate_before_ack() {
    assert_eq!(
        BillingProjectionFence::new(0).unwrap_err(),
        BillingProjectionFenceError::EpochInvalid
    );
    let mut fence = BillingProjectionFence::new(5).expect("fence");
    let started = event("run.started");
    let page = page(vec![started], 1);
    let valid = chained_candidate(5, fence.cursor(), &page);

    // wrong epoch parameter: refused before the candidate is even inspected
    assert_eq!(
        fence.stage(&page, &valid, 6).unwrap_err(),
        BillingProjectionFenceError::EpochInvalid
    );
    assert_eq!(fence.source_cursor(), 0);

    // candidate assembled under another source epoch
    let epoch_drift = chained_candidate(6, fence.cursor(), &page);
    assert_eq!(
        fence.stage(&page, &epoch_drift, 5).unwrap_err(),
        BillingProjectionFenceError::VersionMismatch
    );
    assert_eq!(fence.source_cursor(), 0);

    // projection implementation version drift
    let version_drift = snapshot_with(
        BillingProjectionCursor::new(
            5,
            1,
            2,
            valid.source.replay_fence.clone(),
            valid.source.source_event_ids.clone(),
        )
        .expect("cursor"),
    );
    assert_eq!(
        fence.stage(&page, &version_drift, 5).unwrap_err(),
        BillingProjectionFenceError::VersionMismatch
    );
    assert_eq!(fence.source_cursor(), 0);

    // candidate snapshot that does not validate at all
    let mut invalid = valid.clone();
    invalid.usage.estimated = Some(Money::new("USD", 1).expect("money"));
    assert!(matches!(
        fence.stage(&page, &invalid, 5),
        Err(BillingProjectionFenceError::CandidateInvalid(_))
    ));
    assert_eq!(fence.source_cursor(), 0);

    // every refusal code stays pinned as a stable string
    for (error, code) in [
        (
            BillingProjectionFenceError::EpochInvalid,
            "billing_projection_epoch_invalid",
        ),
        (
            BillingProjectionFenceError::CursorRegressed,
            "billing_projection_cursor_regressed",
        ),
        (
            BillingProjectionFenceError::CursorGap,
            "billing_projection_cursor_gap",
        ),
        (
            BillingProjectionFenceError::VersionMismatch,
            "billing_projection_version_mismatch",
        ),
        (
            BillingProjectionFenceError::SourceFenceMismatch,
            "billing_projection_source_fence_mismatch",
        ),
    ] {
        assert_eq!(error.to_string(), code);
    }
}

#[test]
fn fence_rejects_cursor_gap_and_preserves_previous_cursor() {
    let mut fence = BillingProjectionFence::new(5).expect("fence");
    let started = event("run.started");
    let page = page(vec![started], 2);
    let candidate = chained_candidate(5, fence.cursor(), &page);
    assert_eq!(
        fence.stage(&page, &candidate, 5).unwrap_err(),
        BillingProjectionFenceError::CursorGap
    );
    assert_eq!(fence.source_cursor(), 0);
}

#[test]
fn fence_rejects_gap_regression_and_replayed_fence() {
    let mut fence = BillingProjectionFence::new(5).expect("fence");
    let first = event("run.started");
    let page_one = page(vec![first.clone()], 1);
    let candidate_one = chained_candidate(5, fence.cursor(), &page_one);
    fence
        .stage(&page_one, &candidate_one, 5)
        .expect("first page");

    // cursor gap: the page must cover exactly the committed range
    let second = event("run.completed");
    let gapped = page(vec![second.clone()], 3);
    let candidate_gap = chained_candidate(5, fence.cursor(), &gapped);
    assert_eq!(
        fence.stage(&gapped, &candidate_gap, 5).unwrap_err(),
        BillingProjectionFenceError::CursorGap
    );
    assert_eq!(fence.source_cursor(), 1);

    // regression: a page that ends before the acknowledged cursor
    let regressed = page(Vec::new(), 0);
    let candidate_regressed = chained_candidate(5, fence.cursor(), &regressed);
    assert_eq!(
        fence
            .stage(&regressed, &candidate_regressed, 5)
            .unwrap_err(),
        BillingProjectionFenceError::CursorRegressed
    );
    assert_eq!(fence.source_cursor(), 1);

    let page_two = page(vec![second.clone()], 2);
    // stale candidate: cursor matches the page, replay fence is from the previous page only
    let stale = snapshot_with(
        BillingProjectionCursor::new(
            5,
            2,
            BILLING_PROJECTION_NUMBER,
            candidate_one.source.replay_fence.clone(),
            candidate_one.source.source_event_ids.clone(),
        )
        .expect("stale cursor"),
    );
    assert_eq!(
        fence.stage(&page_two, &stale, 5).unwrap_err(),
        BillingProjectionFenceError::SourceFenceMismatch
    );
    assert_eq!(fence.source_cursor(), 1);

    // correct fence chain but a tail that does not include the new event
    let valid_two = chained_candidate(5, fence.cursor(), &page_two);
    let tail_drift = snapshot_with(
        BillingProjectionCursor::new(
            5,
            2,
            BILLING_PROJECTION_NUMBER,
            valid_two.source.replay_fence.clone(),
            candidate_one.source.source_event_ids.clone(),
        )
        .expect("cursor"),
    );
    assert_eq!(
        fence.stage(&page_two, &tail_drift, 5).unwrap_err(),
        BillingProjectionFenceError::SourceFenceMismatch
    );
    assert_eq!(fence.source_cursor(), 1);

    // the exact successor still passes after all the refusals
    fence.stage(&page_two, &valid_two, 5).expect("successor");
    assert_eq!(fence.source_cursor(), 2);
}

#[test]
fn fence_stages_one_page_at_a_time_and_keeps_the_acknowledged_tail() {
    let mut fence = BillingProjectionFence::new(5).expect("fence");
    let first = event("run.started");
    let second = event("run.completed");
    let page_one = page(vec![first.clone(), second.clone()], 2);
    let candidate_one = chained_candidate(5, fence.cursor(), &page_one);
    fence.stage(&page_one, &candidate_one, 5).expect("page one");
    assert_eq!(fence.source_cursor(), 2);
    assert_eq!(
        fence.cursor().source_event_ids,
        vec![first.event_id, second.event_id]
    );

    // a from-scratch candidate is not a valid successor of an acknowledged cursor
    let replay = chained_candidate(
        5,
        &BillingProjectionCursor::initial(5).expect("initial"),
        &page_one,
    );
    assert_eq!(
        fence.stage(&page_one, &replay, 5).unwrap_err(),
        BillingProjectionFenceError::CursorGap
    );

    // an empty page at the acknowledged cursor is an explicit no-op
    let empty = page(Vec::new(), 2);
    let empty_candidate = snapshot_with(fence.cursor().clone());
    fence
        .stage(&empty, &empty_candidate, 5)
        .expect("empty page");
    assert_eq!(fence.source_cursor(), 2);
    assert_eq!(fence.cursor().source_event_ids.len(), 2);

    // the next page chains onto the acknowledged fence, not onto a rebuilt one
    let third = event("run.settled");
    let page_two = page(vec![third.clone()], 3);
    let candidate_two = chained_candidate(5, fence.cursor(), &page_two);
    fence.stage(&page_two, &candidate_two, 5).expect("page two");
    assert_eq!(fence.source_cursor(), 3);
    assert_eq!(
        fence.cursor().source_event_ids.last(),
        Some(&third.event_id)
    );
}

#[test]
fn core_boundary_is_not_an_event_store_or_execution_loop() {
    for marker in [
        "BillingProjectionFence",
        "stage",
        "billing_replay_fence",
        "billing_source_event_id_tail",
        "SourceFenceMismatch",
        "BILLING_PROJECTION_FENCE_NO_FACT_WRITES",
    ] {
        assert!(
            CORE_SOURCE.contains(marker),
            "BQ-20 core marker missing: {marker}"
        );
    }
    for forbidden in [
        "EventStorePort",
        "CapabilityBroker",
        "commit_transition",
        "append(",
        "release_reservation",
        "ApprovalStore",
        "tokio::spawn",
        "std::fs",
        "#[ignore",
    ] {
        assert!(
            !CORE_SOURCE.contains(forbidden),
            "BQ-20 core boundary widened: {forbidden}"
        );
    }
}

#[test]
fn projection_sources_own_no_update_delete_or_second_authority() {
    for marker in [
        "BillingProjectionCursor",
        "replay_fence",
        "BillingQuarantineRecord",
        "BillingProjectionFreshness",
        "billing_replay_fence",
        "billing_source_event_id_tail",
        "quarantine_for_cursor",
        "quarantine_for_event",
        "quarantine_by_payload_digest",
        "BILLING_WINDOW_MILLIS",
        "MAX_BILLING_QUARANTINE",
        "billing_projection_stale",
        "billing_projection_cursor_ahead",
    ] {
        assert!(
            DOMAIN_SOURCE.contains(marker),
            "BQ-20 domain marker missing: {marker}"
        );
    }
    for marker in [
        "BillingSourcePage",
        "read_billing_page",
        "read_ledger_page",
        "billing_source_epoch_invalid",
        "billing_source_cursor_regressed",
        "billing_source_cursor_not_contiguous",
        "billing_source_event_id_invalid",
        "billing_source_event_duplicate",
        "BILLING_SOURCE_IS_READ_ONLY",
    ] {
        assert!(
            EVENTLOG_SOURCE.contains(marker),
            "BQ-20 eventlog marker missing: {marker}"
        );
    }
    for marker in [
        "BillingLedgerProjector",
        "BillingProjectionSnapshot",
        "quarantine",
        "correction_stream_regression",
        "correction_stream_gap",
        "source_cursor",
        "replay_fence",
        "BILLING_PROJECTOR_NO_FACT_WRITES",
    ] {
        assert!(
            QUERY_SOURCE.contains(marker),
            "BQ-20 query marker missing: {marker}"
        );
    }

    // 每个源文件都不得包含原地修改/删除、第二授权面或执行器引用。eventlog 适配器只允许
    // 通过 EventStorePort 读取，因此它的负断言检查具体写路径而不是 trait 名称。
    for forbidden in [
        "fn update",
        "fn delete",
        "fn replace",
        "fn mutate",
        "DELETE FROM",
        "UPDATE ",
        "set_amount",
        "CapabilityBrokerPort",
        "KeyringPort",
        "reqwest",
        "ModelOutput",
        "FinancialBudget",
        "Uuid::new_v4",
        "unsafe",
    ] {
        for (name, source) in [
            ("domain", DOMAIN_SOURCE),
            ("core", CORE_SOURCE),
            ("eventlog", EVENTLOG_SOURCE),
            ("query", QUERY_SOURCE),
        ] {
            assert!(
                !source.contains(forbidden),
                "BQ-20 {name} authority widened: {forbidden}"
            );
        }
    }
    for forbidden in [".append(", "commit_transition", "tokio::spawn"] {
        for (name, source) in [
            ("domain", DOMAIN_SOURCE),
            ("core", CORE_SOURCE),
            ("eventlog", EVENTLOG_SOURCE),
            ("query", QUERY_SOURCE),
        ] {
            assert!(
                !source.contains(forbidden),
                "BQ-20 {name} side-effect path widened: {forbidden}"
            );
        }
    }
    // 五个模型可见工具面不变：投影路径不注册工具、不新增 harness 入口。
    for forbidden in ["apply_patch", "memory.search", "memory.write", "ToolSchema"] {
        for source in [DOMAIN_SOURCE, CORE_SOURCE, EVENTLOG_SOURCE, QUERY_SOURCE] {
            assert!(
                !source.contains(forbidden),
                "BQ-20 projection path gained a model tool surface: {forbidden}"
            );
        }
    }
    assert!(!QUERY_SOURCE.contains("EventStorePort"));
}

#[test]
fn refusal_codes_and_fixture_names_stay_pinned_in_the_fixtures() {
    for code in [
        "billing_projection_cursor_digest_mismatch",
        "billing_projection_cursor_header_invalid",
        "billing_projection_cursor_event_duplicate",
        "billing_projection_stale",
        "billing_projection_cursor_ahead",
        "billing_projection_snapshot_header_invalid",
        "billing_projection_window_key_invalid",
        "billing_projection_day_key_invalid",
        "billing_projection_snapshot_digest_mismatch",
        "money_currency_mismatch",
        "money_arithmetic_overflow",
        "money_invalid",
    ] {
        assert!(
            DOMAIN_FIXTURES.contains(code),
            "BQ-20 domain fixture refusal code missing: {code}"
        );
    }
    for code in [
        "billing_projection_epoch_invalid",
        "billing_projection_cursor_regressed",
        "billing_projection_cursor_gap",
        "billing_projection_version_mismatch",
        "billing_projection_source_fence_mismatch",
    ] {
        assert!(
            CORE_SOURCE.contains(code),
            "BQ-20 core source refusal code missing: {code}"
        );
    }
    for code in [
        "billing_source_epoch_invalid",
        "billing_source_cursor_regressed",
        "billing_source_cursor_not_contiguous",
        "billing_source_event_id_invalid",
        "billing_source_event_duplicate",
    ] {
        assert!(
            EVENTLOG_FIXTURES.contains(code),
            "BQ-20 eventlog fixture refusal code missing: {code}"
        );
    }
    for code in [
        "usage_cost_decode_failed",
        "usage_cost_attempt_missing",
        "usage_cost_stream_mismatch",
        "usage_cost_revision_gap",
        "usage_cost_transition_invalid",
        "ledger_entry_stream_mismatch",
        "ledger_entry_duplicate",
        "correction_stream_mismatch",
        "correction_target_missing",
        "correction_target_mismatch",
        "correction_duplicate",
        "correction_stream_regression",
        "correction_stream_gap",
        "allocation_stream_mismatch",
        "allocation_usage_duplicate",
        "billing_projection_source_epoch_invalid",
        "billing_projection_source_cursor_invalid",
        "billing_projection_source_cursor_gap",
        "billing_projection_source_replay",
        "billing_projection_source_event_duplicate",
        "billing_quarantine_limit",
        "billing_projection_snapshot_requires_rebuild",
    ] {
        assert!(
            QUERY_FIXTURES.contains(code),
            "BQ-20 query fixture refusal code missing: {code}"
        );
    }

    for name in [
        "cursor_binds_epoch_projection_version_and_replay_fence",
        "stale_projection_cannot_be_claimed_fresh",
        "quarantine_record_is_queryable_and_secret_free",
        "snapshot_validate_rejects_header_rollup_and_quarantine_drift",
        "rollup_keeps_estimated_measured_and_correction_separate",
        "replay_fence_and_event_id_tail_are_bounded_and_deterministic",
        "model_transcript_cannot_decode_into_a_projection_contract",
        "projection_contracts_are_read_only",
    ] {
        assert!(
            DOMAIN_FIXTURES.contains(name),
            "BQ-20 domain fixture missing: {name}"
        );
    }
    for name in [
        "fence_requires_candidate_projection_before_cursor_ack",
        "fence_rejects_cursor_gap_and_preserves_previous_cursor",
        "fence_rejects_epoch_version_and_invalid_candidate_before_ack",
        "fence_rejects_gap_regression_and_replayed_fence",
        "fence_stages_one_page_at_a_time_and_keeps_the_acknowledged_tail",
        "core_boundary_is_not_an_event_store_or_execution_loop",
        "projection_sources_own_no_update_delete_or_second_authority",
    ] {
        assert!(
            include_str!("bq20_ledger_projector_guard.rs").contains(name),
            "BQ-20 core fixture missing: {name}"
        );
    }
    for name in [
        "source_page_uses_committed_logical_cursor",
        "source_page_rejects_non_contiguous_cursor",
        "source_page_rejects_zero_epoch_and_cursor_regression",
        "source_page_rejects_duplicate_or_nil_event_ids",
        "source_page_empty_page_is_a_no_progress_page",
        "source_page_snapshot_does_not_append_to_the_log",
        "source_adapter_is_read_only",
    ] {
        assert!(
            EVENTLOG_FIXTURES.contains(name),
            "BQ-20 eventlog fixture missing: {name}"
        );
    }
    for name in [
        "rebuild_keeps_usage_and_ledger_separate_and_rolls_daily_window",
        "malformed_cost_is_quarantined_but_reservation_is_not_consumed",
        "failed_page_does_not_advance_cursor_and_replay_is_rejected",
        "snapshot_rebuild_failure_does_not_advance_cursor",
        "cursor_gap_and_empty_page_regression_are_refused",
        "quarantine_keeps_bad_facts_queryable_by_cursor_event_and_payload_digest",
        "quarantine_limit_fails_the_page_without_publishing",
        "usage_revisions_fold_to_the_latest_and_timestamp_less_facts_stay_unbucketed",
        "allocation_views_do_not_multiply_ledger_totals",
        "correction_stream_versions_must_follow_the_ledger_stream",
        "new_process_rebuilds_the_same_snapshot_digest",
        "projector_rejects_nil_and_empty_sources",
    ] {
        assert!(
            QUERY_FIXTURES.contains(name),
            "BQ-20 query fixture missing: {name}"
        );
    }

    for (name, source) in [
        ("domain", DOMAIN_FIXTURES),
        ("eventlog", EVENTLOG_FIXTURES),
        ("query", QUERY_FIXTURES),
    ] {
        assert!(
            !source.contains("#[ignore"),
            "BQ-20 {name} fixtures must not be ignored"
        );
    }
    // guard 自身保留 `#[ignore` 负断言文本，因此只禁止跳过参数。
    assert!(
        !QUERY_GUARD.contains("--ignored"),
        "BQ-20 query guard must not skip fixtures"
    );
}

#[test]
fn bq20_fixtures_are_wired_to_the_manual_lane_and_the_auto_shards() {
    for command in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-domain --test bq20_ledger_projection --locked -- --test-threads=1",
        "cargo test -p kiana-core --test bq20_ledger_projector_guard --locked -- --test-threads=1",
        "cargo test -p kiana-eventlog --test bq20_ledger_source --locked -- --test-threads=1",
        "cargo test -p kiana-query --test bq20_ledger_projector --locked -- --test-threads=1",
        "cargo test -p kiana-query --test bq20_ledger_projector_guard --locked -- --test-threads=1",
    ] {
        assert!(
            MANUAL_LANE.contains(command),
            "BQ-20 manual lane missing: {command}"
        );
    }
    for forbidden in ["push:", "pull_request:", "continue-on-error", "--ignored"] {
        assert!(
            !MANUAL_LANE.contains(forbidden),
            "BQ-20 manual lane must stay dispatch-only: {forbidden}"
        );
    }
    assert!(MANUAL_LANE.contains("workflow_dispatch:"));
    assert!(MANUAL_LANE.contains("permissions:"));

    let shards: serde_json::Value = serde_json::from_str(SHARDS).expect("shard registry json");
    let domain_targets = shards["kiana-domain-s1/4"]["targets"]
        .as_array()
        .expect("domain shard targets");
    assert!(domain_targets
        .iter()
        .any(|target| target.as_str() == Some("bq20_ledger_projection")));
    let core_targets = shards["kiana-core-s1/6"]["targets"]
        .as_array()
        .expect("core shard targets");
    assert!(core_targets
        .iter()
        .any(|target| target.as_str() == Some("bq20_ledger_projector_guard")));
    // EventLog 与 Query 是整 crate 分片（targets = null），新夹具自动纳入。
    assert!(shards["kiana-eventlog"]["targets"].is_null());
    assert!(shards["kiana-query"]["targets"].is_null());
}

/// 防止 digest helper 漂移的固定值：`digest('f')` 必须仍是 sha256 形状。
#[test]
fn digest_helper_stays_sha256_shaped() {
    let value = digest('f');
    assert!(value.starts_with("sha256:"));
    assert_eq!(value.len(), 71);
}
