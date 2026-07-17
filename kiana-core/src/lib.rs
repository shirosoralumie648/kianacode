//! The single command and capability control plane for Kiana.

use kiana_domain::{
    AuthorizedCapabilityRequest, CapabilityKind, CapabilityRequest, CommandIntent, CoreResponse,
    ExecutionStatus, GateDecision, RequestContext, RiskLevel, RuntimeEvent,
};
use kiana_gates::GateEngine;
use kiana_policy::PolicyEngine;
use kiana_ports::{CapabilityBrokerPort, EventStorePort, PortError, RunnerPort};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use serde_json::{json, Value};
use std::sync::Arc;

pub const LEGACY_EDGES_REMAINING: usize = 10;
const CONTEXT_QUERY_COMMAND: &str = "context.query.v1";
const CONTEXT_REPO_MAP_OPERATION: &str = "context.repo_map";

pub struct ControlPlane {
    policy: Arc<dyn PolicyEngine>,
    gates: Arc<dyn GateEngine>,
    events: Arc<dyn EventStorePort>,
    capabilities: Arc<dyn CapabilityBrokerPort>,
    runner: Arc<dyn RunnerPort>,
}

impl ControlPlane {
    pub fn new(
        policy: Arc<dyn PolicyEngine>,
        gates: Arc<dyn GateEngine>,
        events: Arc<dyn EventStorePort>,
        capabilities: Arc<dyn CapabilityBrokerPort>,
        runner: Arc<dyn RunnerPort>,
    ) -> Self {
        Self {
            policy,
            gates,
            events,
            capabilities,
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
        let Some(arguments) = normalize_repo_map_arguments(&context, &arguments) else {
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
                json!({ "reason": "command_arguments_invalid" }),
            )
            .await?;
            return Ok(CoreResponse::blocked(
                request_id,
                "command_arguments_invalid",
            ));
        };
        let request = CapabilityRequest::new(
            request_id,
            CapabilityKind::Query,
            CONTEXT_REPO_MAP_OPERATION,
            arguments,
        )
        .with_risk(RiskLevel::ReadOnly);
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
                return Ok(CoreResponse {
                    request_id,
                    status: ExecutionStatus::AwaitingApproval,
                    output: Value::Null,
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
                        3,
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
                    3,
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

fn normalize_repo_map_arguments(context: &RequestContext, arguments: &Value) -> Option<Value> {
    let arguments = arguments.as_object()?;
    if arguments.keys().any(|key| {
        !matches!(
            key.as_str(),
            "operation" | "output" | "options" | "project_root"
        )
    }) {
        return None;
    }
    if arguments.get("operation")?.as_str()? != "repo_map" {
        return None;
    }
    let output = arguments.get("output")?.as_str()?;
    if !matches!(output, "json" | "text") {
        return None;
    }
    let options = arguments.get("options")?.as_object()?;
    if options.keys().any(|key| key != "max_tokens") {
        return None;
    }
    let max_tokens = match options.get("max_tokens") {
        Some(value) => {
            let value = value.as_u64()?;
            if value == 0 {
                return None;
            }
            Some(value)
        }
        None => None,
    };
    Some(json!({
        "project_root": context.project_root,
        "output": output,
        "max_tokens": max_tokens,
    }))
}

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error(transparent)]
    Domain(#[from] kiana_domain::DomainError),
    #[error(transparent)]
    Port(#[from] PortError),
}
