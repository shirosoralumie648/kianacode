//! Immutable edit snapshots and explicitly approved undo, sharing the existing execution spine.
use super::platform::{platform_context, platform_root};
use super::*;
use kiana_domain::{json_digest, WorkspaceCheckpoint};

impl ControlPlane {
    /// Data deletion/revocation invalidates edit snapshots; an earlier undo only invalidates runners.
    async fn checkpoint_source_epoch(
        &self,
        project_root: &str,
    ) -> Result<Option<String>, CoreError> {
        let events = self.events.read_all().await?;
        Ok(events
            .iter()
            .rev()
            .find(|event| {
                event.kind == "data.revocation_requested"
                    && event.data["project_root"].as_str().is_some_and(|root| {
                        Self::canonical_project_root(root)
                            == Self::canonical_project_root(project_root)
                    })
            })
            .map(|event| event.event_id.to_string()))
    }

    pub fn with_workspace_checkpoints(
        mut self,
        adapter: Arc<dyn kiana_ports::WorkspaceCheckpointPort>,
    ) -> Self {
        self.workspace_checkpoints = Some(adapter);
        self
    }

    pub(crate) async fn handle_checkpoint_command(
        &self,
        context: RequestContext,
        name: &str,
        arguments: Value,
    ) -> Result<CoreResponse, CoreError> {
        if let Err(reason) = platform_context(&context) {
            return Ok(CoreResponse::blocked(context.request_id, reason));
        }
        if !arguments.is_object() {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "checkpoint_arguments_invalid",
            ));
        }
        if name == "workspace.checkpoint.list" {
            let checkpoints = self.checkpoints_for(&context).await?;
            return Ok(CoreResponse::completed(
                context.request_id,
                json!({"schema":"kiana.workspace-checkpoints.v1","checkpoints":checkpoints.into_iter().map(|checkpoint|json!({
                "checkpoint_id":checkpoint.checkpoint_id,"run_id":checkpoint.run_id,"session_id":checkpoint.session_id,"transcript_offset":checkpoint.transcript_offset,
                "workspace_revision":checkpoint.workspace_revision,"invocation_id":checkpoint.invocation_id,"reason":checkpoint.reason,"paths":checkpoint.files.iter().map(|file|&file.path).collect::<Vec<_>>()
            })).collect::<Vec<_>>()}),
            ));
        }
        let Some(adapter) = self.workspace_checkpoints.as_ref() else {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "checkpoint_service_unavailable",
            ));
        };
        if name == "workspace.checkpoint.create" {
            let paths: Vec<String> = serde_json::from_value(arguments["paths"].clone())
                .map_err(|_| checkpoint_error("checkpoint_paths_required"))?;
            if paths.is_empty() {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "checkpoint_paths_required",
                ));
            }
            let checkpoint = self
                .capture_edit_checkpoint(&context, None, None, paths, "human", 0)
                .await?
                .ok_or_else(|| checkpoint_error("checkpoint_service_unavailable"))?;
            return Ok(CoreResponse::completed(
                context.request_id,
                json!({"checkpoint":checkpoint}),
            ));
        }
        let Some(checkpoint) = self
            .checkpoints_for(&context)
            .await?
            .into_iter()
            .find(|checkpoint| arguments["checkpoint_id"] == checkpoint.checkpoint_id)
        else {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "checkpoint_not_found",
            ));
        };
        if checkpoint.data_epoch != self.checkpoint_source_epoch(&context.project_root).await? {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "checkpoint_data_epoch_changed",
            ));
        }
        let preview = adapter.preview(&checkpoint).await?;
        if name == "workspace.checkpoint.preview" {
            return Ok(CoreResponse::completed(
                context.request_id,
                json!({"preview":preview}),
            ));
        }
        if name != "workspace.checkpoint.restore" {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "checkpoint_command_unknown",
            ));
        }
        if checkpoint.files.is_empty() {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "checkpoint_contains_no_file_edits",
            ));
        }
        if arguments["expected_revision"].as_str() != Some(preview.current_revision.as_str()) {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "checkpoint_revision_conflict",
            ));
        }
        for file in &checkpoint.files {
            checkpoint_path_allowed(&context, &file.path)?;
        }
        self.validate_checkpoint_company_scope(&context, &checkpoint)
            .await?;
        // The exact immutable snapshot, current revision and operator scope are in the approval hash.
        let request=CapabilityRequest::new(context.request_id,CapabilityKind::Filesystem,"workspace.checkpoint.restore",json!({
            "snapshot":checkpoint,"expected_revision":preview.current_revision,"operator_authorized":true,"sandbox":"workspace-write"
        })).with_risk(RiskLevel::Critical);
        self.authorize_and_execute(&context, request).await
    }

    async fn checkpoints_for(
        &self,
        context: &RequestContext,
    ) -> Result<Vec<WorkspaceCheckpoint>, CoreError> {
        let events = self.platform_owned_events(context).await?;
        let mut checkpoints = Vec::new();
        for event in events
            .into_iter()
            .filter(|event| event.kind == "workspace.checkpoint_created")
        {
            let snapshot: WorkspaceCheckpoint =
                serde_json::from_value(event.data["checkpoint"].clone())
                    .map_err(|_| checkpoint_error("checkpoint_record_invalid"))?;
            if snapshot.project_root == platform_root(context)
                && Some(snapshot.actor_id.as_str()) == context.actor_id.as_deref()
                && snapshot.session_id == context.session_id
                && snapshot.role_id == context.role_id
            {
                if snapshot.schema != "kiana.workspace-checkpoint.v1"
                    || snapshot.workspace_revision != json_digest(&json!(snapshot.files))
                {
                    return Err(checkpoint_error("checkpoint_record_invalid"));
                }
                checkpoints.push(snapshot);
            }
        }
        Ok(checkpoints)
    }

    /// Called before user input and before a write invocation. Empty paths mark a transcript boundary.
    pub(crate) async fn capture_edit_checkpoint(
        &self,
        context: &RequestContext,
        run_id: Option<RunId>,
        invocation_id: Option<String>,
        mut paths: Vec<String>,
        reason: &str,
        transcript_offset: u64,
    ) -> Result<Option<WorkspaceCheckpoint>, CoreError> {
        let Some(adapter) = self.workspace_checkpoints.as_ref() else {
            return Ok(None);
        };
        paths.sort();
        paths.dedup();
        if paths.len() > 128 {
            return Err(checkpoint_error("checkpoint_paths_limit"));
        }
        for path in &paths {
            checkpoint_path_allowed(context, path)?;
        }
        let data_epoch = self.checkpoint_source_epoch(&context.project_root).await?;
        let files = adapter
            .capture_files(&platform_root(context), &paths)
            .await?;
        if data_epoch != self.checkpoint_source_epoch(&context.project_root).await? {
            return Err(checkpoint_error("checkpoint_data_epoch_changed"));
        }
        let checkpoint = WorkspaceCheckpoint {
            schema: "kiana.workspace-checkpoint.v1".to_owned(),
            checkpoint_id: RequestId::new().to_string(),
            project_root: platform_root(context),
            actor_id: context.actor_id.clone().unwrap_or_default(),
            session_id: context.session_id.clone(),
            role_id: context.role_id.clone(),
            work_packet_id: context.work_packet_id.clone(),
            path_allow: context.path_allow.clone(),
            run_id,
            transcript_offset,
            invocation_id,
            reason: reason.to_owned(),
            workspace_revision: json_digest(&json!(files)),
            data_epoch,
            files,
        };
        let data = json!({"project_root":checkpoint.project_root,"actor_id":checkpoint.actor_id,"run_id":run_id,"checkpoint":checkpoint});
        if kiana_domain::redact_value(&data) != data {
            return Err(checkpoint_error("checkpoint_sensitive_content_denied"));
        }
        let event = RuntimeEvent::new(RequestId::new(), 1, "workspace.checkpoint_created", data)?
            .with_stream_metadata("workspace_checkpoint", checkpoint.checkpoint_id.clone(), 1);
        self.events.append_expected(event, Some(0)).await?;
        Ok(Some(checkpoint))
    }

    pub(crate) async fn checkpoint_before_write(
        &self,
        context: &RequestContext,
        run_id: RunId,
        request: &CapabilityRequest,
        offset: u64,
    ) -> Result<(), CoreError> {
        if request.risk == RiskLevel::ReadOnly {
            return Ok(());
        }
        let mut paths = Vec::new();
        if request.operation == "apply_patch" {
            if let Some(patch) = request.arguments["patch"].as_str() {
                for line in patch.lines() {
                    for marker in [
                        "*** Add File: ",
                        "*** Update File: ",
                        "*** Delete File: ",
                        "*** Move to: ",
                    ] {
                        if let Some(path) = line.trim().strip_prefix(marker) {
                            paths.push(path.trim().to_owned());
                        }
                    }
                }
            }
        }
        let invocation = request.arguments["call_id"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| Some(request.request_id.to_string()));
        self.capture_edit_checkpoint(
            context,
            Some(run_id),
            invocation,
            paths,
            if request.operation == "apply_patch" {
                "before_edit"
            } else {
                "before_side_effect_metadata_only"
            },
            offset,
        )
        .await?;
        Ok(())
    }

    pub(crate) async fn checkpoint_before_input(
        &self,
        context: &RequestContext,
        run_id: RunId,
        sequence: &mut u64,
    ) -> Result<Option<CoreResponse>, CoreError> {
        if let Err(error) = self
            .capture_edit_checkpoint(
                context,
                Some(run_id),
                None,
                Vec::new(),
                "before_user_input",
                sequence.saturating_sub(1),
            )
            .await
        {
            let reason = error.to_string();
            self.record_terminal_event(
                context.request_id,
                sequence,
                run_id,
                "run.failed",
                json!({"run_id":run_id,"error":reason,"before_execution":true}),
            )
            .await?;
            return Ok(Some(CoreResponse {
                request_id: context.request_id,
                status: ExecutionStatus::Failed,
                output: json!({"run_id":run_id,"session_id":context.session_id}),
                error: Some(reason),
            }));
        }
        Ok(None)
    }

    async fn validate_checkpoint_company_scope(
        &self,
        context: &RequestContext,
        snapshot: &WorkspaceCheckpoint,
    ) -> Result<(), CoreError> {
        if context.cell_id.is_some()
            || snapshot.actor_id != context.actor_id.clone().unwrap_or_default()
            || snapshot.session_id != context.session_id
            || snapshot.role_id != context.role_id
            || snapshot.project_root != platform_root(context)
        {
            return Err(checkpoint_error("checkpoint_restore_scope_changed"));
        }
        let response = self.company_snapshot(context.clone()).await?;
        if response.status != ExecutionStatus::Completed {
            return Err(checkpoint_error("checkpoint_company_scope_unavailable"));
        }
        let company: kiana_domain::CompanyState =
            serde_json::from_value(response.output["state"].clone())
                .map_err(|_| checkpoint_error("checkpoint_company_scope_invalid"))?;
        if company.projects.is_empty() && snapshot.work_packet_id.is_none() {
            return Ok(());
        }
        let packet_id = snapshot
            .work_packet_id
            .as_deref()
            .ok_or_else(|| checkpoint_error("checkpoint_frozen_packet_required"))?;
        let packet = company
            .packets
            .get(packet_id)
            .ok_or_else(|| checkpoint_error("checkpoint_packet_not_found"))?;
        let run = company
            .runs
            .values()
            .find(|run| {
                run.packet_id == packet_id
                    && run.author_session_id == snapshot.session_id
                    && run.run_id == snapshot.run_id
            })
            .ok_or_else(|| checkpoint_error("checkpoint_packet_author_mismatch"))?;
        let project = company
            .projects
            .get(&run.project_id)
            .ok_or_else(|| checkpoint_error("checkpoint_project_not_found"))?;
        if matches!(
            project.status,
            kiana_domain::ProjectStatus::Closed | kiana_domain::ProjectStatus::Archived
        ) || packet.project_id != run.project_id
            || snapshot.path_allow != packet.packet.path_allow
            || snapshot.files.iter().any(|file| {
                kiana_domain::enforce_path_containment(&packet.packet.path_allow, &file.path)
                    .is_err()
            })
        {
            return Err(checkpoint_error("checkpoint_frozen_scope_invalid"));
        }
        Ok(())
    }

    /// Before any restore writes, fence prior context and revoke every pending project approval.
    pub(crate) async fn prepare_checkpoint_restore(
        &self,
        request: &CapabilityRequest,
    ) -> Result<(), CoreError> {
        if request.operation != "workspace.checkpoint.restore" {
            return Ok(());
        }
        if request.risk != RiskLevel::Critical
            || request.arguments["operator_authorized"] != true
            || request.cell_id.is_some()
        {
            return Err(checkpoint_error("checkpoint_restore_operator_required"));
        }
        let snapshot: WorkspaceCheckpoint =
            serde_json::from_value(request.arguments["snapshot"].clone())
                .map_err(|_| checkpoint_error("checkpoint_record_invalid"))?;
        for (key, expected) in [
            ("project_root", snapshot.project_root.as_str()),
            ("actor_id", snapshot.actor_id.as_str()),
            ("session_id", snapshot.session_id.as_str()),
            ("role_id", snapshot.role_id.as_str()),
        ] {
            if request.arguments[key].as_str() != Some(expected) {
                return Err(checkpoint_error("checkpoint_restore_scope_changed"));
            }
        }
        let records = self
            .events
            .read_stream("workspace_checkpoint", &snapshot.checkpoint_id)
            .await?;
        if records.len() != 1 || records[0].data["checkpoint"] != request.arguments["snapshot"] {
            return Err(checkpoint_error("checkpoint_snapshot_not_authoritative"));
        }
        let role =
            RoleSpec::lookup(&snapshot.role_id).ok_or_else(|| checkpoint_error("role_unknown"))?;
        let context = RequestContext {
            request_id: request.request_id,
            session_id: snapshot.session_id.clone(),
            project_root: snapshot.project_root.clone(),
            actor_id: Some(snapshot.actor_id.clone()),
            project_trusted: request.arguments["project_trusted"] == true,
            permission_profile: PermissionProfile::Balanced,
            role_id: snapshot.role_id.clone(),
            department_id: role.department_id,
            work_packet_id: snapshot.work_packet_id.clone(),
            cell_id: None,
            path_allow: snapshot.path_allow.clone(),
        };
        self.validate_checkpoint_company_scope(&context, &snapshot)
            .await?;
        for file in &snapshot.files {
            checkpoint_path_allowed(&context, &file.path)?;
        }
        if snapshot.data_epoch != self.checkpoint_source_epoch(&snapshot.project_root).await? {
            return Err(checkpoint_error("checkpoint_data_epoch_changed"));
        }
        let adapter = self
            .workspace_checkpoints
            .as_ref()
            .ok_or_else(|| checkpoint_error("checkpoint_service_unavailable"))?;
        if adapter.preview(&snapshot).await?.current_revision
            != request.arguments["expected_revision"]
        {
            return Err(checkpoint_error("checkpoint_revision_conflict"));
        }
        self.begin_project_invalidation(&snapshot.project_root,"workspace.restore_requested",
            json!({"project_root":snapshot.project_root,"actor_id":snapshot.actor_id,"checkpoint_id":snapshot.checkpoint_id,"request_id":request.request_id})).await?;
        self.approvals
            .invalidate_project(&snapshot.project_root, "workspace_restored")
            .await?;
        self.stop_project_runs(&snapshot.project_root, "workspace_restored")
            .await?;
        Ok(())
    }

    pub(crate) async fn finish_checkpoint_restore(
        &self,
        request: &CapabilityRequest,
        result: &CapabilityResult,
    ) -> Result<(), CoreError> {
        if request.operation != "workspace.checkpoint.restore" || !result.success {
            return Ok(());
        }
        if result.request_id != request.request_id
            || result.output["restored"] != true
            || result.output["workspace_revision"]
                != request.arguments["snapshot"]["workspace_revision"]
        {
            return Err(checkpoint_error(
                "result_unknown:checkpoint_restore_result_invalid",
            ));
        }
        let event = RuntimeEvent::new(
            RequestId::new(),
            1,
            "workspace.restored",
            json!({"project_root":request.arguments["project_root"],"actor_id":request.arguments["actor_id"],"session_id":request.arguments["session_id"],
            "checkpoint_id":request.arguments["snapshot"]["checkpoint_id"],"workspace_revision":result.output["workspace_revision"],"request_id":request.request_id,"old_approvals_invalidated":true,"runner_context_invalidated":true}),
        )?;
        self.events.append(event).await?;
        Ok(())
    }
}

fn checkpoint_path_allowed(context: &RequestContext, path: &str) -> Result<(), CoreError> {
    let normalized = kiana_domain::normalize_role_path(path)
        .ok_or_else(|| checkpoint_error("checkpoint_path_invalid"))?;
    if normalized == "."
        || normalized.split('/').any(|part| {
            matches!(part, ".git" | ".kiana" | ".env" | "node_modules" | "target")
                || part.starts_with(".env.")
        })
    {
        return Err(checkpoint_error("checkpoint_protected_path"));
    }
    let role =
        RoleSpec::lookup(&context.role_id).ok_or_else(|| checkpoint_error("role_unknown"))?;
    if kiana_domain::enforce_path_containment(&role.path_allow, &normalized).is_err()
        || (!context.path_allow.is_empty()
            && kiana_domain::enforce_path_containment(&context.path_allow, &normalized).is_err())
    {
        return Err(checkpoint_error("checkpoint_path_denied"));
    }
    Ok(())
}
fn checkpoint_error(reason: &str) -> CoreError {
    PortError::Failed(reason.to_owned()).into()
}
