//! The single command and capability control plane for Kiana.

use kiana_domain::{
    ApprovalDecision, ApprovalId, AuthorizedCapabilityRequest, CapabilityKind, CapabilityRequest,
    CommandIntent, CoreResponse, ExecutionStatus, GateDecision, RequestContext, RiskLevel,
    RuntimeEvent,
};
use kiana_gates::GateEngine;
use kiana_policy::PolicyEngine;
use kiana_ports::{ApprovalStorePort, CapabilityBrokerPort, EventStorePort, PortError, RunnerPort};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use serde_json::{json, Value};
use std::path::{Component, Path};
use std::sync::Arc;

pub const LEGACY_EDGES_REMAINING: usize = 10;
const CONTEXT_QUERY_COMMAND: &str = "context.query.v1";
const CONTEXT_REPO_MAP_OPERATION: &str = "context.repo_map";
const CONTEXT_INDEX_OPERATION: &str = "context.index.read";
const CONTEXT_INDEX_CACHE_OPERATION: &str = "context.index.cache.write";
const CONTEXT_ARTIFACTS_OPERATION: &str = "context.artifacts.read";
const CONTEXT_ARTIFACTS_CACHE_OPERATION: &str = "context.artifacts.cache.write";
const CONTEXT_ARTIFACT_STORE_OPERATION: &str = "context.artifact_store.read";
const CONTEXT_ARTIFACT_STORE_CACHE_OPERATION: &str = "context.artifact_store.cache.write";
const CONTEXT_ARTIFACT_INGEST_OPERATION: &str = "context.artifact_ingest.write";
const CONTEXT_ARTIFACT_GRAPH_OPERATION: &str = "context.artifact_graph.read";
const CONTEXT_ARTIFACT_READINESS_OPERATION: &str = "context.artifact_readiness.read";
const CONTEXT_SEARCH_OPERATION: &str = "context.search";
const CONTEXT_VECTOR_SEARCH_OPERATION: &str = "context.vector_search";
const CONTEXT_PACK_OPERATION: &str = "context.pack";
const MAX_CONTEXT_LIMIT: u64 = 1_000;
const MAX_CONTEXT_BYTES_PER_FILE: u64 = 16 * 1024 * 1024;
const MAX_CONTEXT_SNIPPET_LINES: u64 = 1_000;

pub struct ControlPlane {
    policy: Arc<dyn PolicyEngine>,
    gates: Arc<dyn GateEngine>,
    events: Arc<dyn EventStorePort>,
    capabilities: Arc<dyn CapabilityBrokerPort>,
    approvals: Arc<dyn ApprovalStorePort>,
    runner: Arc<dyn RunnerPort>,
}

impl ControlPlane {
    pub fn new(
        policy: Arc<dyn PolicyEngine>,
        gates: Arc<dyn GateEngine>,
        events: Arc<dyn EventStorePort>,
        capabilities: Arc<dyn CapabilityBrokerPort>,
        approvals: Arc<dyn ApprovalStorePort>,
        runner: Arc<dyn RunnerPort>,
    ) -> Self {
        Self {
            policy,
            gates,
            events,
            capabilities,
            approvals,
            runner,
        }
    }

    pub async fn handle_command(
        &self,
        context: RequestContext,
        intent: CommandIntent,
    ) -> Result<CoreResponse, CoreError> {
        if intent.name == CONTEXT_QUERY_COMMAND {
            return self.handle_context_query(context, intent.arguments).await;
        }
        let request_id = context.request_id;
        self.append_event(
            request_id,
            1,
            "request.accepted",
            json!({
                "command": &intent.name,
            }),
        )
        .await?;

        if !context.project_trusted {
            let reason = "project_untrusted";
            self.append_event(
                request_id,
                2,
                "command.rejected",
                json!({ "reason": reason }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, reason));
        }

        if intent.name != "system.architecture" {
            let reason = "command_unregistered";
            self.append_event(
                request_id,
                2,
                "command.rejected",
                json!({ "reason": reason, "command": &intent.name }),
            )
            .await?;
            return Ok(CoreResponse::blocked(request_id, reason));
        }

        let output = json!({
            "schema": "kiana.architecture-status.v1",
            "control_plane": "kiana-core",
            "composition_root": "kiana-daemon",
            "legacy_edges_remaining": LEGACY_EDGES_REMAINING,
        });
        self.append_event(request_id, 2, "command.completed", output.clone())
            .await?;
        Ok(CoreResponse::completed(request_id, output))
    }

