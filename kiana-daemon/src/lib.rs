//! Composition root for Kiana control-plane adapters.

mod approval_store;
mod context_query;

use approval_store::MemoryApprovalStore;
use async_trait::async_trait;
use kiana_capability_broker::CapabilityBroker;
use kiana_core::ControlPlane;
use kiana_domain::{CommandIntent, RequestContext};
use kiana_eventlog::MemoryEventLog;
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{PortError, RunnerPort};
use kiana_protocol::{RequestBody, RequestEnvelope, ResponseEnvelope, PROTOCOL_SCHEMA};
use kiana_runner::ProtocolRunner;
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use std::sync::Arc;

pub struct DaemonHost {
    core: Arc<ControlPlane>,
}

impl DaemonHost {
    pub fn new(core: Arc<ControlPlane>) -> Self {
        Self { core }
    }

    pub fn local() -> Result<Self, PortError> {
        let mut capabilities = CapabilityBroker::new();
        context_query::register(&mut capabilities)?;
        let core = ControlPlane::new(
            Arc::new(DefaultPolicyEngine),
            Arc::new(DefaultGateEngine),
            Arc::new(MemoryEventLog::new()),
            Arc::new(capabilities),
            Arc::new(MemoryApprovalStore::new()),
            Arc::new(RunnerAdapter(ProtocolRunner)),
        );
        Ok(Self::new(Arc::new(core)))
    }

    pub async fn handle(&self, request: RequestEnvelope) -> ResponseEnvelope {
        let request_id = request.metadata.request_id;
        if request.schema != PROTOCOL_SCHEMA {
            return ResponseEnvelope::rejected(request_id, "protocol_schema_unsupported");
        }
        if request.metadata.session_id.is_empty() {
            return ResponseEnvelope::rejected(request_id, "session_id_required");
        }
        if request.metadata.project_root.trim().is_empty() {
            return ResponseEnvelope::rejected(request_id, "project_root_required");
        }
        if request
            .metadata
            .actor_id
            .as_deref()
            .is_none_or(|actor| actor.trim().is_empty())
        {
            return ResponseEnvelope::rejected(request_id, "actor_identity_required");
        }

        let context = RequestContext {
            request_id,
            session_id: request.metadata.session_id,
            project_root: request.metadata.project_root,
            actor_id: request.metadata.actor_id,
            project_trusted: request.metadata.project_trusted,
            permission_profile: request.metadata.permission_profile,
        };
        let response = match request.body {
            RequestBody::Command(command) => {
                self.core
                    .handle_command(context, CommandIntent::new(command.name, command.arguments))
                    .await
            }
            RequestBody::ApprovalDecision(decision) => {
                self.core
                    .decide_approval(&context, decision.approval_id, decision.decision)
                    .await
            }
        };
        match response {
            Ok(response) => {
                let mut response = ResponseEnvelope::from_core(response);
                response.request_id = request_id;
                response
            }
            Err(error) => ResponseEnvelope {
                schema: PROTOCOL_SCHEMA.to_owned(),
                request_id,
                status: kiana_domain::ExecutionStatus::Failed,
                output: serde_json::Value::Null,
                error: Some(error.to_string()),
            },
        }
    }
}

struct RunnerAdapter(ProtocolRunner);

#[async_trait]
impl RunnerPort for RunnerAdapter {
    async fn send(&self, command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        Ok(self.0.send(command).await)
    }
}
