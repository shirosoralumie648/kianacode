//! UI-05 源码守卫：检查 snapshot projector 的单游标、owner/retention/lag 拒绝条件和分页合同。
//!
//! `include_str!` 把 Domain projector、Daemon facade、Protocol DTO 和 baseline 读成文本，再检查 marker。
//! 该守卫不调用 `ui_snapshot_page()`、不读取持久事件、不验证 query.owner_id 是否绑定认证主体，也不
//! 证明游标摘要不可伪造；这些调用/身份关系不能由字符串存在性断言替代。

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
    // 第二组只检查 baseline 写下的范围/证据术语仍保留，不验证 baseline 与实际实现完全一致。
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
    // the UI-05 requirement is that the snapshot projector entry point itself stays a read-only
    // projection, so the negative check is scoped to that item.
    let projector = source_guard::item_source(daemon, "pub async fn ui_snapshot_page(");
    assert!(projector.contains("project_ui_snapshot"));
    assert!(!projector.contains("CapabilityBroker"));
}