    async fn handle_context_query(
        &self,
        context: RequestContext,
        arguments: Value,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = context.request_id;
        let normalized = match normalize_context_query_arguments(&context, &arguments) {
            Ok(normalized) => normalized,
            Err(error) => {
                let reason = error.reason();
                self.append_event(
                    request_id,
                    1,
                    "request.accepted",
                    json!({ "command": CONTEXT_QUERY_COMMAND }),
                )
                .await?;
                self.append_event(
                    request_id,
                    2,
                    "command.rejected",
                    json!({ "reason": reason }),
                )
                .await?;
                return Ok(CoreResponse::blocked(request_id, reason));
            }
        };
        let request = CapabilityRequest::new(
            request_id,
            CapabilityKind::Query,
            normalized.operation,
            normalized.arguments,
        )
        .with_risk(normalized.risk);
        self.authorize_and_execute(&context, request).await
    }

    pub async fn authorize_and_execute(
        &self,
        context: &RequestContext,
        request: CapabilityRequest,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = request.request_id;
        if request_id != context.request_id {
            return Ok(CoreResponse::blocked(
                request_id,
                "request_context_mismatch",
            ));
        }
        self.append_event(
            request_id,
            1,
            "request.accepted",
            json!({
                "capability": &request.capability,
                "operation": &request.operation,
                "risk": request.risk,
            }),
        )
        .await?;

        let policy = self.policy.evaluate(context, &request);
        let gate = self.gates.evaluate(&policy);
        self.append_event(
            request_id,
            2,
            "capability.decision",
            json!({ "policy": &policy, "gate": &gate }),
        )
        .await?;

        let authorization_id = match gate {
            GateDecision::Allowed { authorization_id } => authorization_id,
            GateDecision::AwaitingApproval { reason } => {
                let challenge = self.approvals.stage(context, request, &reason).await?;
                self.append_event(
                    request_id,
                    3,
                    "approval.requested",
                    json!({
                        "approval_id": challenge.approval_id,
                        "request_hash": &challenge.request_hash,
                        "session_id": context.session_id,
                        "actor_id": context.actor_id,
                        "expires_at_unix_ms": challenge.expires_at_unix_ms,
                    }),
                )
                .await?;
                self.approvals.activate(challenge.approval_id).await?;
                return Ok(CoreResponse {
                    request_id,
                    status: ExecutionStatus::AwaitingApproval,
                    output: json!({ "approval": challenge }),
                    error: Some(reason),
                });
            }
            GateDecision::Denied { reason } => {
                return Ok(CoreResponse {
                    request_id,
                    status: ExecutionStatus::Denied,
                    output: Value::Null,
                    error: Some(reason),
                });
            }
        };

        self.execute_authorized_request(request, authorization_id, 3)
            .await
    }

