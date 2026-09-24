use super::*;

impl ControlPlane {
    /// Select a per-run role limit unless the trusted composition root supplies an override.
    pub fn with_role_step_limits(mut self, max_steps_override: Option<u32>) -> Self {
        self.max_steps_per_turn = max_steps_override;
        self
    }

    pub fn new(
        policy: Arc<dyn PolicyEngine>,
        gates: Arc<dyn GateEngine>,
        events: Arc<dyn EventStorePort>,
        capabilities: Arc<dyn CapabilityBrokerPort>,
        approvals: Arc<dyn ApprovalStorePort>,
        runner: Arc<dyn RunnerPort>,
    ) -> Self {
        Self::with_pre_tool_hooks(
            policy,
            gates,
            events,
            capabilities,
            approvals,
            runner,
            Arc::new(AllowAllPreToolHooks),
        )
    }

    pub fn with_pre_tool_hooks(
        policy: Arc<dyn PolicyEngine>,
        gates: Arc<dyn GateEngine>,
        events: Arc<dyn EventStorePort>,
        capabilities: Arc<dyn CapabilityBrokerPort>,
        approvals: Arc<dyn ApprovalStorePort>,
        runner: Arc<dyn RunnerPort>,
        pre_tool_hooks: Arc<dyn PreToolHookPort>,
    ) -> Self {
        Self::with_pre_tool_hooks_and_runtime_config(
            policy,
            gates,
            events,
            capabilities,
            approvals,
            runner,
            pre_tool_hooks,
            ControlPlaneRuntimeConfig {
                max_steps_per_turn: 32,
            },
        )
        .with_role_step_limits(None)
    }

    pub fn with_pre_tool_hooks_and_runtime_config(
        policy: Arc<dyn PolicyEngine>,
        gates: Arc<dyn GateEngine>,
        events: Arc<dyn EventStorePort>,
        capabilities: Arc<dyn CapabilityBrokerPort>,
        approvals: Arc<dyn ApprovalStorePort>,
        runner: Arc<dyn RunnerPort>,
        pre_tool_hooks: Arc<dyn PreToolHookPort>,
        runtime_config: ControlPlaneRuntimeConfig,
    ) -> Self {
        Self::with_pre_tool_hooks_and_cell_registry(
            policy,
            gates,
            events,
            capabilities,
            approvals,
            runner,
            pre_tool_hooks,
            ControlPlaneRuntimeConfig {
                max_steps_per_turn: runtime_config.max_steps_per_turn,
            },
            Arc::new(MemoryCellRegistry::new()),
        )
    }

    pub fn with_pre_tool_hooks_and_cell_registry(
        policy: Arc<dyn PolicyEngine>,
        gates: Arc<dyn GateEngine>,
        events: Arc<dyn EventStorePort>,
        capabilities: Arc<dyn CapabilityBrokerPort>,
        approvals: Arc<dyn ApprovalStorePort>,
        runner: Arc<dyn RunnerPort>,
        pre_tool_hooks: Arc<dyn PreToolHookPort>,
        runtime_config: ControlPlaneRuntimeConfig,
        cell_registry: Arc<dyn kiana_ports::CellRegistryPort>,
    ) -> Self {
        let max_steps_per_turn = Some(runtime_config.max_steps_per_turn);
        Self {
            policy,
            gates,
            events,
            capabilities,
            approvals,
            runner,
            max_steps_per_turn,
            workspace_checkpoints: None,
            pre_tool_hooks,
            cell_registry,
            sessions: Mutex::new(HashMap::new()),
            invocation_projections: Mutex::new(HashMap::new()),
            invocation_projection_event_ids: Mutex::new(HashMap::new()),
            pending_invocations: Mutex::new(HashMap::new()),
            cancellations: Mutex::new(HashMap::new()),
            capability_stops: Mutex::new(HashMap::new()),
            active_terminal_scopes: Mutex::new(HashMap::new()),
            path_locks: Mutex::new(HashMap::new()),
            durable_path_locks: Mutex::new(HashMap::new()),
            admission_scheduler: Arc::new(CapabilityAdmissionScheduler::default()),
        }
    }

