//! CM-35 外部资源合同的静态源码 marker 守卫。
//!
//! 测试读取 Domain、Daemon MCP/connectors 和 Query context adapter 源码字符串，确认信任范围、
//! scope、quota、撤销和来源引用等词面 marker 存在。它不执行 connector 请求，也不证明每个
//! marker 都位于同一条生产调用链。

#[test]
fn external_resource_is_untrusted_scoped_quota_bound_and_revocable() {
    let domain = include_str!("../../kiana-domain/src/external_resource.rs");
    let mcp = include_str!("../../kiana-daemon/src/harness_mcp.rs");
    let connectors = include_str!("../../kiana-daemon/src/connectors.rs");
    let context = include_str!("../../kiana-query/src/context_inputs.rs");

    // 每个 marker 可以来自四个文件中的任意一个，因此该循环只检查源码库存，不检查真实接线。
    for marker in [
        "ExternalResourceKind",
        "EXTERNAL_RESOURCE_REQUEST_SCHEMA",
        "EXTERNAL_RESOURCE_SNAPSHOT_SCHEMA",
        "processing_grant_digest",
        "quota_digest",
        "base_scope_digest",
        "requested_scope_digest",
        "allow_memory_write",
        "allow_scope_widening",
        "EvidenceStatus::Attributed",
        "revoked_at_ms",
        "readable_at",
        "data_epoch",
        "untrusted_data",
        "connector_binding_revoked",
        "mcp_project_untrusted",
        "SourceRef",
    ] {
        assert!(
            domain.contains(marker)
                || mcp.contains(marker)
                || connectors.contains(marker)
                || context.contains(marker),
            "CM-35 source marker missing: {marker}"
        );
    }

    // 下面两项确认 Domain 源码保留了特定拒绝条件；它们不是运行时撤销行为测试。
    assert!(domain.contains("requested_scope_digest != self.base_scope_digest"));
    assert!(domain.contains("self.revoked_at_ms.is_none()"));
    assert!(!domain.contains("CapabilityBroker"));
}
