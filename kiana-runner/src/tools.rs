//! Kiana Harness 的固定模型可见工具目录边界，以及模型工具调用到能力请求的转换器。
//!
//! 【本文件在系统中的位置】
//! ```text
//! ModelClient 返回 ModelOutput
//!       ↓
//! KianaHarness::model_step_once 校验工具调用并放入 pending_tools
//!       ↓
//! KianaHarness::emit_tool_request
//!       ↓
//! 【本文件】工具 schema 校验 + 参数映射 + capability/operation/risk 绑定
//!       ↓
//! RunnerEvent::CapabilityRequested
//!       ↓
//! ControlPlane policy / gate / approval → Capability Broker → handler
//! ```
//!
//! `tool_schemas()` 从 `kiana-domain` 取得当前目录，并传给模型作为可见工具描述；本文件
//! 提供这份描述对应的**反向转换**：接收模型返回的 `ModelToolCall`，根据服务端工具目录
//! 映射为 `CapabilityRequest`。因此 schema 和转换器必须作为一组理解：前者告诉模型可以
//! 提交什么形状的申请，后者在可信边界处再次校验它实际提交了什么。
//!
//! Harness 只暴露五个固定模型工具：`shell`、`apply_patch`、`mcp`、`memory.search`、
//! `memory.write`。名称及部分消息项形状参考 OpenAI Codex 的公开协议；Kiana 的授权边界
//! 仍由自身 ControlPlane 和 Broker 定义。这里**只构造请求，不执行命令、不写文件、不调用
//! MCP handler，也不读写记忆**。即使请求带有 sandbox、project_root 和风险字段，它们仍是
//! 申请上下文，Core/Broker 必须继续按服务端权威校验。

use crate::model::ModelToolCall;
use kiana_domain::{CapabilityRequest, RequestId};
use serde_json::{json, Value};

pub use kiana_domain::{
    tool_schemas, validate_tool_arguments, TOOL_APPLY_PATCH, TOOL_MCP, TOOL_MEMORY_SEARCH,
    TOOL_MEMORY_WRITE, TOOL_SHELL,
};
#[cfg(test)]
use kiana_domain::{validate_schema_value, CapabilityKind, RiskLevel};

/// 将一次模型工具调用映射成带服务端 request id 的能力请求。
///
/// 【调用者】主要由 `KianaHarness::emit_tool_request` 调用；恢复 checkpoint 时也会调用它
/// 重验待处理调用的形状。旧式或一次性调用方可使用 `capability_for_tool` 包装器。
///
/// 【输入】`call` 是模型生成的工具名称、assistant call id 和 JSON 参数；这三者必须视为
/// 不可信输入。`request_id` 是 Harness/Core 侧派生的稳定 invocation 身份，与模型返回的
/// `call.id` 不同；后者用于把结果配回 assistant 工具项，前者用于审批、派发和恢复绑定。
/// `sandbox` 与 `project_root` 是服务端提供的请求上下文，但自身不是路径或操作授权。
///
/// 【核心流程与输出】先验证当前服务端工具目录，再要求名字精确匹配目录的 canonical name，
/// 按 schema 校验参数，依工具类型把必要字段放入 `CapabilityRequest.arguments`，最后依据
/// operation 与参数计算风险等级并返回请求。`KianaHarness` 把它包装成
/// `RunnerEvent::CapabilityRequested` 交回 Core；Core 才执行策略、审批和 Broker 派发。
///
/// 【为什么不直接执行/使用通用回退】模型输出可能伪造工具名、字段或权限上下文；本函数
/// 只允许固定目录中明确定义的五类操作。未知名称、schema 错误或风险元数据不一致都返回错误，
/// 由 Harness 发出失败事件；不把未知操作降级成 `shell`，也不在这里重试或产生外部副作用。
/// shell 的风险字段只是目录要求的最低分类，不能据此推断 shell 命令内容没有网络或其他副作用。
pub fn capability_for_tool(
    call: &ModelToolCall,
    sandbox: &str,
    project_root: &str,
) -> Result<CapabilityRequest, String> {
    capability_for_tool_with_request_id(call, RequestId::new(), sandbox, project_root)
}