    pub async fn decide_approval(
        &self,
        context: &RequestContext,
        approval_id: ApprovalId,
        decision: ApprovalDecision,
    ) -> Result<CoreResponse, CoreError> {
        let pending = match self.approvals.consume(context, approval_id).await {
            Ok(pending) => pending,
            Err(error) => {
                return Ok(CoreResponse::blocked(context.request_id, error.to_string()));
            }
        };
        let request_id = pending.request.request_id;
        let event_kind = match decision {
            ApprovalDecision::Approve => "approval.approved",
            ApprovalDecision::Deny => "approval.denied",
        };
        self.append_event(
            request_id,
            4,
            event_kind,
            json!({
                "approval_id": approval_id,
                "request_hash": pending.challenge.request_hash,
                "session_id": context.session_id,
                "actor_id": context.actor_id,
            }),
        )
        .await?;

        if decision == ApprovalDecision::Deny {
            return Ok(CoreResponse {
                request_id,
                status: ExecutionStatus::Denied,
                output: json!({ "approval_id": approval_id }),
                error: Some("approval_denied".to_owned()),
            });
        }

        self.execute_authorized_request(pending.request, format!("approval:{approval_id}"), 5)
            .await
    }

    async fn execute_authorized_request(
        &self,
        request: CapabilityRequest,
        authorization_id: String,
        result_sequence: u64,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = request.request_id;
        let authorized = AuthorizedCapabilityRequest::new(authorization_id, request)?;
        match self.capabilities.execute(authorized).await {
            Ok(result) => {
                let success = result.success;
                let status = if success {
                    ExecutionStatus::Completed
                } else {
                    ExecutionStatus::Failed
                };
                let output = result.output;
                let error = (!success).then(|| {
                    output
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("capability_failed")
                        .to_owned()
                });
                if self
                    .append_event(
                        request_id,
                        result_sequence,
                        if success {
                            "capability.completed"
                        } else {
                            "capability.failed"
                        },
                        output.clone(),
                    )
                    .await
                    .is_err()
                {
                    return Ok(CoreResponse {
                        request_id,
                        status: ExecutionStatus::ResultUnknown,
                        output: Value::Null,
                        error: Some("result_event_persistence_failed".to_owned()),
                    });
                }
                Ok(CoreResponse {
                    request_id,
                    status,
                    output,
                    error,
                })
            }
            Err(error) => {
                let reason = error.to_string();
                self.append_event(
                    request_id,
                    result_sequence,
                    "capability.failed",
                    json!({ "error": &reason }),
                )
                .await?;
                Ok(CoreResponse {
                    request_id,
                    status: ExecutionStatus::Failed,
                    output: Value::Null,
                    error: Some(reason),
                })
            }
        }
    }

    pub async fn send_runner(&self, command: RunnerCommand) -> Result<Vec<RunnerEvent>, CoreError> {
        Ok(self.runner.send(command).await?)
    }

    async fn append_event(
        &self,
        request_id: kiana_domain::RequestId,
        sequence: u64,
        kind: &str,
        data: Value,
    ) -> Result<(), CoreError> {
        self.events
            .append(RuntimeEvent::new(request_id, sequence, kind, data)?)
            .await?;
        Ok(())
    }
}

