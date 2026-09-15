#[test]
fn notification_baseline_keeps_stream_and_inbox_as_projections() {
    let platform = include_str!("../src/platform.rs");
    let run_stream = include_str!("../../kiana-daemon/src/run_stream.rs");
    let web = include_str!("../../kiana-entrypoints/src/web.rs");
    let workbench = include_str!("../../kiana-entrypoints/src/workbench_chat.rs");
    let domain = include_str!("../../kiana-domain/src/platform.rs");

    assert!(domain.contains("HumanInboxItem"));
    assert!(platform.contains("pending_approvals"));
    assert!(platform.contains("company_snapshot"));
    assert!(platform.contains("failure_incidents"));
    assert!(platform.contains("HumanAction"));
    assert!(!platform.contains("NotificationStore"));
    assert!(run_stream.contains("broadcast::channel"));
    assert!(run_stream.contains("has_gap"));
    assert!(run_stream.contains("terminal"));
    assert!(!run_stream.contains("DeliveryChannel"));
    assert!(web.contains("/api/events"));
    assert!(web.contains("/api/receipt"));
    assert!(web.contains("snapshot_required_after_stream_gap"));
    assert!(workbench.contains("ChatMessage"));
    assert!(workbench.contains("不是 EventLog 或 Receipt 的替代品"));
}

#[test]
fn notification_baseline_records_the_missing_durable_components() {
    let baseline = include_str!("../../docs/roadmap/notifications-baseline.md");
    for required in [
        "durable NotificationStore",
        "read state",
        "DeliveryWorker",
        "RunStreamBus",
        "HumanInboxItem",
        "proof ceiling",
        "NM-01",
    ] {
        assert!(baseline.contains(required), "baseline missing {required}");
    }
}