/// 用调用方提供的服务端稳定身份，把模型工具调用映射为能力请求。
///
/// 【为什么有 request id 版本】审批记录、事件、恢复检查点都指向同一次 invocation，不能在
/// 每次转换时重新生成 request id。Harness 因此从 run/turn/step 和 assistant call 身份派生 ID，
/// 再把同一个值传给发出请求与恢复校验路径；只有不需要跨阶段保持身份的一次性旧调用才用
/// `capability_for_tool` 随机生成 request id。
///
/// 【失败情况】工具目录自身无效、模型名称不在固定可见面、参数不符合 domain schema，或
/// operation/参数无法计算合法风险时返回稳定错误字符串。调用方将错误变成 `Failed` 事件，
/// 不会把它解释为 Broker 已执行失败，因为此函数尚未派发任何请求。
pub fn capability_for_tool_with_request_id(
    call: &ModelToolCall,
    request_id: RequestId,
    sandbox: &str,
    project_root: &str,
) -> Result<CapabilityRequest, String> {
    let catalog = kiana_domain::current_tool_catalog();
    catalog.validate()?;
    let descriptor = catalog
        .descriptor(&call.name)
        .ok_or_else(|| format!("tool_unsupported:{}", call.name))?;
    // 目录可以为 Broker 保留兼容别名，但它们不因此成为模型可见工具。
    // 模型调用必须使用 schema 中公布的 canonical name；否则即使 descriptor 能查到别名，
    // 这里仍拒绝，避免悄悄扩大模型工具面或让 alias 绕过针对 canonical name 的 schema。
    if descriptor.name != call.name {
        return Err(format!("tool_unsupported:{}", call.name));
    }
    let canonical = descriptor.name.as_str();
    validate_tool_arguments(&call.name, &call.arguments)?;

    // Required/optional 字段约束由上面的 domain schema validator 统一负责。
    // 这里构造各 handler 约定的稳定参数对象：可选标量缺失时保留为 JSON null，MCP 的
    // arguments 缺失时则使用空对象。request id、模型 call id 和授权上下文分别承担不同身份职责。
    let mut request = match canonical {
        // shell 映射到进程能力。`command` 是模型请求运行的命令，`workdir` 和 `timeout_ms`
        // 是执行上下文；`project_root` / `sandbox` 仍需 Broker 结合真实 PathLock 与 profile 校验。
        TOOL_SHELL => CapabilityRequest::new(
            request_id,
            descriptor.capability.clone(),
            descriptor.operation.clone(),
            json!({
                "command": call.arguments.get("command").cloned().unwrap_or(Value::Null),
                "workdir": call.arguments.get("workdir").cloned().unwrap_or(Value::Null),
                "timeout_ms": call.arguments.get("timeout_ms").cloned().unwrap_or(Value::Null),
                "call_id": call.id,
                "sandbox": sandbox,
                "project_root": project_root,
            }),
        ),
        // apply_patch 把补丁文本和目标路径交给文件系统能力 handler；本分支只打包参数，
        // 不应用补丁，也不判断 WorkPacket 是否允许写该路径。
        TOOL_APPLY_PATCH => CapabilityRequest::new(
            request_id,
            descriptor.capability.clone(),
            descriptor.operation.clone(),
            json!({
                "patch": call.arguments.get("patch").cloned().unwrap_or(Value::Null),
                "path": call.arguments.get("path").cloned().unwrap_or(Value::Null),
                "call_id": call.id,
                "sandbox": sandbox,
                "project_root": project_root,
            }),
        ),
        // MCP 请求需要同时标明服务端配置中的 server、该服务器上的工具名和嵌套参数。
        // server 名不是模型提供的网络地址；真正选择已配置连接和检查远程权限属于下游。
        TOOL_MCP => CapabilityRequest::new(
            request_id,
            descriptor.capability.clone(),
            descriptor.operation.clone(),
            json!({
                "server": call.arguments.get("server").cloned().unwrap_or(Value::Null),
                // 新参数名为 `tool`；保留 `tool_name` 作为同一字段的输入兼容形式，
                // 但最终请求始终输出单一 canonical key，避免下游需要处理两种结构。
                "tool": call.arguments.get("tool").cloned().unwrap_or(
                    call.arguments.get("tool_name").cloned().unwrap_or(Value::Null)
                ),
                "arguments": call.arguments.get("arguments").cloned().unwrap_or(json!({})),
                "call_id": call.id,
                "sandbox": sandbox,
                "project_root": project_root,
            }),
        ),
        // memory.search 是查询请求。query 决定搜索内容，collection/limit 用于缩小返回范围；
        // 空字段保留成 null，由后续 memory handler 按它自己的契约解释。
        TOOL_MEMORY_SEARCH => CapabilityRequest::new(
            request_id,
            descriptor.capability.clone(),
            descriptor.operation.clone(),
            json!({
                "query": call.arguments.get("query").cloned().unwrap_or(Value::Null),
                "collection": call.arguments.get("collection").cloned().unwrap_or(Value::Null),
                "limit": call.arguments.get("limit").cloned().unwrap_or(Value::Null),
                "call_id": call.id,
                "sandbox": sandbox,
                "project_root": project_root,
            }),
        ),
        // memory.write 提交待写入文本及来源/晋升提示。它是有写入风险的申请，实际目标范围、
        // 来源可信度和是否允许晋升仍由服务端 memory policy 与 Broker handler 再次判断。
        TOOL_MEMORY_WRITE => CapabilityRequest::new(
            request_id,
            descriptor.capability.clone(),
            descriptor.operation.clone(),
            json!({
                "collection": call.arguments.get("collection").cloned().unwrap_or(Value::Null),
                "text": call.arguments.get("text").cloned().unwrap_or(Value::Null),
                "source": call.arguments.get("source").cloned().unwrap_or(Value::Null),
                "promote_to": call.arguments.get("promote_to").cloned().unwrap_or(Value::Null),
                "call_id": call.id,
                "sandbox": sandbox,
                "project_root": project_root,
            }),
        ),
        // 目录以外的名称不做模糊匹配或通用回退，保持固定工具面和 fail-closed 行为。
        _ => return Err(format!("tool_unsupported:{}", call.name)),
    };
    // 风险不是由模型自报：由 domain action descriptor 按 operation 与最终参数重新计算。
    // 这仍只是请求侧的最低风险元数据，后续 Core/Broker 会结合权限、Gate 和批准状态再裁决。
    request.risk =
        kiana_domain::capability_action_minimum_risk(&request.operation, &request.arguments)
            .map_err(str::to_owned)?;
    Ok(request)
}

