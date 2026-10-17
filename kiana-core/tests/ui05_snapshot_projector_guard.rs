//! UI-05 源码守卫：检查 snapshot projector 的单游标、owner/retention/lag 拒绝条件、projector
//! health/rebuild 合同、分页限制与只读边界。
//!
//! `include_str!` 把 Domain projector、Daemon facade、Protocol DTO、本地 health/rebuild 夹具、手动 lane
//! 和 baseline 读成文本，再检查 marker 与夹具/分片接线。该守卫不调用 `ui_snapshot_page()`、不读取
//! 持久事件、不验证 query.owner_id 是否绑定认证主体，也不证明游标摘要不可伪造；这些调用/身份关系
//! 不能由字符串存在性断言替代。

#[path = "support/source_guard.rs"]
mod source_guard;

#[test]
fn snapshot_projector_keeps_cursor_retention_lag_and_owner_boundaries() {
    // 源码守卫跨四个文档/实现面找 marker；它不是把这些模块在本测试里实际连接起来。
    let domain = include_str!("../../kiana-domain/src/ui_snapshot.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let protocol = include_str!("../../kiana-protocol/src/ui_contracts.rs");
    let baseline = include_str!("../../docs/roadmap/ui05-snapshot-projector-baseline.md");
    // 第一组 marker 约束 DTO、拒绝原因和 Daemon/Protocol 投影接口名称仍可在源码中找到。
    for marker in [
        "UiSnapshotQuery",
        "UiSnapshotPageCursor",
        "UiSnapshotPage",
        "project_ui_snapshot",
        "source_cursor",
        "projection_cursor",
        "projection_lag",
        "retention_floor",
        "retention_protected_pending",
        "stable",
        "offset",
        "cursor_digest",
        "ui_snapshot_session_owner_mismatch",
        "ui_snapshot_source_cursor_mismatch",
        "ui_snapshot_cursor_source_mismatch",
        "ui_snapshot_cursor_generation_mismatch",
        "UiSnapshotV1",
        "persisted_events",
    ] {
        assert!(
            domain.contains(marker) || daemon.contains(marker) || protocol.contains(marker),
            "UI-05 source marker missing: {marker}"
        );
    }
    // 第二组 marker 约束 health/rebuild 读路径与 limitation 编码；缺任一项都说明收口被回退。
    for marker in [
        "UiSnapshotProjectorStatus",
        "UiSnapshotProjectorHealthQuery",
        "UiSnapshotProjectorHealth",
        "project_ui_snapshot_health",
        "UiSnapshotRebuild",
        "rebuild_ui_snapshot_projection",
        "ui_snapshot_projector_health",
        "ui_snapshot_projection_lagging",
        "ui_snapshot_page_partial",
        "ui_snapshot_retention_floor_applied",
        "ui_snapshot_health_status_inconsistent",
        "ui_snapshot_rebuild_inconsistent",
        "ui_snapshot_projection_unknown",
    ] {
        assert!(
            domain.contains(marker) || daemon.contains(marker) || protocol.contains(marker),
            "UI-05 health/rebuild marker missing: {marker}"
        );
    }
    // 第三组只检查 baseline 写下的范围/证据术语仍保留，不验证 baseline 与实际实现完全一致。
    for marker in [
        "atomic",
        "snapshot",
        "分页",
        "retention",
        "lag",
        "owner",
        "feature_status",
        "proof_level",
        "source",
    ] {
        assert!(
            baseline.contains(marker),
            "UI-05 baseline marker missing: {marker}"
        );
    }
    assert!(daemon.contains("ui_snapshot_projection_unknown"));
    // DaemonHost is the single composition root and may construct the capability broker there;
    // the UI-05 requirement is that the snapshot read paths themselves stay read-only
    // projections, so the negative checks are scoped to those three items.
    for signature in [
        "pub async fn ui_snapshot_page(",
        "pub async fn ui_snapshot_projector_health(",
        "pub async fn rebuild_ui_snapshot_projection(",
    ] {
        let projector = source_guard::item_source(daemon, signature);
        assert!(projector.contains("persisted_events"));
        assert!(
            !projector.contains("CapabilityBroker"),
            "UI-05 read path {signature} must not construct a capability broker"
        );
        assert!(
            !projector.contains(".append(") && !projector.contains("commit_transition"),
            "UI-05 read path {signature} must not append facts"
        );
    }
    assert!(
        source_guard::item_source(daemon, "pub async fn ui_snapshot_page(")
            .contains("project_ui_snapshot")
    );
}

#[test]
fn ui05_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shards() {
    // 夹具、lane 与分片接线一起绑定：源码里存在功能但没有被 CI 执行不算收口。
    let domain_fixture = include_str!("../../kiana-domain/tests/ui05_snapshot_projector.rs");
    let daemon_fixture = include_str!("../../kiana-daemon/tests/ui05_snapshot_page.rs");
    let baseline = include_str!("../../docs/roadmap/ui05-snapshot-projector-baseline.md");
    let lane = include_str!("../../.github/workflows/ui05-snapshot-projector.yml");
    let shards = include_str!("../../scripts/ci/test-shards.json");
    let projector = include_str!("../../kiana-domain/src/ui_snapshot.rs");

    for fixture in [
        "snapshot_is_atomic_stable_and_page_cursor_bound",
        "lag_retention_and_owner_boundaries_fail_closed",
        "empty_slice_projects_an_empty_page_without_a_next_cursor",
        "repeat_reads_and_cursor_replay_return_the_same_page_without_changing_facts",
        "page_limitations_expose_partial_pages_and_the_retention_floor",
        "pagination_boundaries_and_illegal_cursors_fail_closed",
        "projector_health_reports_lag_and_unknown_instead_of_an_empty_list",
        "rebuild_matches_the_page_and_recovers_after_projection_faults",
    ] {
        assert!(
            domain_fixture.contains(fixture) && baseline.contains(fixture),
            "UI-05 domain fixture missing from the fixture or the baseline: {fixture}"
        );
    }
    for fixture in [
        "ui_snapshot_page_projects_one_owner_scoped_slice_and_paginates_with_limitations",
        "ui_snapshot_page_refuses_lag_foreign_session_retention_and_stitched_cursors",
        "ui_snapshot_page_fails_closed_when_the_projection_source_cannot_be_read",
        "projector_health_reports_lag_and_rebuild_matches_the_page_for_the_same_cursor",
        "rebuild_from_a_restarted_host_over_the_same_ledger_returns_the_same_page",
    ] {
        assert!(
            daemon_fixture.contains(fixture) && baseline.contains(fixture),
            "UI-05 daemon fixture missing from the fixture or the baseline: {fixture}"
        );
    }
    // 手动 lane 必须保持 workflow_dispatch-only、只读权限，并执行三个 target。
    assert!(!lane.contains("push:") && !lane.contains("pull_request:"));
    assert!(lane.contains("workflow_dispatch:"));
    assert!(lane.contains("permissions:") && lane.contains("contents: read"));
    assert!(lane.contains("cargo fmt --all --check"));
    assert!(lane.contains("cargo test -p kiana-domain --test ui05_snapshot_projector"));
    assert!(lane.contains("cargo test -p kiana-daemon --test ui05_snapshot_page"));
    assert!(lane.contains("cargo test -p kiana-core --test ui05_snapshot_projector_guard"));
    // 自动分片册必须仍然覆盖 domain/core 两个 target；daemon 走整 crate 分片，自动发现新 target。
    assert!(shards.contains("\"ui05_snapshot_projector\""));
    assert!(shards.contains("\"ui05_snapshot_projector_guard\""));
    assert!(shards.contains("\"kiana-daemon\""));
    // 只读边界：域 projector 不写事件、不读文件、不起网络、不执行副作用。
    for forbidden in [
        "commit_transition",
        "std::fs",
        "std::net",
        "tokio::net",
        "reqwest",
        "std::process",
        "CapabilityBroker",
        "EventStorePort",
        "unsafe",
    ] {
        assert!(
            !projector.contains(forbidden),
            "UI-05 domain projector must stay read-only; found {forbidden}"
        );
    }
    assert!(!domain_fixture.contains("#[ignore"));
}
