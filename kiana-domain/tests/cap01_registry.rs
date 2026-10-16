//! CAP-01 的 Domain 合同测试：核对模型工具目录、操作规格和 Capability Action 描述符之间的静态映射。
//!
//! 这些用例直接调用 `kiana-domain` 的纯内存目录函数，不启动 Daemon、Runner 或 Broker，
//! 也不发送模型/Provider 请求。它们适合发现名称、schema、风险、能力类别和 descriptor
//! 投影之间的回归，但不能单独证明宿主启动、Core 授权或具体 handler 执行成功。
//! 继续追踪实现可读 `kiana-domain/src/tool_authority.rs`、`tool_catalog.rs` 和 `actions.rs`。
use kiana_domain::{
    action_result_schema, canonical_action_operation, capability_action_descriptor,
    model_tool_name, model_tool_result_schema, operator_only_action, tool_catalog_digest,
    tool_schemas, tool_spec, validate_action_catalog, validate_tool_action_binding,
    validate_tool_authority, CapabilityKind, RiskLevel, ToolCatalogSnapshot,
    ACTION_HANDLER_BINDING_VERSION, ACTION_OPERATIONS, OPERATION_SPECS, TOOL_CATALOG_SCHEMA,
    TOOL_CATALOG_VERSION, TOOL_SPECS,
};
use serde_json::{json, Value};

#[test]
/// 验证五个模型可见工具均有唯一的规范规格、模型 schema 和动作目录绑定。
///
/// 这里的“覆盖”指 Domain 元数据能彼此对应；不代表运行时授予了这五项能力。
fn tool_authority_covers_every_model_visible_tool() {
    validate_tool_authority().unwrap();
    // 这是当前安全边界允许模型直接提出的五个规范工具名；ACTION_OPERATIONS 的全集更大，
    // 其中还有只能由系统/操作员路径使用的内部操作，不能因此暴露给模型。
    let visible = [
        "shell",
        "apply_patch",
        "mcp",
        "memory.search",
        "memory.write",
    ];
    let schemas = tool_schemas();
    assert_eq!(schemas.len(), visible.len());
    // 同时检查模型名、兼容 alias、内部 operation、能力/风险元数据和 action binding version，
    // 防止只改 schema 名称却没有同步底层动作合同。
    for name in visible {
        let canonical = model_tool_name(name).expect("model tool mapping");
        let spec = tool_spec(name).unwrap();
        assert_eq!(spec.name, name);
        assert_eq!(canonical_action_operation(canonical), Some(spec.operation));
        assert!(ACTION_OPERATIONS.contains(&spec.operation));
        assert!(schemas.iter().any(|schema| schema["name"] == name));
        let descriptor = capability_action_descriptor(canonical).expect("action descriptor");
        assert_eq!(descriptor.operation, spec.operation);
        assert_eq!(descriptor.capability, spec.capability);
        assert_eq!(descriptor.minimum_risk, spec.risk_policy);
        assert_eq!(descriptor.binding_version, ACTION_HANDLER_BINDING_VERSION);
        for alias in spec.aliases {
            assert_eq!(model_tool_name(alias), Some(spec.name));
            assert_eq!(canonical_action_operation(alias), Some(spec.operation));
        }
    }
    assert_eq!(TOOL_SPECS.len(), visible.len());
}

#[test]
/// 用手工复制并篡改的 `ToolSpec` 检查 action binding 是否拒绝关键元数据漂移。
///
/// 测试只调用 Domain validator，没有改变全局静态目录；三个错误分别对应 operation、
/// capability 与 minimum risk 不一致。它不覆盖所有 descriptor 字段或实际请求授权。
fn tool_action_binding_rejects_controlled_metadata_drift() {
    let canonical = tool_spec("shell").unwrap();

    let mut operation_mismatch = (*canonical).clone();
    operation_mismatch.operation = "apply_patch";
    assert_eq!(
        validate_tool_action_binding(&operation_mismatch).unwrap_err(),
        "tool_action_operation_mismatch"
    );

    let mut capability_mismatch = (*canonical).clone();
    capability_mismatch.capability = CapabilityKind::Filesystem;
    assert_eq!(
        validate_tool_action_binding(&capability_mismatch).unwrap_err(),
        "tool_action_metadata_mismatch"
    );

    let mut risk_mismatch = (*canonical).clone();
    risk_mismatch.risk_policy = RiskLevel::Critical;
    assert_eq!(
        validate_tool_action_binding(&risk_mismatch).unwrap_err(),
        "tool_action_metadata_mismatch"
    );
}

