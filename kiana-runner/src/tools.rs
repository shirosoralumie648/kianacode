//! 模型可见工具目录及其能力请求映射。
//!
//! 工具名和部分 item 形状参考 OpenAI Codex 的公开协议；Kiana 的安全边界仍由自身
//! ControlPlane 和 Broker 定义。Harness 只暴露五个固定工具：`shell`、`apply_patch`、
//! `mcp`、`memory.search`、`memory.write`。模型调用在这里仅被转换为
//! [`CapabilityRequest`]，不会直接执行命令、写文件、访问 MCP 或读写记忆。

use crate::model::ModelToolCall;
use kiana_domain::{CapabilityKind, CapabilityRequest, RequestId, RiskLevel};
use serde_json::{json, Value};

/// 模型可见的 shell 工具名称。
pub const TOOL_SHELL: &str = "shell";
/// 模型可见的补丁工具名称。
pub const TOOL_APPLY_PATCH: &str = "apply_patch";
/// 模型可见的 MCP 工具名称。
pub const TOOL_MCP: &str = "mcp";
/// 记忆搜索工具名称。
pub const TOOL_MEMORY_SEARCH: &str = "memory.search";
/// 记忆写入工具名称。
pub const TOOL_MEMORY_WRITE: &str = "memory.write";

/// 返回五个固定模型工具的 JSON schema。
///
/// schema 只约束模型输出形状；每个字段仍会在能力映射、策略、审批和 Broker 中再次
/// 校验。新增工具必须同时更新角色目录、策略映射和拒绝测试，不能只在这里追加 schema。
pub fn tool_schemas() -> Vec<Value> {
    vec![
        json!({
            "name": TOOL_SHELL,
            "description": "Run a shell command in the project sandbox. Codex-compatible command_execution item. Linux commands are confined with bubblewrap; read-only cannot write the workspace.",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": {
                        "anyOf": [
                            { "type": "string" },
                            { "type": "array", "items": { "type": "string" } }
                        ]
                    },
                    "workdir": { "type": "string" },
                    "timeout_ms": { "type": "integer", "minimum": 1 }
                },
                "required": ["command"]
            }
        }),
        json!({
            "name": TOOL_APPLY_PATCH,
            "description": "Apply a file patch in the project workspace. Codex-compatible file_change item.",
            "parameters": {
                "type": "object",
                "properties": {
                    "patch": { "type": "string" },
                    "path": { "type": "string" }
                },
                "required": ["patch"]
            }
        }),
        json!({
            "name": TOOL_MCP,
            "description": "Call a configured MCP server tool through the daemon broker. stdio only. Untrusted projects are denied.",
            "parameters": {
                "type": "object",
                "properties": {
                    "server": { "type": "string" },
                    "tool": { "type": "string" },
                    "tool_name": { "type": "string" },
                    "arguments": { "type": "object" }
                },
                "required": ["tool"]
            }
        }),
        json!({
            "name": TOOL_MEMORY_SEARCH,
            "description": "Search a Kiana memory collection through the daemon broker. Hits include layer, collection, and source. Unsourced hits are not verified conclusions.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "collection": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1 }
                },
                "required": ["query"]
            }
        }),
        json!({
            "name": TOOL_MEMORY_WRITE,
            "description": "Write an explicit memory record to one collection. Instance scratch does not promote. Chat is never auto-ingested.",
            "parameters": {
                "type": "object",
                "properties": {
                    "collection": { "type": "string" },
                    "text": { "type": "string" },
                    "source": { "type": "string" },
                    "promote_to": { "type": "string" }
                },
                "required": ["collection", "text", "source"]
            }
        }),
    ]
}

