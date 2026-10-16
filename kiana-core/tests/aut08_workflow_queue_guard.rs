//! AUT-08 源码文本守卫：确保 Core、Port 与 Memory adapter 保留队列 Lease/dispatch 的契约标记。
//!
//! `include_str!` 和字符串 contains 只检查源码文字是否存在或出现禁止名称，不调用 Lease 状态机，也不创建并发
//! worker。真正的 transition 行为在 Domain/EventLog fixture 中检查；该静态守卫不证明磁盘持久化或跨进程互斥。

#[test]
fn core_keeps_queue_lease_separate_from_capability_execution() {
    // 三个文件分别代表 Core 的交接门、Store trait 和当前进程内的语义 adapter。
    let source = include_str!("../src/workflow_queue.rs");
    let port = include_str!("../../kiana-ports/src/lib.rs");
    let adapter = include_str!("../../kiana-eventlog/src/workflow_queue.rs");
    for marker in [
        "validate_workflow_queue_dispatch",
        "workflow_queue_requires_recovery",
        "is_dispatchable",
        "WorkflowQueueLeaseStatus::ResultUnknown",
    ] {
        // 这些字符串只是防止关键边界名称意外删除，不代表它们每次测试时都被运行。
        assert!(
            source.contains(marker),
            "AUT-08 core marker missing: {marker}"
        );
    }
    for marker in [
        "trait WorkflowQueueStore",
        "async fn heartbeat",
        "async fn fence",
        "async fn reclaim",
        "workflow_queue_claim_unsupported",
    ] {
        assert!(
            port.contains(marker),
            "AUT-08 port marker missing: {marker}"
        );
    }
    for marker in [
        "MemoryWorkflowQueueStore",
        "workflow_queue_reclaim_effect_in_flight",
        "workflow_queue_recovery_required",
        "tokio::sync::Mutex",
    ] {
        assert!(
            adapter.contains(marker),
            "AUT-08 adapter marker missing: {marker}"
        );
    }
    assert!(!source.contains("CapabilityBrokerPort::execute"));
    assert!(!adapter.contains("CapabilityBrokerPort::execute"));
}
