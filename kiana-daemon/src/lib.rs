//! Composition root for Kiana control-plane adapters.

mod apply_patch;
mod approval_store;
mod context_query;
mod harness_capabilities;
mod harness_sandbox;
mod model_client;

use approval_store::MemoryApprovalStore;
use kiana_capability_broker::CapabilityBroker;
use kiana_core::ControlPlane;
use kiana_domain::{CommandIntent, RequestContext, RoleSpec};
use kiana_eventlog::{JsonlEventLog, MemoryEventLog};
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{PortError, RunnerPort};
use kiana_protocol::{RequestBody, RequestEnvelope, ResponseEnvelope, PROTOCOL_SCHEMA};
use kiana_runner::KianaHarness;
use std::sync::Arc;

pub struct DaemonHost {
    core: Arc<ControlPlane>,
}

impl DaemonHost {
    pub fn new(core: Arc<ControlPlane>) -> Self {
        Self { core }
    }

    pub fn local() -> Result<Self, PortError> {
        Self::with_runner_and_events(
            Arc::new(KianaHarness::new(model_client::from_env())),
            Arc::new(JsonlEventLog::open_default()?),
        )
    }

    pub fn with_harness(harness: KianaHarness) -> Result<Self, PortError> {
        Self::with_runner(Arc::new(harness))
    }

    pub fn with_harness_on_disk(
        harness: KianaHarness,
        events_path: impl AsRef<std::path::Path>,
    ) -> Result<Self, PortError> {
        Self::with_runner_and_events(
            Arc::new(harness),
            Arc::new(JsonlEventLog::open(events_path)?),
        )
    }

    pub fn with_runner(runner: Arc<dyn RunnerPort>) -> Result<Self, PortError> {
        Self::with_runner_and_events(runner, Arc::new(MemoryEventLog::new()))
    }

    pub fn with_runner_and_events(
        runner: Arc<dyn RunnerPort>,
        events: Arc<dyn kiana_ports::EventStorePort>,
    ) -> Result<Self, PortError> {
        let mut capabilities = CapabilityBroker::new();
        context_query::register(&mut capabilities)?;
        harness_capabilities::register(&mut capabilities)?;
        let core = ControlPlane::new(
            Arc::new(DefaultPolicyEngine),
            Arc::new(DefaultGateEngine),
            events,
            Arc::new(capabilities),
            Arc::new(MemoryApprovalStore::new()),
            runner,
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

        let Some(role) = RoleSpec::lookup(&request.metadata.role_id) else {
            return ResponseEnvelope::rejected(request_id, "role_unknown");
        };
        let department = request.metadata.department_id.trim();
        if !department.is_empty() && department != role.department_id {
            return ResponseEnvelope::rejected(request_id, "role_department_mismatch");
        }

        let context = RequestContext {
            request_id,
            session_id: request.metadata.session_id,
            project_root: request.metadata.project_root,
            actor_id: request.metadata.actor_id,
            project_trusted: request.metadata.project_trusted,
            permission_profile: request.metadata.permission_profile,
            role_id: role.role_id,
            department_id: role.department_id,
            work_packet_id: None,
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
            RequestBody::Run(run) => self.core.start_run(context, run.prompt, run.sandbox).await,
            RequestBody::Continue(run) => {
                self.core
                    .continue_run(context, run.prompt, run.sandbox, run.run_id)
                    .await
            }
            RequestBody::Cancel(run) => self.core.cancel_run(context, run.run_id, run.reason).await,
            RequestBody::Receipt(receipt) => self.core.read_receipt(context, receipt.run_id).await,
            RequestBody::Spawn(spawn) => {
                self.core
                    .spawn_from_packet(context, spawn.packet, spawn.sandbox)
                    .await
            }
            RequestBody::Symposium(symposium) => {
                self.core
                    .convene_symposium(
                        context,
                        symposium.goal,
                        symposium.anti_meeting,
                        Some(symposium.max_rounds),
                        symposium.sandbox,
                    )
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
