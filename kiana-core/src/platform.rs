//! Local human operations retain the original Approval/Company authorities and event facts.
use super::*;
use kiana_domain::{
    json_digest, AcceptanceStatus, CapabilityErrorCode, CompanyCommand, CompanyCommandRequest,
    CompanyState, FailureClass, FailureIncident, FeedbackCandidate, HumanAction, HumanInboxItem,
    HumanInboxKind, IncidentStatus, RecoveryPlan, RecoveryPlanState, COMPANY_COMMAND_SCHEMA,
};

const PLATFORM_STREAM: &str = "human_operations";

/// Bridge the committed notification projection to the existing display-only inbox DTO.  The
/// materializer owns no action execution; callers still submit an action through the original
/// ControlPlane authority after rechecking the source HumanTask.
pub(crate) fn materialize_notification_inbox(
    materializer: &NotificationMaterializer,
    server_recipient_id: &str,
) -> Result<Vec<HumanInboxItem>, CoreError> {
    materializer
        .human_inbox_items(server_recipient_id)
        .map_err(|reason| platform_error(&reason))
}

impl ControlPlane {
    pub(crate) async fn handle_platform_command(
        &self,
        context: RequestContext,
        name: &str,
        arguments: Value,
    ) -> Result<CoreResponse, CoreError> {
        if let Err(reason) = platform_context(&context) {
            return Ok(CoreResponse::blocked(context.request_id, reason));
        }
        if !arguments.is_object()
            || serde_json::to_vec(&arguments).map_or(true, |bytes| bytes.len() > 64 * 1024)
        {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "human_arguments_invalid",
            ));
        }
        match name {
            "human.inbox" => {
                let items = self.human_items(&context).await?;
                Ok(CoreResponse::completed(
                    context.request_id,
                    json!({"schema":"kiana.human-inbox.v1","revision":json_digest(&json!(items)),"items":items}),
                ))
            }
            "human.resolve" => self.resolve_human_item(context, arguments).await,
            "failure.incidents" => {
                let history = self.platform_history(&context).await?;
                let key = json_digest(
                    &json!({"project_root":Self::canonical_project_root(&context.project_root)}),
                );
                let resources = self.events.read_stream("resource_quarantine", &key).await?;
                let resource_revision = resources
                    .iter()
                    .filter_map(|event| event.stream_version)
                    .max()
                    .unwrap_or(0);
                Ok(CoreResponse::completed(
                    context.request_id,
                    json!({"schema":"kiana.failure-incidents.v1","revision":platform_revision(&history),"resource_revision":resource_revision,"incidents":self.failure_incidents(&context,&history).await?}),
                ))
            }
            "failure.reconcile" => self.reconcile_failure(&context, arguments).await,
            "failure.recovery" => self.advance_recovery_plan(&context, arguments).await,
            "failure.release" => {
                self.release_quarantined_resources(&context, arguments)
                    .await
            }
            "feedback.list" => {
                let history = self.platform_history(&context).await?;
                Ok(CoreResponse::completed(
                    context.request_id,
                    json!({"schema":"kiana.feedback-candidates.v1","revision":platform_revision(&history),"candidates":feedback_candidates(&history)?}),
                ))
            }
            "feedback.submit" | "feedback.review" => {
                self.record_feedback(&context, name, arguments).await
            }
            _ => Ok(CoreResponse::blocked(
                context.request_id,
                "human_command_unknown",
            )),
        }
    }

    async fn platform_history(
        &self,
        context: &RequestContext,
    ) -> Result<Vec<RuntimeEvent>, CoreError> {
        let mut history = self
            .events
            .read_stream(PLATFORM_STREAM, &platform_scope(context))
            .await?;
        history.sort_by_key(|event| event.stream_version.unwrap_or(0));
        for (index, event) in history.iter().enumerate() {
            if event.stream_version != Some(index as u64 + 1)
                || event.data["actor_id"].as_str() != context.actor_id.as_deref()
                || event.data["project_root"].as_str() != Some(platform_root(context).as_str())
            {
                return Err(platform_error("human_history_invalid"));
            }
        }
        Ok(history)
    }

    async fn commit_platform(
        &self,
        context: &RequestContext,
        history: &[RuntimeEvent],
        kind: &str,
        arguments: &Value,
        payload: Value,
    ) -> Result<CoreResponse, CoreError> {
        if context.permission_profile == PermissionProfile::Safe {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "human_write_requires_non_safe_profile",
            ));
        }
        let Some(key) = arguments["idempotency_key"]
            .as_str()
            .filter(|key| !key.trim().is_empty() && key.len() <= 256)
        else {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "human_idempotency_key_required",
            ));
        };
        let digest = json_digest(
            &json!({"kind":kind,"arguments":arguments,"session_id":context.session_id,"role_id":context.role_id}),
        );
        if let Some(event) = history
            .iter()
            .find(|event| event.data["idempotency_key"] == key)
        {
            if event.data["request_digest"] != digest {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "human_idempotency_conflict",
                ));
            }
            return Ok(CoreResponse::completed(
                context.request_id,
                json!({"replayed":true,"event_id":event.event_id,"revision":platform_revision(history),"record":event.data["record"]}),
            ));
        }
        let revision = platform_revision(history);
        if arguments["expected_revision"].as_u64() != Some(revision) {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "human_revision_conflict",
            ));
        }
        let data = json!({"actor_id":context.actor_id,"project_root":platform_root(context),"session_id":context.session_id,"role_id":context.role_id,
            "idempotency_key":key,"request_digest":digest,"record":payload});
        if kiana_domain::redact_value(&data) != data {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "human_sensitive_record_denied",
            ));
        }
        let event = RuntimeEvent::new(context.request_id, 1, kind, data)?.with_stream_metadata(
            PLATFORM_STREAM,
            platform_scope(context),
            revision + 1,
        );
        let event_id = event.event_id;
        self.commit_protected_event(context, event, revision)
            .await?;
        Ok(CoreResponse::completed(
            context.request_id,
            json!({"event_id":event_id,"revision":revision+1,"record":payload}),
        ))
    }

    pub(crate) async fn platform_owned_events(
        &self,
        context: &RequestContext,
    ) -> Result<Vec<RuntimeEvent>, CoreError> {
        let all = self.events.read_all().await?;
        let root = platform_root(context);
        let runs: HashSet<_> = all
            .iter()
            .filter(|event| {
                event.kind == "run.authorized"
                    && event.data["actor_id"].as_str() == context.actor_id.as_deref()
                    && event.data["project_root"].as_str().is_some_and(|path| {
                        Self::canonical_project_root(path) == PathBuf::from(&root)
                    })
            })
            .filter_map(|event| event.data["run_id"].as_str().map(str::to_owned))
            .collect();
        let requests: HashSet<_> = all
            .iter()
            .filter(|event| {
                event.data["run_id"]
                    .as_str()
                    .is_some_and(|id| runs.contains(id))
            })
            .map(|event| event.request_id)
            .collect();
        Ok(all
            .into_iter()
            .filter(|event| {
                requests.contains(&event.request_id)
                    || (event.data["actor_id"].as_str() == context.actor_id.as_deref()
                        && event.data["project_root"].as_str() == Some(root.as_str()))
                    || event.aggregate_id.as_deref()
                        == Some(
                            format!(
                                "{}\n{}",
                                context.actor_id.as_deref().unwrap_or_default(),
                                root
                            )
                            .as_str(),
                        )
            })
            .collect())
    }

    async fn evidence_exists(
        &self,
        context: &RequestContext,
        arguments: &Value,
    ) -> Result<bool, CoreError> {
        let Some(refs) = arguments["evidence_refs"]
            .as_array()
            .filter(|refs| !refs.is_empty() && refs.len() <= 64)
        else {
            return Ok(false);
        };
        let events = self.platform_owned_events(context).await?;
        Ok(refs.iter().all(|value| {
            value
                .as_str()
                .and_then(|value| value.strip_prefix("event:"))
                .is_some_and(|id| events.iter().any(|event| event.event_id.to_string() == id))
        }))
    }

    async fn reconciliation_evidence_exists(
        &self,
        context: &RequestContext,
        arguments: &Value,
        incident: &FailureIncident,
    ) -> Result<bool, CoreError> {
        let Some(refs) = arguments["evidence_refs"]
            .as_array()
            .filter(|refs| !refs.is_empty() && refs.len() <= 64)
        else {
            return Ok(false);
        };
        let source_ref = format!("event:{}", incident.source_event_id);
        let owned_events = self.platform_owned_events(context).await?;
        let mut source_seen = false;
        let mut independent_seen = false;
        for value in refs {
            let Some(reference) = value
                .as_str()
                .filter(|reference| !reference.trim().is_empty() && reference.len() <= 512)
            else {
                return Ok(false);
            };
            if reference == source_ref {
                source_seen = true;
                continue;
            }
            if let Some(event_id) = reference.strip_prefix("event:") {
                let Some(event) = owned_events
                    .iter()
                    .find(|event| event.event_id.to_string() == event_id)
                else {
                    return Ok(false);
                };
                if incident.run_id.is_some()
                    && event.data["run_id"].as_str().and_then(RunId::parse_str) != incident.run_id
                {
                    return Ok(false);
                }
                independent_seen = true;
                continue;
            }
            let typed_external = ["provider:", "os:", "file:", "human:"]
                .iter()
                .any(|prefix| reference.starts_with(prefix));
            if !typed_external || !reference.contains(&incident.incident_id) {
                return Ok(false);
            }
            independent_seen = true;
        }
        Ok(source_seen && independent_seen)
    }

    async fn reconciliation_epoch_current(
        &self,
        context: &RequestContext,
        incident: &FailureIncident,
    ) -> Result<bool, CoreError> {
        let events = self.events.read_all().await?;
        let Some(source_index) = events
            .iter()
            .position(|event| event.event_id == incident.source_event_id)
        else {
            return Ok(false);
        };
        let source = &events[source_index];
        let source_authority = source.data["authority_revision"].as_str();
        let current_authority = self.authority_revision(&context.project_root).await?;
        if source_authority.is_some() && source_authority != current_authority.as_deref() {
            return Ok(false);
        }
        let root = source.data["project_root"]
            .as_str()
            .map(Self::canonical_project_root)
            .unwrap_or_else(|| Self::canonical_project_root(&context.project_root));
        if events[source_index + 1..].iter().any(|event| {
            matches!(
                event.kind.as_str(),
                "data.revocation_requested" | "workspace.restore_requested" | "workspace.restored"
            ) && event.data["project_root"]
                .as_str()
                .is_some_and(|other| Self::canonical_project_root(other) == root)
        }) {
            return Ok(false);
        }
        Ok(true)
    }

    async fn failure_incidents(
        &self,
        context: &RequestContext,
        history: &[RuntimeEvent],
    ) -> Result<Vec<FailureIncident>, CoreError> {
        let events = self.platform_owned_events(context).await?;
        let mut incidents = Vec::new();
        for event in &events {
            if !matches!(
                event.kind.as_str(),
                "run.failed"
                    | "run.result_unknown"
                    | "run.cancelled"
                    | "capability.failed"
                    | "capability.result_unknown"
            ) {
                continue;
            }
            let summary = event
                .data
                .get("error")
                .or_else(|| event.data.get("reason"))
                .and_then(Value::as_str)
                .unwrap_or(&event.kind)
                .to_owned();
            let lower = summary.to_ascii_lowercase();
            let unknown = matches!(
                event.kind.as_str(),
                "run.result_unknown" | "capability.result_unknown" | "execution.result_unknown"
            ) || event
                .data
                .get("effect_known")
                .and_then(Value::as_bool)
                .is_some_and(|known| !known)
                || event
                    .data
                    .get("stop_confirmed")
                    .and_then(Value::as_bool)
                    .is_some_and(|confirmed| !confirmed)
                || event
                    .data
                    .get("error")
                    .and_then(Value::as_str)
                    .map(CapabilityErrorCode::from_reason)
                    .is_some_and(|code| {
                        matches!(
                            code,
                            CapabilityErrorCode::ResultUnknown
                                | CapabilityErrorCode::CompensationRequired
                        )
                    });
            let class = if lower.contains("no space")
                || lower.contains("disk_full")
                || lower.contains("os error 28")
            {
                FailureClass::DiskFull
            } else if lower.contains("timeout") || lower.contains("timed out") {
                FailureClass::Timeout
            } else if lower.contains("mcp") {
                FailureClass::McpFailure
            } else if event.kind == "run.cancelled"
                || event.data.get("cancelled") == Some(&Value::Bool(true))
                || event
                    .data
                    .get("error")
                    .and_then(Value::as_str)
                    .map(CapabilityErrorCode::from_reason)
                    .is_some_and(|code| code == CapabilityErrorCode::Cancelled)
            {
                FailureClass::Cancel
            } else if unknown || lower.contains("provider") {
                FailureClass::ProviderUnknown
            } else {
                FailureClass::Crash
            };
            let id = format!("failure:{}", event.event_id);
            let reconciliation = history
                .iter()
                .rev()
                .find(|record| {
                    record.kind == "failure.reconciled"
                        && record.data["record"]["incident_id"] == id
                })
                .map(|event| event.data["record"].clone());
            let recovery = Self::project_recovery_plan(&history, &id, class.recovery(unknown))?;
            incidents.push(FailureIncident {
                incident_id: id,
                source_event_id: event.event_id,
                run_id: event.data["run_id"].as_str().and_then(RunId::parse_str),
                class,
                summary,
                recovery,
                reconciliation,
            });
        }
        // Missing terminal facts after loss of this host's continuation are an uncertainty,
        // not proof that the external work stopped or is safe to retry.
        let live: HashSet<RunId> = self
            .sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .map(|binding| binding.run_id)
            .collect();
        let mut projected = HashSet::new();
        for identity in events.iter().filter(|event| event.kind == "run.authorized") {
            let Some(run_id) = identity.data["run_id"].as_str().and_then(RunId::parse_str) else {
                continue;
            };
            if !projected.insert(run_id) || live.contains(&run_id) {
                continue;
            }
            let unresolved = crate::project_run_state(run_id, &events)
                .map_or(true, |state| state.outcome.is_none());
            if !unresolved {
                continue;
            }
            let id = format!("failure:{}", identity.event_id);
            let reconciliation = history
                .iter()
                .rev()
                .find(|event| {
                    event.kind == "failure.reconciled" && event.data["record"]["incident_id"] == id
                })
                .map(|event| event.data["record"].clone());
            let recovery =
                Self::project_recovery_plan(&history, &id, FailureClass::Crash.recovery(true))?;
            incidents.push(FailureIncident {
                incident_id: id,
                source_event_id: identity.event_id,
                run_id: Some(run_id),
                class: FailureClass::Crash,
                summary: "run_continuation_unavailable_in_this_host: terminal result and process stop are unconfirmed".to_owned(),
                recovery,
                reconciliation,
            });
        }
        Ok(incidents)
    }

    fn project_recovery_plan(
        history: &[RuntimeEvent],
        incident_id: &str,
        mut plan: RecoveryPlan,
    ) -> Result<RecoveryPlan, CoreError> {
        for event in history.iter().filter(|event| {
            event.kind == "failure.recovery.transition"
                && event.data["record"]["incident_id"] == incident_id
        }) {
            let record = &event.data["record"];
            let next: RecoveryPlanState = serde_json::from_value(record["state"].clone())
                .map_err(|_| platform_error("recovery_plan_history_state_invalid"))?;
            let evidence_refs: Vec<String> = serde_json::from_value(
                record
                    .get("evidence_refs")
                    .cloned()
                    .unwrap_or_else(|| json!([])),
            )
            .map_err(|_| platform_error("recovery_plan_history_evidence_invalid"))?;
            plan.transition_with_evidence(next, evidence_refs)
                .map_err(platform_error)?;
            for field in ["safe_actions", "forbidden_actions"] {
                let actions: Vec<String> =
                    serde_json::from_value(record.get(field).cloned().unwrap_or_else(|| json!([])))
                        .map_err(|_| platform_error("recovery_plan_history_actions_invalid"))?;
                let target = if field == "safe_actions" {
                    &mut plan.safe_actions
                } else {
                    &mut plan.forbidden_actions
                };
                for action in actions {
                    if !action.trim().is_empty() && !target.contains(&action) {
                        target.push(action);
                    }
                }
                target.sort();
            }
        }
        Ok(plan)
    }

    async fn advance_recovery_plan(
        &self,
        context: &RequestContext,
        arguments: Value,
    ) -> Result<CoreResponse, CoreError> {
        let history = self.platform_history(context).await?;
        if let Some(response) =
            platform_replay(context, &history, "failure.recovery.transition", &arguments)
        {
            return Ok(response);
        }
        let Some(incident_id) = bounded_text(&arguments, "incident_id", 256) else {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "recovery_plan_incident_required",
            ));
        };
        let requested: RecoveryPlanState = match serde_json::from_value(arguments["state"].clone())
        {
            Ok(state) => state,
            Err(_) => {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "recovery_plan_state_invalid",
                ))
            }
        };
        let Some(incident) = self
            .failure_incidents(context, &history)
            .await?
            .into_iter()
            .find(|incident| incident.incident_id == incident_id)
        else {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "failure_incident_not_found",
            ));
        };
        if incident.recovery.state.transition(requested).is_err() {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "recovery_plan_state_transition_invalid",
            ));
        }
        if requested == RecoveryPlanState::Approved
            && !matches!(context.role_id.as_str(), "reviewer" | "sponsor")
        {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "recovery_plan_cannot_self_approve",
            ));
        }
        let all_events = self.events.read_all().await?;
        let source_actor = all_events
            .iter()
            .find(|event| event.event_id == incident.source_event_id)
            .and_then(|event| event.data["actor_id"].as_str());
        if requested == RecoveryPlanState::Approved && source_actor == context.actor_id.as_deref() {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "recovery_plan_cannot_self_approve",
            ));
        }
        let Some(evidence_refs) = arguments["evidence_refs"].as_array() else {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "recovery_plan_evidence_required",
            ));
        };
        let source_ref = format!("event:{}", incident.source_event_id);
        if !evidence_refs
            .iter()
            .any(|reference| reference.as_str() == Some(source_ref.as_str()))
            || !self.evidence_exists(context, &arguments).await?
        {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "recovery_plan_evidence_required",
            ));
        }
        self.commit_platform(
            context,
            &history,
            "failure.recovery.transition",
            &arguments,
            json!({
                "incident_id": incident.incident_id,
                "source_event_id": incident.source_event_id,
                "from": incident.recovery.state,
                "state": requested,
                "actor_id": context.actor_id,
                "evidence_refs": evidence_refs,
                "safe_actions": incident.recovery.safe_actions,
                "forbidden_actions": incident.recovery.forbidden_actions,
                "automatic_retry_allowed": false,
            }),
        )
        .await
    }

    async fn release_quarantined_resources(
        &self,
        context: &RequestContext,
        arguments: Value,
    ) -> Result<CoreResponse, CoreError> {
        if context.permission_profile == PermissionProfile::Safe {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "human_write_requires_non_safe_profile",
            ));
        }
        let run_id = arguments["run_id"]
            .as_str()
            .and_then(RunId::parse_str)
            .ok_or_else(|| platform_error("run_id_required"))?;
        let events = self.platform_owned_events(context).await?;
        if !events
            .iter()
            .any(|event| event.kind == "run.authorized" && event.data["run_id"] == json!(run_id))
        {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "run_owner_mismatch",
            ));
        }
        let history = self.platform_history(context).await?;
        if !self
            .failure_incidents(context, &history)
            .await?
            .iter()
            .any(|incident| incident.run_id == Some(run_id) && incident.reconciliation.is_some())
        {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "resource_release_requires_reconciliation",
            ));
        }
        for dispatch in events.iter().filter(|event| {
            event.kind == "invocation.dispatching" && event.data["run_id"] == json!(run_id)
        }) {
            if dispatch.data["execution_id"].is_null()
                || !events.iter().any(|event| {
                    event.kind == "execution.result_committed"
                        && event.data["run_id"] == json!(run_id)
                        && event.data["execution_id"] == dispatch.data["execution_id"]
                        && event.data["stop_confirmed"] == true
                })
            {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "resource_release_stop_unconfirmed",
                ));
            }
        }
        if !self.await_capability_stop(run_id).await {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "resource_release_stop_unconfirmed",
            ));
        }
        let key = json_digest(
            &json!({"project_root":Self::canonical_project_root(&context.project_root)}),
        );
        let records = self.events.read_stream("resource_quarantine", &key).await?;
        if records
            .iter()
            .rev()
            .find(|event| event.data["run_id"] == json!(run_id))
            .is_none_or(|event| event.kind != "resource.quarantined")
        {
            return Ok(CoreResponse::completed(
                context.request_id,
                json!({"run_id":run_id,"already_released":true}),
            ));
        }
        let version = records
            .iter()
            .filter_map(|event| event.stream_version)
            .max()
            .unwrap_or(0);
        if arguments["expected_revision"].as_u64() != Some(version) {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "resource_revision_conflict",
            ));
        }
        let event=RuntimeEvent::new(context.request_id,1,"resource.released",json!({"run_id":run_id,"project_root":context.project_root,
            "actor_id":context.actor_id,"stop_confirmed":true,"original_outcome_unchanged":true,"unknown_usage_refunded":false}))?
            .with_stream_metadata("resource_quarantine",key,version+1);
        self.commit_protected_event(context, event, version).await?;
        if let Some(cell_id) = self.cell_registry.cell_for_run(run_id).await? {
            if let Some(reservation) = self.cell_registry.reservation_for_cell(cell_id).await? {
                if reservation.cell.lifecycle == CellLifecycle::Quarantined {
                    self.cell_registry
                        .retire_cell(cell_id, "human_reconciled_confirmed_stop")
                        .await?;
                }
            }
        }
        Ok(CoreResponse::completed(
            context.request_id,
            json!({"run_id":run_id,"resources_released":true,"revision":version+1,
            "automatic_retry_allowed":false,"new_request_required":true}),
        ))
    }

    async fn reconcile_failure(
        &self,
        context: &RequestContext,
        arguments: Value,
    ) -> Result<CoreResponse, CoreError> {
        let history = self.platform_history(context).await?;
        if let Some(response) = platform_replay(context, &history, "failure.reconciled", &arguments)
        {
            return Ok(response);
        }
        let Some(incident) = self
            .failure_incidents(context, &history)
            .await?
            .into_iter()
            .find(|incident| arguments["incident_id"] == incident.incident_id)
        else {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "failure_incident_not_found",
            ));
        };
        if incident.reconciliation.is_some() {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "failure_already_reconciled",
            ));
        }
        let authority_revision = self.authority_revision(&context.project_root).await?;
        let data_epoch = self.data_epoch(&context.project_root).await?;
        if !matches!(
            arguments["resolution"].as_str(),
            Some("observed_succeeded" | "observed_failed" | "no_effect")
        ) || !self
            .reconciliation_evidence_exists(context, &arguments, &incident)
            .await?
        {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "failure_reconciliation_evidence_required",
            ));
        }
        if !self
            .reconciliation_epoch_current(context, &incident)
            .await?
        {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "failure_reconciliation_evidence_or_epoch_invalid",
            ));
        }
        self.commit_platform(context,&history,"failure.reconciled",&arguments,json!({"incident_id":incident.incident_id,"source_event_id":incident.source_event_id,"resolution":arguments["resolution"],"evidence_refs":arguments["evidence_refs"],
            "authority_revision":authority_revision,"data_epoch":data_epoch,"evidence_scope":"incident_bound",
            "runtime_outcome_unchanged":true,"automatic_retry_allowed":false})).await
    }

    async fn record_feedback(
        &self,
        context: &RequestContext,
        name: &str,
        arguments: Value,
    ) -> Result<CoreResponse, CoreError> {
        let history = self.platform_history(context).await?;
        let event_kind = if name == "feedback.submit" {
            "feedback.candidate_created"
        } else {
            "feedback.candidate_reviewed"
        };
        if let Some(response) = platform_replay(context, &history, event_kind, &arguments) {
            return Ok(response);
        }
        let candidates = feedback_candidates(&history)?;
        if name == "feedback.submit" {
            let Some(category) = arguments["category"].as_str().filter(|value| {
                matches!(
                    *value,
                    "quality" | "workflow" | "prompt" | "reliability" | "usability"
                )
            }) else {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "feedback_category_invalid",
                ));
            };
            let Some(observation) = bounded_text(&arguments, "observation", 8000) else {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "feedback_observation_required",
                ));
            };
            let Some(proposal) = bounded_text(&arguments, "proposed_change", 8000) else {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "feedback_proposal_required",
                ));
            };
            if !self.evidence_exists(context, &arguments).await? {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "feedback_evidence_required",
                ));
            }
            let Some(id) = bounded_text(&arguments, "candidate_id", 128) else {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "feedback_candidate_id_required",
                ));
            };
            if candidates
                .iter()
                .any(|candidate| candidate.candidate_id == id)
            {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "feedback_candidate_exists",
                ));
            }
            let candidate = FeedbackCandidate {
                candidate_id: id.to_owned(),
                submitted_by: context.actor_id.clone().unwrap_or_default(),
                session_id: context.session_id.clone(),
                category: category.to_owned(),
                observation: observation.to_owned(),
                proposed_change: proposal.to_owned(),
                evidence_refs: serde_json::from_value(arguments["evidence_refs"].clone())
                    .map_err(|_| platform_error("feedback_evidence_invalid"))?,
                submitted_at_ms: platform_now(),
                review: None,
            };
            self.commit_platform(
                context,
                &history,
                "feedback.candidate_created",
                &arguments,
                json!(candidate),
            )
            .await
        } else {
            let Some(candidate) = candidates
                .into_iter()
                .find(|candidate| arguments["candidate_id"] == candidate.candidate_id)
            else {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "feedback_candidate_not_found",
                ));
            };
            if !matches!(context.role_id.as_str(), "reviewer" | "sponsor")
                || context.session_id == candidate.session_id
            {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "feedback_independent_reviewer_required",
                ));
            }
            if candidate.review.is_some() {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "feedback_already_reviewed",
                ));
            }
            if !matches!(
                arguments["outcome"].as_str(),
                Some("quality_approved" | "rejected")
            ) || !self.evidence_exists(context, &arguments).await?
            {
                return Ok(CoreResponse::blocked(
                    context.request_id,
                    "feedback_quality_gate_required",
                ));
            }
            self.commit_platform(context,&history,"feedback.candidate_reviewed",&arguments,json!({"candidate_id":candidate.candidate_id,"outcome":arguments["outcome"],"evidence_refs":arguments["evidence_refs"],
                "reviewer_session_id":context.session_id,"promotion":"candidate_only","authority_changes_applied":false})).await
        }
    }

    async fn human_items(
        &self,
        context: &RequestContext,
    ) -> Result<Vec<HumanInboxItem>, CoreError> {
        let history = self.platform_history(context).await?;
        let revision = platform_revision(&history);
        let mut items = Vec::new();
        let pending_response = self.list_pending_approvals(context, None).await?;
        if pending_response.status != ExecutionStatus::Completed {
            return Err(platform_error("human_approval_list_unavailable"));
        }
        let pending_approvals: Vec<kiana_domain::ApprovalView> =
            serde_json::from_value(pending_response.output["approvals"].clone())
                .map_err(|_| platform_error("human_approval_list_invalid"))?;
        for pending in pending_approvals {
            let id = pending.challenge.approval_id.to_string();
            items.push(HumanInboxItem{item_id:format!("approval:{id}"),kind:HumanInboxKind::Approval,title:pending.operation.clone(),source_ref:format!("approval:{id}"),run_id:None,
                detail:json!({"challenge":pending.challenge,"arguments":kiana_domain::redact_value(&pending.arguments),"expected_version":pending.expected_version}),actions:[("approve","批准"),("deny","拒绝")].into_iter().map(|(decision,label)|action(decision,label,"approval",json!({"approval_id":id,"decision":decision,"request_hash":pending.challenge.request_hash,"nonce":pending.challenge.nonce,"expected_version":pending.expected_version}),&[])).collect()});
        }
        let snapshot = self.company_snapshot(context.clone()).await?;
        if snapshot.status != ExecutionStatus::Completed {
            return Err(platform_error("human_company_snapshot_unavailable"));
        }
        let company: CompanyState = serde_json::from_value(snapshot.output["state"].clone())
            .map_err(|_| platform_error("human_company_snapshot_invalid"))?;
        items.extend(self.company_business_inbox(context, &company));
        for delivery in company.business.closeout.deliveries.values() {
            let (id, label, fields, arguments) = match delivery.status {
                kiana_domain::BusinessDeliveryStatus::Approved
                    if matches!(context.role_id.as_str(), "closer" | "sponsor") =>
                {
                    (
                        "dispatch_delivery",
                        "生成本地交付包",
                        Vec::new(),
                        json!({"type":"closeout","schema":kiana_domain::COMPANY_BUSINESS_SCHEMA,"action":{"type":"dispatch_delivery","delivery_id":delivery.manifest.delivery_id}}),
                    )
                }
                kiana_domain::BusinessDeliveryStatus::DispatchRequested
                    if matches!(context.role_id.as_str(), "closer" | "sponsor") =>
                {
                    (
                        "reconcile_delivery",
                        "核对交付效果",
                        Vec::new(),
                        json!({"type":"closeout","schema":kiana_domain::COMPANY_BUSINESS_SCHEMA,"action":{"type":"reconcile_delivery","delivery_id":delivery.manifest.delivery_id}}),
                    )
                }
                kiana_domain::BusinessDeliveryStatus::Delivered
                    if delivery.manifest.recipient
                        == context.actor_id.as_deref().unwrap_or_default() =>
                {
                    (
                        "confirm_delivery",
                        "确认收到交付包",
                        vec!["package_sha256".into()],
                        json!({"type":"closeout","schema":kiana_domain::COMPANY_BUSINESS_SCHEMA,"action":{"type":"confirm_delivery","delivery_id":delivery.manifest.delivery_id,"manifest_digest":delivery.digest}}),
                    )
                }
                kiana_domain::BusinessDeliveryStatus::ResultUnknown
                    if matches!(context.role_id.as_str(), "closer" | "sponsor") =>
                {
                    (
                        "reconcile_delivery",
                        "处理未知交付结果",
                        Vec::new(),
                        json!({"type":"closeout","schema":kiana_domain::COMPANY_BUSINESS_SCHEMA,"action":{"type":"reconcile_delivery","delivery_id":delivery.manifest.delivery_id}}),
                    )
                }
                _ => continue,
            };
            items.push(HumanInboxItem {
                item_id: format!("company-delivery:{}", delivery.manifest.delivery_id),
                kind: HumanInboxKind::Acceptance,
                title: label.into(),
                source_ref: format!("company:delivery:{}", delivery.manifest.delivery_id),
                run_id: None,
                detail: json!({"delivery":delivery,"company_revision":company.revision}),
                actions: vec![HumanAction {
                    id: id.into(),
                    label: label.into(),
                    command: "company.business".into(),
                    arguments,
                    required_fields: fields,
                }],
            });
        }
        for change in company.business.closeout.changes.values().filter(|change| {
            change.status == kiana_domain::ChangeStatus::PendingDecision
                && context.role_id == "sponsor"
        }) {
            items.push(HumanInboxItem {
                item_id: format!("company-change:{}", change.change_id),
                kind: HumanInboxKind::Acceptance,
                title: "审批基线变更".into(),
                source_ref: format!("company:change:{}", change.change_id),
                run_id: None,
                detail: json!({"change":change,"company_revision":company.revision}),
                actions: vec![HumanAction { id: "decide_change".into(), label: "决定变更".into(), command: "company.business".into(), arguments: json!({"type":"closeout","schema":kiana_domain::COMPANY_BUSINESS_SCHEMA,"action":{"type":"decide_change","change_id":change.change_id,"digest":change.digest,"approve":false,"decision_ref":""}}), required_fields: vec!["approve".into(), "decision_ref".into()] }],
            });
        }
        for project in company
            .projects
            .values()
            .filter(|project| project.status == kiana_domain::ProjectStatus::CancelRequested)
        {
            if matches!(context.role_id.as_str(), "sponsor" | "closer" | "pm") {
                items.push(HumanInboxItem {
                    item_id: format!("company-cancel:{}", project.project_id),
                    kind: HumanInboxKind::Reconciliation,
                    title: "核对项目取消".into(),
                    source_ref: format!("company:cancel:{}", project.project_id),
                    run_id: None,
                    detail: json!({"project":project,"company_revision":company.revision}),
                    actions: vec![HumanAction { id: "reconcile_cancel".into(), label: "核对停止结果".into(), command: "company.business".into(), arguments: json!({"type":"closeout","schema":kiana_domain::COMPANY_BUSINESS_SCHEMA,"action":{"type":"reconcile_cancel","project_id":project.project_id}}), required_fields: Vec::new() }],
                });
            }
        }
        for acceptance in company.acceptances.values() {
            let (kind, actions) = match acceptance.status {
                AcceptanceStatus::Requested | AcceptanceStatus::EvidencePending => (
                    HumanInboxKind::Review,
                    if context.role_id == "reviewer"
                        && context.session_id != acceptance.author_session_id
                    {
                        vec![action(
                            "record_review",
                            "提交复核",
                            "company",
                            json!({"type":"record_review","acceptance_id":acceptance.acceptance_id}),
                            &["review_id", "criterion_results", "evidence_refs"],
                        )]
                    } else {
                        vec![]
                    },
                ),
                AcceptanceStatus::ReadyForDecision => (
                    HumanInboxKind::Acceptance,
                    if matches!(context.role_id.as_str(), "reviewer" | "sponsor") {
                        ["accept","reject","waive"].into_iter().filter(|decision|*decision!="waive"||context.role_id=="sponsor").map(|decision|action(decision,decision,"company",json!({"type":"decide_acceptance","acceptance_id":acceptance.acceptance_id,"decision":decision}),&["review_id","reasons","waiver_ref"])).collect()
                    } else {
                        vec![]
                    },
                ),
                _ => continue,
            };
            items.push(HumanInboxItem {
                item_id: format!("acceptance:{}", acceptance.acceptance_id),
                kind,
                title: acceptance.acceptance_id.clone(),
                source_ref: format!("company:acceptance:{}", acceptance.acceptance_id),
                run_id: Some(acceptance.author_run_id),
                detail: json!({"acceptance":acceptance,"company_revision":company.revision}),
                actions,
            });
        }
        for incident in company.incidents.values().filter(|incident| {
            !matches!(
                incident.status,
                IncidentStatus::Resolved | IncidentStatus::Closed
            )
        }) {
            let actions=[("triaged",IncidentStatus::Triaged),("assigned",IncidentStatus::Assigned),("mitigating",IncidentStatus::Mitigating),("monitoring",IncidentStatus::Monitoring),("resolved",IncidentStatus::Resolved),("escalated",IncidentStatus::Escalated),("closed",IncidentStatus::Closed)].into_iter()
                .filter(|(_,status)|incident.status.transition(*status).is_ok()).map(|(label,status)|action(label,label,"company",json!({"type":"advance_incident","incident_id":incident.incident_id,"status":status}),&["evidence_refs"])).collect();
            items.push(HumanInboxItem {
                item_id: format!("incident:{}", incident.incident_id),
                kind: HumanInboxKind::Incident,
                title: incident.impact.clone(),
                source_ref: format!("company:incident:{}", incident.incident_id),
                run_id: incident.run_id,
                detail: json!({"incident":incident,"company_revision":company.revision}),
                actions,
            });
        }
        for incident in self
            .failure_incidents(context, &history)
            .await?
            .into_iter()
            .filter(|incident| {
                incident.reconciliation.is_none() || !incident.recovery.state.is_terminal()
            })
        {
            let mut actions = vec![action(
                "reconcile",
                "提交对账证据",
                "failure.reconcile",
                json!({"incident_id":incident.incident_id,"expected_revision":revision}),
                &["resolution", "evidence_refs"],
            )];
            for (state, label) in [
                (RecoveryPlanState::Approved, "批准恢复计划"),
                (RecoveryPlanState::Executing, "开始恢复计划"),
                (RecoveryPlanState::Verified, "确认恢复完成"),
                (RecoveryPlanState::Failed, "标记恢复失败"),
                (RecoveryPlanState::Abandoned, "放弃恢复计划"),
            ] {
                if incident.recovery.state.transition(state).is_ok()
                    && (state != RecoveryPlanState::Approved
                        || matches!(context.role_id.as_str(), "reviewer" | "sponsor"))
                {
                    actions.push(action(
                        &format!("recovery_{label}"),
                        label,
                        "failure.recovery",
                        json!({"incident_id":incident.incident_id,"state":state,"expected_revision":revision}),
                        &["evidence_refs"],
                    ));
                }
            }
            items.push(HumanInboxItem {
                item_id: incident.incident_id.clone(),
                kind: if incident.recovery.requires_reconciliation {
                    HumanInboxKind::Reconciliation
                } else {
                    HumanInboxKind::Incident
                },
                title: incident.summary.clone(),
                source_ref: format!("event:{}", incident.source_event_id),
                run_id: incident.run_id,
                detail: json!(incident),
                actions,
            });
        }
        for candidate in feedback_candidates(&history)?
            .into_iter()
            .filter(|candidate| candidate.review.is_none())
        {
            let actions = if matches!(context.role_id.as_str(), "reviewer" | "sponsor")
                && context.session_id != candidate.session_id
            {
                ["quality_approved","rejected"].into_iter().map(|outcome|action(outcome,outcome,"feedback.review",json!({"candidate_id":candidate.candidate_id,"outcome":outcome,"expected_revision":revision}),&["evidence_refs"])).collect()
            } else {
                vec![]
            };
            items.push(HumanInboxItem {
                item_id: format!("feedback:{}", candidate.candidate_id),
                kind: HumanInboxKind::Feedback,
                title: candidate.observation.clone(),
                source_ref: format!("feedback:{}", candidate.candidate_id),
                run_id: None,
                detail: json!(candidate),
                actions,
            });
        }
        items.sort_by(|a, b| a.item_id.cmp(&b.item_id));
        Ok(items)
    }

    async fn resolve_human_item(
        &self,
        context: RequestContext,
        arguments: Value,
    ) -> Result<CoreResponse, CoreError> {
        let items = self.human_items(&context).await?;
        if arguments["inbox_revision"] != json_digest(&json!(items)) {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "human_inbox_stale",
            ));
        }
        let Some(item) = items
            .into_iter()
            .find(|item| arguments["item_id"] == item.item_id)
        else {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "human_item_not_found",
            ));
        };
        let Some(action) = item
            .actions
            .iter()
            .find(|action| arguments["action_id"] == action.id)
        else {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "human_action_unavailable",
            ));
        };
        let mut resolved = action.arguments.clone();
        if let Some(extra) = arguments["fields"].as_object() {
            for (key, value) in extra {
                if !action.required_fields.contains(key) {
                    return Ok(CoreResponse::blocked(
                        context.request_id,
                        "human_action_field_denied",
                    ));
                }
                resolved[key] = value.clone();
            }
        }
        if action
            .required_fields
            .iter()
            .any(|field| resolved.get(field).is_none())
        {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "human_action_fields_required",
            ));
        }
        match action.command.as_str() {
            "approval" => {
                let id: ApprovalId = serde_json::from_value(resolved["approval_id"].clone())
                    .map_err(|_| platform_error("approval_id_invalid"))?;
                let decision: ApprovalDecision =
                    serde_json::from_value(resolved["decision"].clone())
                        .map_err(|_| platform_error("approval_decision_invalid"))?;
                self.decide_approval_with_proof(
                    &context,
                    id,
                    decision,
                    resolved["request_hash"].as_str(),
                    resolved["nonce"].as_str(),
                )
                .await
            }
            "company.business" => {
                let action: kiana_domain::CompanyBusinessAction = serde_json::from_value(resolved)
                    .map_err(|_| platform_error("human_business_action_invalid"))?;
                let request = CompanyCommandRequest {
                    schema: COMPANY_COMMAND_SCHEMA.into(),
                    expected_revision: item.detail["company_revision"].as_u64().unwrap_or(u64::MAX),
                    idempotency_key: arguments["idempotency_key"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                    command: CompanyCommand::Business {
                        schema: kiana_domain::COMPANY_BUSINESS_SCHEMA.into(),
                        action: Box::new(action),
                    },
                };
                Box::pin(self.handle_company_command(context, json!(request))).await
            }
            "company" => {
                let command: CompanyCommand = serde_json::from_value(resolved)
                    .map_err(|_| platform_error("human_company_action_invalid"))?;
                let key = arguments["idempotency_key"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                let request = CompanyCommandRequest {
                    schema: COMPANY_COMMAND_SCHEMA.to_owned(),
                    expected_revision: item.detail["company_revision"].as_u64().unwrap_or(u64::MAX),
                    idempotency_key: key,
                    command,
                };
                self.handle_company_command(context, json!(request)).await
            }
            "failure.reconcile" | "failure.recovery" | "feedback.review" => {
                resolved["idempotency_key"] = arguments["idempotency_key"].clone();
                if action.command == "failure.reconcile" {
                    self.reconcile_failure(&context, resolved).await
                } else if action.command == "failure.recovery" {
                    self.advance_recovery_plan(&context, resolved).await
                } else {
                    self.record_feedback(&context, "feedback.review", resolved)
                        .await
                }
            }
            _ => Ok(CoreResponse::blocked(
                context.request_id,
                "human_action_unavailable",
            )),
        }
    }
}