#[cfg(test)]
mod tests {
    // 这些测试只锁定 Harness 转换器的本地契约：固定工具名、参数形状、上下文透传和 fail-closed。
    // 它们不执行 shell/patch/MCP/memory，也不能单独证明 ControlPlane 授权或 Broker 行为。
    use super::*;
    use crate::model::ModelToolCall;

    #[test]
    fn shell_in_read_only_sandbox_is_not_an_unattended_side_effect() {
        let request = capability_for_tool(
            &ModelToolCall {
                id: "c1".to_owned(),
                name: TOOL_SHELL.to_owned(),
                arguments: json!({ "command": "ls" }),
            },
            "read-only",
            "/repo",
        )
        .unwrap();
        assert_eq!(request.capability, CapabilityKind::Process);
        assert_eq!(request.operation, "shell.exec");
        assert_eq!(request.risk, RiskLevel::ReadOnly);
        assert_eq!(request.arguments["project_root"], "/repo");
    }

    #[test]
    fn shell_timeout_ms_is_forwarded_into_the_capability() {
        let request = capability_for_tool(
            &ModelToolCall {
                id: "c-timeout".to_owned(),
                name: TOOL_SHELL.to_owned(),
                arguments: json!({ "command": "sleep 8", "timeout_ms": 250 }),
            },
            "read-only",
            "/repo",
        )
        .unwrap();
        assert_eq!(request.arguments["timeout_ms"], 250);
        assert_eq!(request.arguments["command"], "sleep 8");
    }

    #[test]
    fn apply_patch_is_a_local_write() {
        let request = capability_for_tool(
            &ModelToolCall {
                id: "c2".to_owned(),
                name: TOOL_APPLY_PATCH.to_owned(),
                arguments: json!({ "patch": "*** Begin Patch" }),
            },
            "workspace-write",
            "/repo",
        )
        .unwrap();
        assert_eq!(request.capability, CapabilityKind::Filesystem);
        assert_eq!(request.risk, RiskLevel::LocalWrite);
        assert_eq!(request.arguments["project_root"], "/repo");
    }

    #[test]
    fn mcp_is_an_external_side_effect_on_the_network_broker() {
        let request = capability_for_tool(
            &ModelToolCall {
                id: "c-mcp".to_owned(),
                name: TOOL_MCP.to_owned(),
                arguments: json!({
                    "server": "mock",
                    "tool": "echo",
                    "arguments": { "message": "hello" }
                }),
            },
            "workspace-write",
            "/repo",
        )
        .unwrap();
        assert_eq!(request.capability, CapabilityKind::Network);
        assert_eq!(request.operation, "mcp.call");
        assert_eq!(request.risk, RiskLevel::ExternalSideEffect);
        assert_eq!(request.arguments["server"], "mock");
        assert_eq!(request.arguments["tool"], "echo");
        assert_eq!(request.arguments["arguments"]["message"], "hello");
    }

    #[test]
    fn memory_search_is_a_readonly_query() {
        let request = capability_for_tool(
            &ModelToolCall {
                id: "c-mem".to_owned(),
                name: TOOL_MEMORY_SEARCH.to_owned(),
                arguments: json!({ "query": "acceptance", "collection": "project" }),
            },
            "workspace-write",
            "/repo",
        )
        .unwrap();
        assert_eq!(request.capability, CapabilityKind::Query);
        assert_eq!(request.operation, TOOL_MEMORY_SEARCH);
        assert_eq!(request.risk, RiskLevel::ReadOnly);
        assert_eq!(request.arguments["query"], "acceptance");
        assert_eq!(request.arguments["collection"], "project");
    }

