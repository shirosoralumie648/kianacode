//! Tool catalog and capability mapping derived from OpenAI Codex (Apache-2.0)
//! `codex-rs/exec` item shapes and `codex-rs/protocol` shell/apply_patch tools.
//!
//! The harness owns the model-visible tool names. Execution stays on the
//! control plane: each call becomes a `CapabilityRequest` for the broker.

use crate::model::ModelToolCall;
use kiana_domain::{CapabilityKind, CapabilityRequest, RequestId, RiskLevel};
use serde_json::{json, Value};

pub const TOOL_SHELL: &str = "shell";
pub const TOOL_APPLY_PATCH: &str = "apply_patch";
pub const MAX_STEPS_PER_TURN: u32 = 32;

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
    ]
}

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
        other => Err(format!("tool_unsupported:{other}")),
    }
}

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
}