#[test]
/// 确认标记为 operator-only 的操作不会拥有模型工具名称映射。
///
/// 这是静态目录关系检查，不会模拟操作者身份，也不代表 operator-only 操作的其他入口行为。
fn operator_only_capabilities_never_enter_model_schema() {
    for operation in ACTION_OPERATIONS {
        if operator_only_action(operation) {
            assert!(
                model_tool_name(operation).is_none(),
                "operator operation leaked: {operation}"
            );
        }
    }
}

#[test]
/// 对全部操作规格检查排序、完整性，以及五个模型工具 schema/目录 descriptor 的投影结果。
///
/// 38 个 operation 是内部动作目录规模，和当前五个模型可见工具不是同一概念。后半段只对
/// 模型可见的五项校验 wire name、别名、schema、风险、副作用和调度元数据。
fn operation_specs_preserve_model_schema_and_descriptor_projection() {
    // 该列表冻结 Domain 操作目录的当前完整顺序；测试用精确顺序比较能发现遗漏、重复或重排。
    let expected_operations = [
        "shell.exec",
        "apply_patch",
        "mcp.call",
        "mcp.discover",
        "memory.search",
        "memory.write",
        "context.repo_map",
        "context.index.read",
        "context.index.cache.write",
        "context.artifacts.read",
        "context.artifacts.cache.write",
        "context.artifact_store.read",
        "context.artifact_store.cache.write",
        "context.artifact_ingest.write",
        "context.artifact_graph.read",
        "context.artifact_readiness.read",
        "context.search",
        "context.vector_search",
        "context.pack",
        "memory.review",
        "data.governance",
        "extension.manage",
        "connector.manage",
        "connector.invoke",
        "connector.health",
        "connector.mcp_handshake",
        "apply_patch.preview",
        "workspace.checkpoint.restore",
        "workspace.transaction",
        "local.package",
        "execution.output.read",
        "process.start",
        "process.poll",
        "process.stdin",
        "process.resize",
        "process.stop",
        "environment.inspect",
        "tool.search",
    ];
    assert_eq!(ACTION_OPERATIONS.len(), 38);
    assert_eq!(ACTION_OPERATIONS, expected_operations.as_slice());
    assert_eq!(OPERATION_SPECS.len(), expected_operations.len());
    validate_action_catalog().unwrap();

    for (operation, spec) in expected_operations.iter().zip(OPERATION_SPECS) {
        assert_eq!(spec.operation, *operation);
        let descriptor = capability_action_descriptor(operation).unwrap();
        assert_eq!(descriptor.operation, spec.operation);
        assert_eq!(descriptor.binding_version, ACTION_HANDLER_BINDING_VERSION);
        assert_eq!(descriptor.capability, spec.capability);
        assert_eq!(descriptor.minimum_risk, spec.minimum_risk);
        assert_eq!(descriptor.resource_fields, spec.resource_fields);
        assert_eq!(descriptor.effect, spec.effect);
        assert_eq!(descriptor.cancellation, spec.cancellation);
        assert_eq!(descriptor.reconciliation, spec.reconciliation);
        assert_eq!(descriptor.idempotency, spec.idempotency);
        assert_eq!(descriptor.result_schema, action_result_schema());
        assert_eq!(operator_only_action(operation), spec.operator_only);
        if *operation == "process.start" {
            assert_eq!(descriptor.capability, CapabilityKind::Process);
            assert_eq!(descriptor.minimum_risk, RiskLevel::ReadOnly);
            assert_eq!(spec.risk_rule, kiana_domain::DynamicRiskRule::SandboxMode);
        }
        if *operation == "execution.output.read" {
            assert_eq!(descriptor.resource_fields, &["root"][..]);
            assert_eq!(descriptor.effect, "read");
        }
    }

    // 将模型 schema 作为独立期望值保存在测试中，避免实现端和测试端一起从同一个错误投影
    // 得到相同结果而漏掉回归。这里描述模型输入，不是内部 action 的结果合同。
    let expected_schemas: Vec<Value> = vec![
        json!({
            "name":"shell",
            "description":"Run a shell command in the project sandbox. Codex-compatible command_execution item. Linux commands are confined with bubblewrap; read-only cannot write the workspace.",
            "parameters":{"type":"object","properties":{
                "command":{"anyOf":[{"type":"string"},{"type":"array","items":{"type":"string"}}]},
                "workdir":{"type":"string"},
                "timeout_ms":{"type":"integer","minimum":1}
            },"required":["command"]}
        }),
        json!({
            "name":"apply_patch",
            "description":"Apply a file patch in the project workspace. Codex-compatible file_change item.",
            "parameters":{"type":"object","properties":{
                "patch":{"type":"string"},"path":{"type":"string"}
            },"required":["patch"]}
        }),
        json!({
            "name":"mcp",
            "description":"Call a configured MCP server tool through the daemon broker. stdio only. Untrusted projects are denied.",
            "parameters":{"type":"object","properties":{
                "server":{"type":"string"},"tool":{"type":"string"},
                "tool_name":{"type":"string"},"arguments":{"type":"object"}
            },"required":["tool"]}
        }),
        json!({
            "name":"memory.search",
            "description":"Search a Kiana memory collection through the daemon broker. Hits include layer, collection, and source. Unsourced hits are not verified conclusions.",
            "parameters":{"type":"object","properties":{
                "query":{"type":"string"},"collection":{"type":"string"},
                "limit":{"type":"integer","minimum":1}
            },"required":["query"]}
        }),
        json!({
            "name":"memory.write",
            "description":"Write an explicit memory record to one collection. Instance scratch does not promote. Chat is never auto-ingested.",
            "parameters":{"type":"object","properties":{
                "collection":{"type":"string"},"text":{"type":"string"},
                "source":{"type":"string"},"promote_to":{"type":"string"}
            },"required":["collection","text","source"]}
        }),
    ];
    assert_eq!(tool_schemas(), expected_schemas);

    // current() 将 OperationSpec、模型参数 schema 和结果 schema 组合成快照；validate() 检查
    // 其 digest 与 descriptor 形状，以下断言再逐字段核对公开合同。
    let snapshot = ToolCatalogSnapshot::current();
    snapshot.validate().unwrap();
    assert_eq!(snapshot.schema, TOOL_CATALOG_SCHEMA);
    assert_eq!(snapshot.version, TOOL_CATALOG_VERSION);
    assert_eq!(snapshot.digest, tool_catalog_digest());
    assert_eq!(snapshot.tools.len(), expected_schemas.len());
    // tuple 顺序与下方断言对应：规范名、wire 名、别名、capability、operation、风险、
    // 是否有副作用、resource 声明、max_parallelism。并行度只是 batch planner 元数据。
    let expected_tool_metadata = [
        (
            "shell",
            "shell",
            &["shell.exec", "bash", "exec", "command_execution"][..],
            CapabilityKind::Process,
            "shell.exec",
            RiskLevel::ReadOnly,
            true,
            "workspace",
            1,
        ),
        (
            "apply_patch",
            "apply_patch",
            &["file_change"][..],
            CapabilityKind::Filesystem,
            "apply_patch",
            RiskLevel::LocalWrite,
            true,
            "workspace",
            1,
        ),
        (
            "mcp",
            "mcp",
            &["mcp.call"][..],
            CapabilityKind::Network,
            "mcp.call",
            RiskLevel::ExternalSideEffect,
            true,
            "workspace",
            1,
        ),
        (
            "memory.search",
            "memory_search",
            &[][..],
            CapabilityKind::Query,
            "memory.search",
            RiskLevel::ReadOnly,
            false,
            "memory",
            4,
        ),
        (
            "memory.write",
            "memory_write",
            &[][..],
            CapabilityKind::Filesystem,
            "memory.write",
            RiskLevel::LocalWrite,
            true,
            "workspace",
            1,
        ),
    ];
    for ((descriptor, schema), expected) in snapshot
        .tools
        .iter()
        .zip(&expected_schemas)
        .zip(expected_tool_metadata)
    {
        assert_eq!(descriptor.name, schema["name"].as_str().unwrap());
        assert_eq!(descriptor.name, expected.0);
        assert_eq!(descriptor.wire_name, expected.1);
        assert_eq!(
            descriptor.aliases,
            expected
                .2
                .iter()
                .map(|alias| (*alias).to_owned())
                .collect::<Vec<_>>()
        );
        assert_eq!(descriptor.capability, expected.3);
        assert_eq!(descriptor.operation, expected.4);
        assert_eq!(descriptor.minimum_risk, expected.5);
        assert_eq!(descriptor.side_effecting, expected.6);
        assert_eq!(descriptor.argument_schema, schema["parameters"]);
        // 模型可见结果 schema 故意与统一 action result schema 区分；两者服务于不同边界，
        // 不能为了“看起来统一”就直接替换其中一个。
        assert_eq!(descriptor.result_schema, model_tool_result_schema());
        assert_eq!(
            descriptor.result_schema,
            json!({"type":"object","additionalProperties":true})
        );
        assert_ne!(descriptor.result_schema, action_result_schema());
        assert_eq!(descriptor.output_limit, 1024 * 1024);
        assert_eq!(descriptor.execution_mode, "brokered");
        assert_eq!(
            descriptor.replay_class,
            if expected.6 {
                "reconcile"
            } else {
                "replay_safe"
            }
        );
        assert_eq!(
            descriptor.scheduling,
            if expected.6 {
                "exclusive"
            } else {
                "parallel_read"
            }
        );
        assert_eq!(descriptor.resources, vec![expected.7.to_owned()]);
        assert_eq!(descriptor.max_parallelism, expected.8);
    }
}