/// 将一次模型工具调用映射成带风险等级的能力请求。
///
/// 只接受固定工具名及少量兼容别名；未知名称返回 `tool_unsupported`。映射会把 sandbox
/// 和 project_root 作为参数上下文带下去，但这两个字段不是授权凭证，真实边界仍由
/// ControlPlane/Broker 再次验证。shell 的风险只按 sandbox 粗分为只读或本地写，不能
/// 由此推断命令内部一定没有外部副作用。
pub fn capability_for_tool(
    call: &ModelToolCall,
    sandbox: &str,
    project_root: &str,
) -> Result<CapabilityRequest, String> {
    match call.name.as_str() {
        TOOL_SHELL | "bash" | "exec" | "command_execution" => Ok(CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Process,
            "shell.exec",
            json!({
                "command": call.arguments.get("command").cloned().unwrap_or(Value::Null),
                "workdir": call.arguments.get("workdir").cloned().unwrap_or(Value::Null),
                "timeout_ms": call.arguments.get("timeout_ms").cloned().unwrap_or(Value::Null),
                "call_id": call.id,
                "sandbox": sandbox,
                "project_root": project_root,
            }),
        )
        .with_risk(shell_risk(sandbox))),
        TOOL_APPLY_PATCH | "file_change" => Ok(CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Filesystem,
            "apply_patch",
            json!({
                "patch": call.arguments.get("patch").cloned().unwrap_or(Value::Null),
                "path": call.arguments.get("path").cloned().unwrap_or(Value::Null),
                "call_id": call.id,
                "sandbox": sandbox,
                "project_root": project_root,
            }),
        )
        .with_risk(RiskLevel::LocalWrite)),
        TOOL_MCP | "mcp.call" => Ok(CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Network,
            "mcp.call",
            json!({
                "server": call.arguments.get("server").cloned().unwrap_or(Value::Null),
                "tool": call.arguments.get("tool").cloned().unwrap_or(
                    call.arguments.get("tool_name").cloned().unwrap_or(Value::Null)
                ),
                "arguments": call.arguments.get("arguments").cloned().unwrap_or(json!({})),
                "call_id": call.id,
                "sandbox": sandbox,
                "project_root": project_root,
            }),
        )
        .with_risk(RiskLevel::ExternalSideEffect)),
        TOOL_MEMORY_SEARCH => Ok(CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Query,
            TOOL_MEMORY_SEARCH,
            json!({
                "query": call.arguments.get("query").cloned().unwrap_or(Value::Null),
                "collection": call.arguments.get("collection").cloned().unwrap_or(Value::Null),
                "limit": call.arguments.get("limit").cloned().unwrap_or(Value::Null),
                "call_id": call.id,
                "sandbox": sandbox,
                "project_root": project_root,
            }),
        )
        .with_risk(RiskLevel::ReadOnly)),
        TOOL_MEMORY_WRITE => Ok(CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Filesystem,
            TOOL_MEMORY_WRITE,
            json!({
                "collection": call.arguments.get("collection").cloned().unwrap_or(Value::Null),
                "text": call.arguments.get("text").cloned().unwrap_or(Value::Null),
                "source": call.arguments.get("source").cloned().unwrap_or(Value::Null),
                "promote_to": call.arguments.get("promote_to").cloned().unwrap_or(Value::Null),
                "call_id": call.id,
                "sandbox": sandbox,
                "project_root": project_root,
            }),
        )
        .with_risk(RiskLevel::LocalWrite)),
        // 未知工具不做模糊匹配或通用回退，保持固定工具面和 fail-closed 行为。
        other => Err(format!("tool_unsupported:{other}")),
    }
}

/// 将 sandbox 字符串转换为 shell 的初步风险等级。
///
/// 只有精确的 `workspace-write` 视为 LocalWrite，其他值按只读处理；这只是请求构造时的
/// 初步分类，ControlPlane、grant 和实际执行器必须继续验证 sandbox 是否有效。
fn shell_risk(sandbox: &str) -> RiskLevel {
    match sandbox {
        "workspace-write" => RiskLevel::LocalWrite,
        _ => RiskLevel::ReadOnly,
    }
}

#[cfg(test)]
mod tests {
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
    fn unknown_tool_names_fail_closed_without_alias_or_shell_fallback() {
        // 固定工具面拒绝近似名和注入式名称，不能把未知输入降级成 shell 或其他能力。
        for name in [
            "shell.exec",
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