    pub async fn handle_command(
        &self,
        context: RequestContext,
        intent: CommandIntent,
    ) -> Result<CoreResponse, CoreError> {
        if matches!(
            intent.name.as_str(),
            "workspace.transaction"
                | "execution.output.read"
                | "environment.inspect"
                | "tool.search"
                | "process.start"
                | "process.poll"
                | "process.stdin"
                | "process.resize"
                | "process.stop"
        ) {
            if context.cell_id.is_some()
                || context.actor_id.as_deref().is_none_or(str::is_empty)
                || !intent.arguments.is_object()
            {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "execution_operator_required",
                ));
            }
            let mut arguments = intent.arguments;
            if intent.name == "process.start" {
                let sandbox = crate::lifecycle::authorized_harness_sandbox(
                    &context,
                    arguments["sandbox"].as_str(),
                )
                .map_err(|reason| PortError::Failed(reason.to_owned()))?;
                arguments["sandbox"] = json!(sandbox);
            }
            arguments["operator_authorized"] = json!(true);
            let (kind, risk) = match intent.name.as_str() {
                "workspace.transaction" => (
                    CapabilityKind::Filesystem,
                    if matches!(arguments["action"].as_str(), Some("list" | "inspect")) {
                        RiskLevel::ReadOnly
                    } else {
                        RiskLevel::Critical
                    },
                ),
                "process.start" => (
                    CapabilityKind::Process,
                    if arguments["sandbox"] == "read-only" {
                        RiskLevel::ReadOnly
                    } else {
                        RiskLevel::LocalWrite
                    },
                ),
                "process.stdin" | "process.resize" | "process.stop" => {
                    (CapabilityKind::Process, RiskLevel::LocalWrite)
                }
                _ => (CapabilityKind::Query, RiskLevel::ReadOnly),
            };
            return self
                .authorize_and_execute(
                    &context,
                    CapabilityRequest::new(context.request_id, kind, intent.name, arguments)
                        .with_risk(risk),
                )
                .await;
        }
        if intent.name == "run.turn.v2" {
            let prompt = intent.arguments["prompt"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            let sandbox = intent.arguments["sandbox"].as_str().map(str::to_owned);
            let previous = intent.arguments["run_id"]
                .as_str()
                .map(|id| {
                    RunId::parse_str(id)
                        .ok_or_else(|| PortError::Failed("run_id_invalid".to_owned()))
                })
                .transpose()?;
            return self
                .continue_new_turn(context, prompt, sandbox, previous)
                .await;
        }
        if matches!(
            intent.name.as_str(),
            "communication.send"
                | "communication.ack"
                | "communication.reject"
                | "communication.escalate"
        ) {
            return self
                .handle_communication_command(context, &intent.name, intent.arguments)
                .await;
        }
        if matches!(
            intent.name.as_str(),
            "trace.capture" | "trace.replay" | "version.drift"
        ) {
            return self
                .handle_version_command(context, &intent.name, intent.arguments)
                .await;
        }
        if intent.name.starts_with("workspace.checkpoint.") {
            return self
                .handle_checkpoint_command(context, &intent.name, intent.arguments)
                .await;
        }
        if matches!(
            intent.name.as_str(),
            "human.inbox"
                | "human.resolve"
                | "failure.incidents"
                | "failure.reconcile"
                | "failure.recovery"
                | "failure.release"
                | "feedback.list"
                | "feedback.submit"
                | "feedback.review"
        ) {
            return self
                .handle_platform_command(context, &intent.name, intent.arguments)
                .await;
        }
        if intent.name == kiana_domain::SWARM_COMMAND {
            return self.handle_swarm_command(context, intent.arguments).await;
        }
        if intent.name == kiana_domain::SWARM_SNAPSHOT {
            return self.swarm_snapshot(context).await;
        }
        if intent.name == kiana_domain::AUTOMATION_COMMAND {
            return self
                .handle_workflow_command(context, intent.arguments)
                .await;
        }
        if intent.name == kiana_domain::AUTOMATION_SNAPSHOT {
            return self.workflow_snapshot(context).await;
        }
        if intent.name == "company.reclaim_expired.v1" {
            return self.reclaim_packet_leases(context, intent.arguments).await;
        }
        if intent.name == kiana_domain::COMPANY_COMMAND {
            return self.handle_company_command(context, intent.arguments).await;
        }
        if intent.name == kiana_domain::COMPANY_SNAPSHOT || intent.name == "company.next.v1" {
            return self.company_snapshot(context).await;
        }
        if intent.name == kiana_domain::COMPANY_GOVERNANCE {
            let Some(project_id) = intent.arguments["project_id"].as_str() else {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "company_governance_project_required",
                ));
            };
            if project_id.trim().is_empty() || project_id.len() > 256 {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "company_governance_project_invalid",
                ));
            }
            return self.company_governance(&context, project_id).await;
        }
        if intent.name == kiana_domain::MEMORY_DISTILL_COMMAND {
            return self
                .handle_memory_distillation(context, intent.arguments)
                .await;
        }
        if matches!(
            intent.name.as_str(),
            kiana_domain::CONNECTOR_MANAGE_OPERATION | kiana_domain::CONNECTOR_INVOKE_OPERATION
        ) {
            return self.handle_connector_command(context, intent).await;
        }
        if intent.name == kiana_domain::EXTENSION_MANAGE_OPERATION {
            return self
                .handle_extension_management(context, intent.arguments)
                .await;
        }
        if intent.name == CONTEXT_QUERY_COMMAND {
            return self.handle_context_query(context, intent.arguments).await;
        }
        if intent.name == "data.governance" {
            if context.cell_id.is_some() || context.actor_id.as_deref().is_none_or(str::is_empty) {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "governance_operator_required",
                ));
            }
            let mut arguments = intent.arguments;
            if !arguments.is_object() {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "governance_arguments_invalid",
                ));
            }
            arguments["operator_authorized"] = json!(true);
            let risk = if arguments["action"] == "list" {
                RiskLevel::ReadOnly
            } else {
                RiskLevel::ExternalSideEffect
            };
            return self
                .authorize_and_execute(
                    &context,
                    CapabilityRequest::new(
                        context.request_id,
                        CapabilityKind::Filesystem,
                        "data.governance",
                        arguments,
                    )
                    .with_risk(risk),
                )
                .await;
        }
        if intent.name == "memory.review" {
            if context.cell_id.is_some() || context.actor_id.as_deref().is_none_or(str::is_empty) {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "memory_review_operator_required",
                ));
            }
            let mut arguments = intent.arguments;
            self.prepare_memory_review(&context, &mut arguments).await?;
            let collection = arguments["collection"].as_str().map(str::to_owned);
            let Some(object) = arguments.as_object_mut() else {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "memory_review_arguments_invalid",
                ));
            };
            object.insert("operator_authorized".to_owned(), json!(true));
            object.insert("project_root".to_owned(), json!(context.project_root));
            object.insert("session_id".to_owned(), json!(context.session_id));
            object.insert("actor_id".to_owned(), json!(context.actor_id));
            object.insert("role_id".to_owned(), json!(context.role_id));
            object.insert("department_id".to_owned(), json!(context.department_id));
            let risk = if arguments["action"] == "list" {
                RiskLevel::ReadOnly
            } else {
                RiskLevel::ExternalSideEffect
            };
            let mut request = CapabilityRequest::new(
                context.request_id,
                CapabilityKind::Filesystem,
                "memory.review",
                arguments,
            );
            request.risk = risk;
            let mut response = self.authorize_and_execute(&context, request).await?;
            if risk == RiskLevel::ReadOnly && response.status == ExecutionStatus::Completed {
                response.output["proposals"] = json!(
                    self.pending_memory_proposals(&context, collection.as_deref())
                        .await?
                );
            }
            return Ok(response);
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
            "runner": "kiana-runner",
            "harness": HARNESS_ID,
            "capability_mode": "brokered",
            "legacy_prompt_loop": false,
            "legacy_edges_remaining": LEGACY_EDGES_REMAINING,
        });
        self.append_event(request_id, 2, "command.completed", output.clone())
            .await?;
        Ok(CoreResponse::completed(request_id, output))
    }

    async fn handle_extension_management(
        &self,
        context: RequestContext,
        arguments: Value,
    ) -> Result<CoreResponse, CoreError> {
        let normalized = normalize_extension_command(&context, arguments);
        let (arguments, risk) = match normalized {
            Ok(value) => value,
            Err(reason) => {
                self.append_event(
                    context.request_id,
                    1,
                    "request.accepted",
                    json!({"command":kiana_domain::EXTENSION_MANAGE_OPERATION}),
                )
                .await?;
                self.append_event(
                    context.request_id,
                    2,
                    "command.rejected",
                    json!({"reason":reason}),
                )
                .await?;
                return Ok(CoreResponse::blocked(context.request_id, reason));
            }
        };
        let request = CapabilityRequest::new(
            context.request_id,
            CapabilityKind::Filesystem,
            kiana_domain::EXTENSION_MANAGE_OPERATION,
            arguments,
        )
        .with_risk(risk);
        self.authorize_and_execute(&context, request).await
    }
}

