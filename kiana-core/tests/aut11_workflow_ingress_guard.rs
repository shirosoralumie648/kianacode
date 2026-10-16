//! AUT-11 的源码文本守卫：检查 Domain、Daemon 与 Core 文件仍包含事件到 Fire/evidence 的边界标记。
//!
//! `include_str!` 在编译测试时把源码当成字符串读取；下面的 contains/negative contains 不会调用 verifier、
//! ControlPlane 或 EventStore，也无法证明产品入口实际接线、签名可用或状态已持久化。它只防止关键源码标记
//! 被意外移除，以及 Domain/Daemon 文件出现本守卫列出的快捷执行标记。

#[test]
fn ingress_requires_existing_workflow_evidence_route() {
    // 静态读取目标文件，后续断言检索约定的职责/错误标记，而不是执行这些文件中的函数。
    let domain = include_str!("../../kiana-domain/src/workflow_event_ingress.rs");
    let daemon = include_str!("../../kiana-daemon/src/workflow_ingress.rs");
    let core = include_str!("../src/automation.rs");
    for marker in [
        "WorkflowEventIngress",
        "WorkflowEventSourcePolicy",
        "WorkflowEventOccurrence",
        "payload_filter",
        "occurrence_key",
        "to_fire_command",
    ] {
        // 缺少任一 marker 时，测试能提示约定名称已漂移；它不验证该分支的运行时语义。
        assert!(
            domain.contains(marker),
            "AUT-11 domain marker missing: {marker}"
        );
    }
    for marker in [
        "source_not_allowlisted",
        "signature_invalid",
        "dedupe_payload_conflict",
        "hmac::verify",
    ] {
        assert!(
            daemon.contains(marker),
            "AUT-11 daemon marker missing: {marker}"
        );
    }
    assert!(core.contains("workflow_event_reference_required"));
    assert!(core.contains("workflow_evidence_not_found"));
    // 负向断言阻止 Domain 直接拥有能力请求类型；真实安全边界仍需沿调用者和运行路径核验。
    assert!(!domain.contains("CapabilityRequest"));
    assert!(!daemon.contains("CapabilityBrokerPort::execute"));
}