    #[test]
    fn memory_write_is_a_local_write() {
        let request = capability_for_tool(
            &ModelToolCall {
                id: "c-mem-w".to_owned(),
                name: TOOL_MEMORY_WRITE.to_owned(),
                arguments: json!({
                    "collection": "instance-scratch",
                    "text": "draft",
                    "source": "explicit:test"
                }),
            },
            "workspace-write",
            "/repo",
        )
        .unwrap();
        assert_eq!(request.capability, CapabilityKind::Filesystem);
        assert_eq!(request.operation, TOOL_MEMORY_WRITE);
        assert_eq!(request.risk, RiskLevel::LocalWrite);
    }

    #[test]
    fn missing_required_tool_argument_fails_during_mapping_with_stable_error() {
        let call = ModelToolCall {
            id: "missing-command".to_owned(),
            name: TOOL_SHELL.to_owned(),
            arguments: json!({}),
        };

        assert_eq!(
            capability_for_tool(&call, "read-only", "/repo").unwrap_err(),
            "invalid_arguments:shell:command"
        );
    }

    #[test]
    fn wrong_tool_argument_type_fails_during_mapping_with_stable_error() {
        let call = ModelToolCall {
            id: "numeric-command".to_owned(),
            name: TOOL_SHELL.to_owned(),
            arguments: json!({ "command": 42 }),
        };

        assert_eq!(
            capability_for_tool(&call, "read-only", "/repo").unwrap_err(),
            "invalid_arguments:shell:command"
        );
    }

    #[test]
    fn extra_tool_argument_is_allowed_for_existing_cassette_compatibility() {
        let call = ModelToolCall {
            id: "extra-field".to_owned(),
            name: TOOL_SHELL.to_owned(),
            arguments: json!({
                "command": "ls",
                "cassette_extension": { "metadata": true }
            }),
        };

        let request = capability_for_tool(&call, "read-only", "/repo").unwrap();
        assert_eq!(request.arguments["command"], "ls");
        assert!(request.arguments.get("cassette_extension").is_none());
    }

    #[test]
    fn all_five_model_visible_tools_accept_minimal_arguments() {
        let cases = [
            (TOOL_SHELL, json!({ "command": "ls" })),
            (
                TOOL_APPLY_PATCH,
                json!({ "patch": "*** Begin Patch\n*** End Patch\n" }),
            ),
            (TOOL_MCP, json!({ "tool": "echo" })),
            (TOOL_MEMORY_SEARCH, json!({ "query": "acceptance" })),
            (
                TOOL_MEMORY_WRITE,
                json!({
                    "collection": "instance-scratch",
                    "text": "draft",
                    "source": "explicit:test"
                }),
            ),
        ];

        for (name, arguments) in cases {
            validate_tool_arguments(name, &arguments)
                .unwrap_or_else(|error| panic!("{name} minimal arguments rejected: {error}"));
        }
    }

    #[test]
    fn minimum_constraint_rejects_out_of_range_tool_argument() {
        assert_eq!(
            validate_tool_arguments(TOOL_SHELL, &json!({ "command": "ls", "timeout_ms": 0 })),
            Err("invalid_arguments:shell:timeout_ms".to_owned())
        );
    }

    #[test]
    fn schema_validator_supports_enum_constraints() {
        let schema = json!({
            "type": "string",
            "enum": ["read-only", "workspace-write"]
        });

        assert!(validate_schema_value(&json!("read-only"), &schema).is_ok());
        assert_eq!(
            validate_schema_value(&json!("danger-full-access"), &schema),
            Err("<arguments>".to_owned())
        );
    }

    #[test]
    fn unknown_tool_names_fail_closed_without_alias_or_shell_fallback() {
        // 固定工具面拒绝近似名和注入式名称，不能把未知输入降级成 shell 或其他能力。
        for name in [
            "shell.exec",
            "bash",
            "exec",
            "command_execution",
            "shell;rm -rf /",
            "memory.search.extra",
            "unknown",
        ] {
            let call = ModelToolCall {
                id: "unknown-call".to_owned(),
                name: name.to_owned(),
                arguments: json!({ "command": "echo should-not-run" }),
            };
            assert_eq!(
                capability_for_tool(&call, "read-only", "/repo").unwrap_err(),
                format!("tool_unsupported:{name}")
            );
        }
    }

    #[test]
    fn tool_schema_exposes_exactly_the_five_fixed_names() {
        // schema 列表是模型可见面的边界，名称数量和集合都必须保持冻结。
        let mut names = tool_schemas()
            .into_iter()
            .map(|schema| schema["name"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        names.sort();
        assert_eq!(
            names,
            [
                TOOL_APPLY_PATCH,
                TOOL_MCP,
                TOOL_MEMORY_SEARCH,
                TOOL_MEMORY_WRITE,
                TOOL_SHELL,
            ]
        );
    }
}
