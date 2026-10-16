//! H09 工具目录的 Runner/Domain 行为合同测试。
//!
//! 这些用例在内存中构造 `ToolCatalogSnapshot` 和 `ModelToolCall`，实际调用 Domain validator
//! 与 Runner 的 `capability_for_tool()` 映射函数。它们覆盖未知名称拒绝、摘要变更拒绝和
//! mapper 与 descriptor 的字段一致性；不会运行完整 Harness、Provider 网络适配、Core 授权
//! 或 Broker handler，所以文件名中的“Broker resolve”不能理解为本文件真实执行了 Broker。
//! 相关生产路径继续读 `kiana-runner/src/tools.rs`、`harness.rs` 和
//! `kiana-domain/src/tool_authority.rs`。
use kiana_domain::{
    current_tool_catalog, CapabilityKind, ModelToolCall, ToolCatalogSnapshot, TOOL_APPLY_PATCH,
    TOOL_MCP, TOOL_MEMORY_SEARCH, TOOL_MEMORY_WRITE, TOOL_SHELL,
};
use kiana_runner::capability_for_tool;
use serde_json::json;

/// 为映射测试构造一个最小且形状正确的模型调用。
///
/// 不同工具需要不同必填参数，因此这里只提供各自的最小 JSON fixture；这些值不访问文件、
/// 网络或 memory store。未知名称走空对象分支，目的是验证名称拒绝而非参数业务行为。
fn minimal_call(name: &str) -> ModelToolCall {
    let arguments = match name {
        TOOL_SHELL => json!({"command":"pwd"}),
        TOOL_APPLY_PATCH => json!({"patch":"*** Begin Patch\n*** End Patch\n"}),
        TOOL_MCP => json!({"tool":"echo"}),
        TOOL_MEMORY_SEARCH => json!({"query":"catalog"}),
        TOOL_MEMORY_WRITE => {
            json!({"collection":"instance-scratch","text":"catalog","source":"explicit:h09"})
        }
        _ => json!({}),
    };
    ModelToolCall {
        id: format!("h09-{name}"),
        name: name.to_owned(),
        arguments,
    }
}

#[test]
/// 未登记的模型工具名必须失败，且两个 descriptor 不能声明相同 Provider wire name。
///
/// 第一部分调用 Runner mapper，确认未知工具得到 `tool_unsupported` 错误；第二部分篡改快照
/// 的 wire name，但没有重算 digest，所以实际先因摘要不匹配而被拒绝。它没有单独证明重复
/// wire name 在摘要有效时会命中对应分支，也没有真正发出 Provider 请求。
fn unadvertised_tool_and_wire_name_collision_are_rejected() {
    let unknown = ModelToolCall {
        id: "unknown".to_owned(),
        name: "shell.exec.injected".to_owned(),
        arguments: json!({"command":"echo nope"}),
    };
    assert_eq!(
        capability_for_tool(&unknown, "read-only", "/h09"),
        Err("tool_unsupported:shell.exec.injected".to_owned())
    );

    let mut catalog = current_tool_catalog();
    let second_wire = catalog.tools[1].wire_name.clone();
    catalog.tools[0].wire_name = second_wire;
    assert_eq!(
        catalog.validate(),
        Err("tool_catalog_snapshot_invalid".to_owned())
    );
}

#[test]
/// descriptor 改变后旧目录摘要校验失败，序列化快照仍带版本和摘要字段。
///
/// 此 fixture 只证明目录内容被修改时 snapshot digest 不再有效，并没有创建或消费真实
/// Approval。因此标题中的“不能复用旧 approval”依赖更上层把 catalog digest 纳入绑定来成立，
/// 本用例本身没有端到端重放审批。
fn catalog_change_cannot_reuse_old_approval() {
    let mut catalog = current_tool_catalog();
    catalog.tools[0].output_limit += 1;
    assert!(catalog.validate().is_err());
    let encoded = serde_json::to_value(ToolCatalogSnapshot::current()).unwrap();
    assert_eq!(encoded["schema"], "kiana.tool-catalog.v1");
    assert!(encoded["digest"].as_str().unwrap().starts_with("sha256:"));
}

#[test]
/// 检查五个规范模型名经 Runner mapper 后产生的 operation/capability 与 Domain descriptor 相同。
///
/// 这里比较的是内存对象，不会进入 Broker dispatch。它能发现 Runner 和 Domain 映射分叉，
/// 不能证明 CapabilityRequest 通过 Core 策略、approval 或实际 handler。
fn schema_mapper_and_broker_resolve_same_tool_version() {
    let catalog = current_tool_catalog();
    catalog.validate().unwrap();
    for name in [
        TOOL_SHELL,
        TOOL_APPLY_PATCH,
        TOOL_MCP,
        TOOL_MEMORY_SEARCH,
        TOOL_MEMORY_WRITE,
    ] {
        let descriptor = catalog.descriptor(name).unwrap();
        let request = capability_for_tool(&minimal_call(name), "read-only", "/h09").unwrap();
        assert_eq!(request.operation, descriptor.operation);
        assert_eq!(request.capability, descriptor.capability);
    }
    assert_eq!(
        catalog.descriptor(TOOL_SHELL).unwrap().capability,
        CapabilityKind::Process
    );
}