fn feedback_candidates(history: &[RuntimeEvent]) -> Result<Vec<FeedbackCandidate>, CoreError> {
    let mut candidates = std::collections::BTreeMap::new();
    for event in history {
        if event.kind == "feedback.candidate_created" {
            let candidate: FeedbackCandidate = serde_json::from_value(event.data["record"].clone())
                .map_err(|_| platform_error("feedback_history_invalid"))?;
            candidates.insert(candidate.candidate_id.clone(), candidate);
        } else if event.kind == "feedback.candidate_reviewed" {
            let id = event.data["record"]["candidate_id"]
                .as_str()
                .ok_or_else(|| platform_error("feedback_history_invalid"))?;
            let candidate = candidates
                .get_mut(id)
                .ok_or_else(|| platform_error("feedback_history_invalid"))?;
            if candidate.review.is_some() {
                return Err(platform_error("feedback_review_conflict"));
            }
            candidate.review = Some(event.data["record"].clone());
        }
    }
    Ok(candidates.into_values().collect())
}
fn platform_replay(
    context: &RequestContext,
    history: &[RuntimeEvent],
    kind: &str,
    arguments: &Value,
) -> Option<CoreResponse> {
    let key = arguments["idempotency_key"].as_str()?;
    let event = history
        .iter()
        .find(|event| event.data["idempotency_key"] == key)?;
    let digest = json_digest(
        &json!({"kind":kind,"arguments":arguments,"session_id":context.session_id,"role_id":context.role_id}),
    );
    Some(if event.data["request_digest"] == digest {
        CoreResponse::completed(
            context.request_id,
            json!({"replayed":true,"event_id":event.event_id,"revision":platform_revision(history),"record":event.data["record"]}),
        )
    } else {
        CoreResponse::blocked(context.request_id, "human_idempotency_conflict")
    })
}
fn action(id: &str, label: &str, command: &str, arguments: Value, fields: &[&str]) -> HumanAction {
    HumanAction {
        id: id.to_owned(),
        label: label.to_owned(),
        command: command.to_owned(),
        arguments,
        required_fields: fields.iter().map(|field| (*field).to_owned()).collect(),
    }
}
pub(crate) fn platform_root(context: &RequestContext) -> String {
    ControlPlane::canonical_project_root(&context.project_root)
        .to_string_lossy()
        .into_owned()
}
pub(crate) fn platform_context(context: &RequestContext) -> Result<(), &'static str> {
    if !context.project_trusted {
        return Err("project_untrusted");
    }
    if context.cell_id.is_some() || context.actor_id.as_deref().is_none_or(str::is_empty) {
        return Err("human_operator_required");
    }
    let role = RoleSpec::lookup(&context.role_id).ok_or("role_unknown")?;
    if role.department_id != context.department_id {
        return Err("role_department_mismatch");
    }
    Ok(())
}
fn platform_scope(context: &RequestContext) -> String {
    json_digest(&json!({"actor_id":context.actor_id,"project_root":platform_root(context)}))
}
fn platform_revision(events: &[RuntimeEvent]) -> u64 {
    events
        .last()
        .and_then(|event| event.stream_version)
        .unwrap_or(0)
}
fn platform_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}
fn platform_error(reason: &str) -> CoreError {
    PortError::Failed(reason.to_owned()).into()
}
fn bounded_text<'a>(value: &'a Value, key: &str, max: usize) -> Option<&'a str> {
    value[key]
        .as_str()
        .filter(|value| !value.trim().is_empty() && value.len() <= max)
}