fn normalize_extension_command(
    context: &RequestContext,
    arguments: Value,
) -> Result<(Value, RiskLevel), &'static str> {
    if context.cell_id.is_some()
        || context
            .actor_id
            .as_deref()
            .is_none_or(|s| s.trim().is_empty())
    {
        return Err("extension_operator_required");
    }
    let mut object = arguments
        .as_object()
        .cloned()
        .ok_or("extension_arguments_invalid")?;
    if object.keys().any(|key| {
        !matches!(
            key.as_str(),
            "action"
                | "extension_id"
                | "package_path"
                | "package_sha256"
                | "expected_registry_version"
                | "idempotency_key"
                | "reason"
                | "query"
                | "max_results"
        )
    }) {
        return Err("extension_arguments_invalid");
    }
    let action = object
        .get("action")
        .and_then(Value::as_str)
        .ok_or("extension_action_invalid")?;
    if !matches!(
        action,
        "inspect" | "list" | "search" | "install" | "upgrade" | "revoke" | "rollback"
    ) {
        return Err("extension_action_invalid");
    }
    let risk = if matches!(action, "list" | "inspect") {
        RiskLevel::ReadOnly
    } else {
        RiskLevel::ExternalSideEffect
    };
    if matches!(action, "install" | "upgrade")
        && object
            .get("package_path")
            .and_then(Value::as_str)
            .is_none_or(|s| !kiana_domain::valid_extension_path(s))
    {
        return Err("extension_package_path_must_be_project_relative");
    }
    if matches!(action, "search")
        && object
            .get("query")
            .and_then(Value::as_str)
            .is_some_and(|query| query.len() > 256 || query.contains('\0'))
    {
        return Err("extension_visibility_query_invalid");
    }
    if object
        .get("max_results")
        .and_then(Value::as_u64)
        .is_some_and(|value| value == 0 || value > 512)
    {
        return Err("extension_visibility_max_results_invalid");
    }
    if risk != RiskLevel::ReadOnly {
        if object
            .get("extension_id")
            .and_then(Value::as_str)
            .is_none_or(|s| !kiana_domain::valid_extension_identifier(s))
            || object
                .get("expected_registry_version")
                .and_then(Value::as_u64)
                .is_none()
            || object
                .get("idempotency_key")
                .and_then(Value::as_str)
                .is_none_or(|s| s.trim().is_empty() || s.len() > 128)
            || object
                .get("reason")
                .and_then(Value::as_str)
                .is_none_or(|s| s.trim().is_empty() || s.len() > 4096)
        {
            return Err("extension_mutation_fields_required");
        }
        if action != "revoke"
            && object
                .get("package_sha256")
                .and_then(Value::as_str)
                .is_none_or(|s| !kiana_domain::is_sha256_hex(s))
        {
            return Err("extension_final_package_hash_required");
        }
    }
    object.insert("operator_authorized".to_owned(), json!(true));
    object.insert("project_root".to_owned(), json!(context.project_root));
    object.insert("actor_id".to_owned(), json!(context.actor_id));
    object.insert("role_id".to_owned(), json!(context.role_id));
    object.insert("department_id".to_owned(), json!(context.department_id));
    Ok((Value::Object(object), risk))
}