fn normalize_context_query_arguments(
    context: &RequestContext,
    arguments: &Value,
) -> Result<NormalizedContextQuery, ContextQueryIntentError> {
    let arguments = arguments
        .as_object()
        .ok_or(ContextQueryIntentError::Invalid)?;
    if arguments.keys().any(|key| {
        !matches!(
            key.as_str(),
            "operation" | "output" | "options" | "project_root"
        )
    }) {
        return Err(ContextQueryIntentError::Invalid);
    }
    let operation = arguments
        .get("operation")
        .and_then(Value::as_str)
        .ok_or(ContextQueryIntentError::Invalid)?;
    let output = arguments
        .get("output")
        .and_then(Value::as_str)
        .ok_or(ContextQueryIntentError::Invalid)?;
    if !matches!(output, "json" | "text") {
        return Err(ContextQueryIntentError::Invalid);
    }
    let options = arguments
        .get("options")
        .and_then(Value::as_object)
        .ok_or(ContextQueryIntentError::Invalid)?;

    let (broker_operation, risk, normalized) = match operation {
        "repo_map" => {
            ensure_option_keys(options, &["max_tokens"])?;
            let max_tokens = optional_bounded_u64(options, "max_tokens", u64::MAX)?;
            (
                CONTEXT_REPO_MAP_OPERATION,
                RiskLevel::ReadOnly,
                json!({
                    "project_root": context.project_root,
                    "output": output,
                    "max_tokens": max_tokens,
                }),
            )
        }
        "artifact_graph" | "artifact_readiness" => {
            ensure_option_keys(options, &["root", "max_bytes_per_file"])?;
            let root = optional_relative_path(options, "root")?;
            let max_bytes_per_file =
                optional_bounded_u64(options, "max_bytes_per_file", MAX_CONTEXT_BYTES_PER_FILE)?;
            let broker_operation = if operation == "artifact_graph" {
                CONTEXT_ARTIFACT_GRAPH_OPERATION
            } else {
                CONTEXT_ARTIFACT_READINESS_OPERATION
            };
            (
                broker_operation,
                RiskLevel::ReadOnly,
                json!({
                    "project_root": context.project_root,
                    "root": root,
                    "output": output,
                    "max_bytes_per_file": max_bytes_per_file,
                }),
            )
        }
        "index" | "artifacts" | "artifact_store" => {
            ensure_option_keys(options, &["root", "max_bytes_per_file"])?;
            let root = optional_relative_path(options, "root")?;
            let max_bytes_per_file =
                optional_bounded_u64(options, "max_bytes_per_file", MAX_CONTEXT_BYTES_PER_FILE)?;
            let broker_operation = match operation {
                "index" => CONTEXT_INDEX_OPERATION,
                "artifacts" => CONTEXT_ARTIFACTS_OPERATION,
                _ => CONTEXT_ARTIFACT_STORE_OPERATION,
            };
            (
                broker_operation,
                RiskLevel::ReadOnly,
                json!({
                    "project_root": context.project_root,
                    "root": root,
                    "output": output,
                    "max_bytes_per_file": max_bytes_per_file,
                }),
            )
        }
        "index_cache_write" | "artifacts_cache_write" | "artifact_store_cache_write" => {
            ensure_option_keys(options, &["root", "cache", "max_bytes_per_file"])?;
            let root = optional_relative_path(options, "root")?;
            let cache = required_relative_path(options, "cache")?;
            let max_bytes_per_file =
                optional_bounded_u64(options, "max_bytes_per_file", MAX_CONTEXT_BYTES_PER_FILE)?;
            let broker_operation = match operation {
                "index_cache_write" => CONTEXT_INDEX_CACHE_OPERATION,
                "artifacts_cache_write" => CONTEXT_ARTIFACTS_CACHE_OPERATION,
                _ => CONTEXT_ARTIFACT_STORE_CACHE_OPERATION,
            };
            (
                broker_operation,
                RiskLevel::LocalWrite,
                json!({
                    "project_root": context.project_root,
                    "root": root,
                    "cache": cache,
                    "output": output,
                    "max_bytes_per_file": max_bytes_per_file,
                }),
            )
        }
        "artifact_ingest_write" => {
            ensure_option_keys(options, &["root", "source", "store", "max_bytes_per_file"])?;
            let root = optional_relative_path(options, "root")?;
            let source = required_relative_path(options, "source")?;
            let store = optional_relative_path(options, "store")?;
            let max_bytes_per_file =
                optional_bounded_u64(options, "max_bytes_per_file", MAX_CONTEXT_BYTES_PER_FILE)?;
            (
                CONTEXT_ARTIFACT_INGEST_OPERATION,
                RiskLevel::LocalWrite,
                json!({
                    "project_root": context.project_root,
                    "root": root,
                    "source": source,
                    "store": store,
                    "output": output,
                    "max_bytes_per_file": max_bytes_per_file,
                }),
            )
        }
        "search" | "vector_search" | "pack" => {
            let allowed = if operation == "pack" {
                &[
                    "query",
                    "root",
                    "limit",
                    "max_bytes_per_file",
                    "max_snippet_lines",
                ][..]
            } else {
                &["query", "root", "limit", "max_bytes_per_file"][..]
            };
            ensure_option_keys(options, allowed)?;
            let query = options
                .get("query")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|query| !query.is_empty())
                .ok_or(ContextQueryIntentError::Invalid)?;
            let root = optional_relative_path(options, "root")?;
            let limit = optional_bounded_u64(options, "limit", MAX_CONTEXT_LIMIT)?;
            let max_bytes_per_file =
                optional_bounded_u64(options, "max_bytes_per_file", MAX_CONTEXT_BYTES_PER_FILE)?;
            let max_snippet_lines = if operation == "pack" {
                optional_bounded_u64(options, "max_snippet_lines", MAX_CONTEXT_SNIPPET_LINES)?
            } else {
                None
            };
            let broker_operation = match operation {
                "search" => CONTEXT_SEARCH_OPERATION,
                "vector_search" => CONTEXT_VECTOR_SEARCH_OPERATION,
                _ => CONTEXT_PACK_OPERATION,
            };
            let normalized = if operation == "pack" {
                json!({
                    "project_root": context.project_root,
                    "root": root,
                    "output": output,
                    "query": query,
                    "limit": limit,
                    "max_bytes_per_file": max_bytes_per_file,
                    "max_snippet_lines": max_snippet_lines,
                })
            } else {
                json!({
                    "project_root": context.project_root,
                    "root": root,
                    "output": output,
                    "query": query,
                    "limit": limit,
                    "max_bytes_per_file": max_bytes_per_file,
                })
            };
            (broker_operation, RiskLevel::ReadOnly, normalized)
        }
        _ => return Err(ContextQueryIntentError::Unregistered),
    };

    Ok(NormalizedContextQuery {
        operation: broker_operation,
        risk,
        arguments: normalized,
    })
}

