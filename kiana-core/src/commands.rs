use super::*;

impl ControlPlane {
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
        Self::with_pre_tool_hooks_and_cell_registry(
            policy,
            gates,
            events,
            capabilities,
            approvals,
            runner,
            pre_tool_hooks,
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
        cell_registry: Arc<dyn kiana_ports::CellRegistryPort>,
    ) -> Self {
        Self {
            policy,
            gates,
            events,
            capabilities,
            approvals,
            runner,
            pre_tool_hooks,
            cell_registry,
            sessions: Mutex::new(HashMap::new()),
            pending_invocations: Mutex::new(HashMap::new()),
            cancellations: Mutex::new(HashMap::new()),
            path_locks: Mutex::new(HashMap::new()),
            durable_path_locks: Mutex::new(HashMap::new()),
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
}
