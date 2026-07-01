use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SDKMessage {
    #[serde(rename = "assistant")]
    Assistant(SDKAssistantMessage),
    #[serde(rename = "user")]
    User(SDKUserMessage),
    #[serde(rename = "stream_event")]
    StreamEvent(SDKPartialAssistantMessage),
    #[serde(rename = "result")]
    Result(SDKResultMessage),
    #[serde(rename = "system")]
    System(SDKSystemMessage),
    #[serde(rename = "tool_progress")]
    ToolProgress(SDKToolProgressMessage),
    #[serde(rename = "auth_status")]
    AuthStatus,
    #[serde(rename = "tool_use_summary")]
    ToolUseSummary,
    #[serde(rename = "rate_limit_event")]
    RateLimitEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SDKAssistantMessage {
    pub message: serde_json::Value,
    pub uuid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SDKUserMessage {
    pub message: Option<serde_json::Value>,
    pub uuid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_use_result: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SDKPartialAssistantMessage {
    pub event: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SDKResultMessage {
    pub subtype: String,
    pub uuid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errors: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SDKSystemMessage {
    pub subtype: String,
    pub uuid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compact_metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SDKToolProgressMessage {
    pub tool_name: String,
    pub tool_use_id: String,
    pub elapsed_time_seconds: f64,
    pub uuid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SDKCompactBoundaryMessage {
    pub uuid: String,
    pub compact_metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ControlMessage {
    #[serde(rename = "control_request")]
    Request(SDKControlRequest),
    #[serde(rename = "control_response")]
    Response(SDKControlResponse),
    #[serde(rename = "control_cancel_request")]
    CancelRequest(SDKControlCancelRequest),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SDKControlRequest {
    pub request_id: String,
    pub request: SDKControlRequestInner,
}

#[derive(Debug, Clone)]
pub enum SDKControlRequestInner {
    Initialize,
    CanUseTool(SDKControlPermissionRequest),
    Interrupt,
    SetModel { model: Option<String> },
    SetMaxThinkingTokens { max_thinking_tokens: Option<u64> },
    SetPermissionMode { mode: Option<String> },
    Unknown { subtype: String, payload: Value },
}

impl Serialize for SDKControlRequestInner {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            SDKControlRequestInner::Initialize => {
                serde_json::json!({"subtype": "initialize"}).serialize(serializer)
            }
            SDKControlRequestInner::CanUseTool(request) => request.serialize(serializer),
            SDKControlRequestInner::Interrupt => {
                serde_json::json!({"subtype": "interrupt"}).serialize(serializer)
            }
            SDKControlRequestInner::SetModel { model } => serde_json::json!({
                "subtype": "set_model",
                "model": model
            })
            .serialize(serializer),
            SDKControlRequestInner::SetMaxThinkingTokens {
                max_thinking_tokens,
            } => serde_json::json!({
                "subtype": "set_max_thinking_tokens",
                "max_thinking_tokens": max_thinking_tokens
            })
            .serialize(serializer),
            SDKControlRequestInner::SetPermissionMode { mode } => serde_json::json!({
                "subtype": "set_permission_mode",
                "mode": mode
            })
            .serialize(serializer),
            SDKControlRequestInner::Unknown { subtype, payload } => {
                let mut value = payload.clone();
                if let Value::Object(object) = &mut value {
                    object.insert("subtype".to_string(), Value::String(subtype.clone()));
                    value.serialize(serializer)
                } else {
                    serde_json::json!({
                        "subtype": subtype,
                        "payload": payload
                    })
                    .serialize(serializer)
                }
            }
        }
    }
}

impl<'de> Deserialize<'de> for SDKControlRequestInner {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let subtype = value
            .get("subtype")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

        match subtype.as_str() {
            "initialize" => Ok(SDKControlRequestInner::Initialize),
            "can_use_tool" => {
                let request = SDKControlPermissionRequest::deserialize(value)
                    .map_err(serde::de::Error::custom)?;
                Ok(SDKControlRequestInner::CanUseTool(request))
            }
            "interrupt" => Ok(SDKControlRequestInner::Interrupt),
            "set_model" => {
                #[derive(Deserialize)]
                struct SetModelRequest {
                    #[serde(default)]
                    model: Option<String>,
                }

                let request =
                    SetModelRequest::deserialize(value).map_err(serde::de::Error::custom)?;
                Ok(SDKControlRequestInner::SetModel {
                    model: request.model,
                })
            }
            "set_max_thinking_tokens" => {
                #[derive(Deserialize)]
                struct SetMaxThinkingTokensRequest {
                    #[serde(default, alias = "maxThinkingTokens")]
                    max_thinking_tokens: Option<u64>,
                }

                let request = SetMaxThinkingTokensRequest::deserialize(value)
                    .map_err(serde::de::Error::custom)?;
                Ok(SDKControlRequestInner::SetMaxThinkingTokens {
                    max_thinking_tokens: request.max_thinking_tokens,
                })
            }
            "set_permission_mode" => {
                #[derive(Deserialize)]
                struct SetPermissionModeRequest {
                    #[serde(default)]
                    mode: Option<String>,
                }

                let request = SetPermissionModeRequest::deserialize(value)
                    .map_err(serde::de::Error::custom)?;
                Ok(SDKControlRequestInner::SetPermissionMode { mode: request.mode })
            }
            _ => Ok(SDKControlRequestInner::Unknown {
                subtype,
                payload: value,
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SDKControlPermissionRequest {
    #[serde(default = "default_permission_request_subtype")]
    pub subtype: String,
    pub tool_name: String,
    pub tool_use_id: String,
    pub input: HashMap<String, serde_json::Value>,
    #[serde(
        default,
        alias = "permissionSuggestions",
        skip_serializing_if = "Option::is_none"
    )]
    pub permission_suggestions: Option<Value>,
    #[serde(
        default,
        alias = "blockedPath",
        skip_serializing_if = "Option::is_none"
    )]
    pub blocked_path: Option<String>,
    #[serde(
        default,
        alias = "decisionReason",
        skip_serializing_if = "Option::is_none"
    )]
    pub decision_reason: Option<Value>,
    #[serde(default, alias = "agentId", skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
}

fn default_permission_request_subtype() -> String {
    "can_use_tool".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SDKControlResponse {
    pub response: SDKControlResponseInner,
}

impl SDKControlResponse {
    pub fn success(request_id: impl Into<String>, response: Option<Value>) -> Self {
        Self {
            response: SDKControlResponseInner::Success {
                request_id: request_id.into(),
                response,
            },
        }
    }

    pub fn permission_success(request_id: impl Into<String>, response: PermissionResponse) -> Self {
        Self::success(
            request_id,
            Some(
                serde_json::to_value(response)
                    .expect("PermissionResponse should always serialize to JSON"),
            ),
        )
    }

    pub fn error(request_id: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            response: SDKControlResponseInner::Error {
                request_id: request_id.into(),
                error: error.into(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "subtype")]
pub enum SDKControlResponseInner {
    #[serde(rename = "success")]
    Success {
        request_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        response: Option<Value>,
    },
    #[serde(rename = "error")]
    Error { request_id: String, error: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "behavior")]
pub enum PermissionResponse {
    #[serde(rename = "allow")]
    Allow {
        #[serde(rename = "updatedInput")]
        updated_input: HashMap<String, serde_json::Value>,
    },
    #[serde(rename = "deny")]
    Deny { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SDKControlCancelRequest {
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SessionMessage {
    SDK(SDKMessage),
    Control(ControlMessage),
}

#[derive(Debug, Clone)]
pub enum RemotePermissionResponse {
    Allow {
        updated_input: HashMap<String, serde_json::Value>,
    },
    Deny {
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_can_use_tool_control_request_from_remote_session() {
        let message: SessionMessage = serde_json::from_value(json!({
            "type": "control_request",
            "request_id": "req-1",
            "request": {
                "subtype": "can_use_tool",
                "tool_name": "Bash",
                "tool_use_id": "tool-1",
                "input": {
                    "command": "pwd"
                },
                "permission_suggestions": [{"type": "addRules"}],
                "blocked_path": "/tmp/workspace",
                "decision_reason": {"type": "other", "reason": "ask mode"},
                "agent_id": "researcher@review"
            }
        }))
        .unwrap();

        match message {
            SessionMessage::Control(ControlMessage::Request(request)) => {
                assert_eq!(request.request_id, "req-1");
                match request.request {
                    SDKControlRequestInner::CanUseTool(permission) => {
                        assert_eq!(permission.subtype, "can_use_tool");
                        assert_eq!(permission.tool_name, "Bash");
                        assert_eq!(permission.tool_use_id, "tool-1");
                        assert_eq!(permission.input["command"], json!("pwd"));
                        assert_eq!(
                            permission.permission_suggestions.as_ref().unwrap()[0]["type"],
                            json!("addRules")
                        );
                        assert_eq!(permission.blocked_path.as_deref(), Some("/tmp/workspace"));
                        assert_eq!(
                            permission.decision_reason.as_ref().unwrap()["reason"],
                            json!("ask mode")
                        );
                        assert_eq!(permission.agent_id.as_deref(), Some("researcher@review"));
                    }
                    other => panic!("expected can_use_tool, got {:?}", other),
                }
            }
            other => panic!("expected control request, got {:?}", other),
        }
    }

    #[test]
    fn parses_known_server_control_requests() {
        let message: SessionMessage = serde_json::from_value(json!({
            "type": "control_request",
            "request_id": "req-init",
            "request": {
                "subtype": "initialize"
            }
        }))
        .unwrap();

        match message {
            SessionMessage::Control(ControlMessage::Request(request)) => {
                assert_eq!(request.request_id, "req-init");
                assert!(matches!(
                    request.request,
                    SDKControlRequestInner::Initialize
                ));
            }
            other => panic!("expected initialize control request, got {:?}", other),
        }

        let message: SessionMessage = serde_json::from_value(json!({
            "type": "control_request",
            "request_id": "req-model",
            "request": {
                "subtype": "set_model",
                "model": "claude-sonnet-4-5"
            }
        }))
        .unwrap();

        match message {
            SessionMessage::Control(ControlMessage::Request(request)) => {
                assert_eq!(request.request_id, "req-model");
                match request.request {
                    SDKControlRequestInner::SetModel { model } => {
                        assert_eq!(model.as_deref(), Some("claude-sonnet-4-5"));
                    }
                    other => panic!("expected set_model, got {:?}", other),
                }
            }
            other => panic!("expected control request, got {:?}", other),
        }

        let message: SessionMessage = serde_json::from_value(json!({
            "type": "control_request",
            "request_id": "req-thinking",
            "request": {
                "subtype": "set_max_thinking_tokens",
                "maxThinkingTokens": 4096
            }
        }))
        .unwrap();

        match message {
            SessionMessage::Control(ControlMessage::Request(request)) => match request.request {
                SDKControlRequestInner::SetMaxThinkingTokens {
                    max_thinking_tokens,
                } => {
                    assert_eq!(max_thinking_tokens, Some(4096));
                }
                other => panic!("expected set_max_thinking_tokens, got {:?}", other),
            },
            other => panic!("expected control request, got {:?}", other),
        }

        let message: SessionMessage = serde_json::from_value(json!({
            "type": "control_request",
            "request_id": "req-permission-mode",
            "request": {
                "subtype": "set_permission_mode",
                "mode": "acceptEdits",
                "ultraplan": false
            }
        }))
        .unwrap();

        match message {
            SessionMessage::Control(ControlMessage::Request(request)) => match request.request {
                SDKControlRequestInner::SetPermissionMode { mode } => {
                    assert_eq!(mode.as_deref(), Some("acceptEdits"));
                }
                other => panic!("expected set_permission_mode, got {:?}", other),
            },
            other => panic!("expected control request, got {:?}", other),
        }
    }

    #[test]
    fn parses_unknown_control_request_without_dropping_payload() {
        let message: SessionMessage = serde_json::from_value(json!({
            "type": "control_request",
            "request_id": "req-unknown",
            "request": {
                "subtype": "launch_missiles",
                "target": "moon"
            }
        }))
        .unwrap();

        match message {
            SessionMessage::Control(ControlMessage::Request(request)) => {
                assert_eq!(request.request_id, "req-unknown");
                match request.request {
                    SDKControlRequestInner::Unknown { subtype, payload } => {
                        assert_eq!(subtype, "launch_missiles");
                        assert_eq!(payload["target"], json!("moon"));
                    }
                    other => panic!("expected unknown control request, got {:?}", other),
                }
            }
            other => panic!("expected control request, got {:?}", other),
        }
    }
}