fn ensure_option_keys(
    options: &serde_json::Map<String, Value>,
    allowed: &[&str],
) -> Result<(), ContextQueryIntentError> {
    if options.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(ContextQueryIntentError::Invalid);
    }
    Ok(())
}

fn optional_bounded_u64(
    options: &serde_json::Map<String, Value>,
    key: &str,
    maximum: u64,
) -> Result<Option<u64>, ContextQueryIntentError> {
    match options.get(key) {
        None => Ok(None),
        Some(value) => value
            .as_u64()
            .filter(|value| *value > 0 && *value <= maximum)
            .map(Some)
            .ok_or(ContextQueryIntentError::Invalid),
    }
}

fn optional_relative_path(
    options: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<String>, ContextQueryIntentError> {
    let Some(value) = options.get(key) else {
        return Ok(None);
    };
    let value = value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(ContextQueryIntentError::Invalid)?;
    validate_relative_path(value)?;
    Ok(Some(value.to_owned()))
}

fn required_relative_path(
    options: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<String, ContextQueryIntentError> {
    let value = options
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(ContextQueryIntentError::Invalid)?;
    validate_relative_path(value)?;
    Ok(value.to_owned())
}

fn validate_relative_path(value: &str) -> Result<(), ContextQueryIntentError> {
    let path = Path::new(value);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(ContextQueryIntentError::Invalid);
    }
    Ok(())
}

struct NormalizedContextQuery {
    operation: &'static str,
    risk: RiskLevel,
    arguments: Value,
}

enum ContextQueryIntentError {
    Invalid,
    Unregistered,
}

impl ContextQueryIntentError {
    fn reason(&self) -> &'static str {
        match self {
            Self::Invalid => "command_arguments_invalid",
            Self::Unregistered => "command_unregistered",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error(transparent)]
    Domain(#[from] kiana_domain::DomainError),
    #[error(transparent)]
    Port(#[from] PortError),
}
