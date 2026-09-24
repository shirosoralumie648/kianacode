use super::*;
use kiana_domain::{
    CapabilityErrorCode, DataGovernanceSnapshot, DataPayloadState, DataPolicy, DataPropagationPlan,
    ImmutableEventSeal, ReceiptAuditMetadata, ReceiptDataBinding, ReceiptPayloadRef, RuntimeEvent,
    MAX_SOURCE_EVENT_IDS,
};
use serde_json::Value;
use std::collections::HashSet;

/// Keep receipt audit metadata and payload references as separate, independently governed data.
/// This helper only derives digest references from committed events; it never copies event or
/// artifact bytes into a receipt.
pub fn receipt_data_binding_from_events(
    receipt_digest: &str,
    project_ref: &str,
    policy: &DataPolicy,
    events: &[RuntimeEvent],
) -> Result<ReceiptDataBinding, String> {
    policy.validate()?;
    if events.is_empty() {
        return Err("receipt_data_source_empty".to_owned());
    }
    let mut source_event_ids = Vec::with_capacity(events.len().min(MAX_SOURCE_EVENT_IDS));
    let mut seen = HashSet::new();
    let mut payload_refs = Vec::new();
    for event in events.iter().take(MAX_SOURCE_EVENT_IDS) {
        if seen.insert(event.event_id) {
            source_event_ids.push(event.event_id);
        }
        for reference in &event.artifact_refs {
            if payload_refs.len() >= kiana_domain::MAX_RECEIPT_PAYLOAD_REFS {
                return Err("receipt_payload_ref_limit".to_owned());
            }
            payload_refs.push(ReceiptPayloadRef {
                object_ref: reference.clone(),
                source_digest: kiana_domain::json_digest(&event.data),
                data_epoch: event.data_epoch.unwrap_or(policy.data_epoch),
                payload_digest: kiana_domain::json_digest(&event.data),
            });
        }
    }
    let source_cursor = events.len().min(u64::MAX as usize) as u64;
    let redaction_profile = events
        .iter()
        .rev()
        .find_map(|event| event.redaction_profile.clone())
        .unwrap_or_else(|| "unredacted-source-profile".to_owned());
    let metadata = ReceiptAuditMetadata {
        project_ref: project_ref.to_owned(),
        policy_revision: policy.revision.max(1),
        data_epoch: policy.data_epoch,
        source_cursor,
        source_event_ids,
        redaction_profile,
        retained: true,
    };
    // A receipt reference is never an authorization grant. Without a fresh governance snapshot
    // the payload remains Unknown even when the event was committed successfully.
    ReceiptDataBinding::new(
        receipt_digest,
        metadata,
        payload_refs,
        DataPayloadState::Unknown,
    )
}

/// Build the bounded propagation plan after a tombstone/epoch change. Event facts remain sealed;
/// adapters must append target receipts before a target can be reported as complete.
pub fn plan_data_propagation(
    snapshot: &DataGovernanceSnapshot,
    previous_epoch: u64,
    kind: &str,
    tombstone_digest: &str,
    observed_at_ms: u64,
) -> Result<DataPropagationPlan, String> {
    DataPropagationPlan::from_snapshot(
        snapshot,
        previous_epoch,
        kind,
        tombstone_digest,
        observed_at_ms,
    )
}

/// Seal the immutable event boundary used by a governance projection. Revoke/delete/expire may
/// invalidate projections and bytes, but they cannot rewrite or remove committed source facts.
pub fn seal_governance_events(
    events: &[RuntimeEvent],
    data_epoch: u64,
    encryption_profile: &str,
) -> Result<ImmutableEventSeal, String> {
    if events.is_empty() {
        return Err("immutable_event_seal_source_empty".to_owned());
    }
    let source_event_ids = events
        .iter()
        .map(|event| event.event_id)
        .take(MAX_SOURCE_EVENT_IDS)
        .collect::<Vec<_>>();
    ImmutableEventSeal::new(
        events.len().min(u64::MAX as usize) as u64,
        data_epoch,
        source_event_ids,
        encryption_profile,
    )
}

/// Rebuild data visibility and derived-store propagation from a server-owned policy and the
/// committed invalidation facts. This is a projection only: it never erases EventLog facts.
pub fn project_data_governance_snapshot(
    policy: &DataPolicy,
    project_ref: &str,
    events: &[RuntimeEvent],
    now_ms: u64,
) -> Result<DataGovernanceSnapshot, String> {
    if events.is_empty() {
        return Err("data_governance_source_empty".to_owned());
    }
    let first_cursor = events
        .first()
        .and_then(|event| event.data.get("source_cursor"))
        .and_then(Value::as_u64)
        .unwrap_or(1);
    if first_cursor == 0 || events.len() > MAX_SOURCE_EVENT_IDS {
        return Err("data_governance_source_cursor_invalid".to_owned());
    }
    let mut ids = Vec::with_capacity(events.len());
    let mut seen = HashSet::new();
    let mut revoked_sources = HashSet::new();
    let mut pending_invalidation = false;
    let mut last_cursor = first_cursor;
    for (index, event) in events.iter().enumerate() {
        if !seen.insert(event.event_id.to_string()) {
            return Err("data_governance_source_event_duplicate".to_owned());
        }
        ids.push(event.event_id);
        let expected = first_cursor
            .checked_add(index as u64)
            .ok_or_else(|| "data_governance_source_cursor_overflow".to_owned())?;
        last_cursor = expected;
        if let Some(cursor) = event.data.get("source_cursor").and_then(Value::as_u64) {
            if cursor != expected {
                return Err("data_governance_source_cursor_gap".to_owned());
            }
        }
        if event.kind == "data.revocation_requested" || event.kind == "workspace.restore_requested"
        {
            pending_invalidation = true;
        }
        let output = if event.kind == "execution.result_committed" {
            event
                .data
                .get("result")
                .and_then(|result| result.get("output"))
        } else {
            Some(&event.data)
        };
        if let Some(output) = output {
            if output.get("schema").and_then(Value::as_str)
                == Some("kiana.data-governance-result.v1")
            {
                pending_invalidation = false;
                if let Some(paths) = output
                    .pointer("/policy/revoked_sources")
                    .and_then(Value::as_array)
                {
                    revoked_sources
                        .extend(paths.iter().filter_map(Value::as_str).map(str::to_owned));
                }
            }
        }
    }
    let mut snapshot =
        kiana_domain::project_data_governance(policy, project_ref, last_cursor, ids, now_ms)?;
    kiana_domain::MemoryProjectionFence::validate_epoch(policy.data_epoch, snapshot.data_epoch)?;
    if pending_invalidation {
        for observation in &mut snapshot.observations {
            observation.payload = DataPayloadState::Unknown;
        }
        for state in snapshot.derived_store_states.values_mut() {
            *state = DataPayloadState::Unknown;
        }
    } else if !revoked_sources.is_empty() {
        for observation in &mut snapshot.observations {
            if revoked_sources.contains(&observation.source_ref) {
                observation.payload = DataPayloadState::Revoked;
            }
        }
        if snapshot
            .observations
            .iter()
            .any(|observation| observation.payload == DataPayloadState::Revoked)
        {
            for state in snapshot.derived_store_states.values_mut() {
                *state = DataPayloadState::Revoked;
            }
        }
    }
    snapshot.snapshot_digest = snapshot.digest();
    snapshot.validate()?;
    Ok(snapshot)
}

pub fn project_data_governance(
    policy: &DataPolicy,
    project_ref: &str,
    events: &[RuntimeEvent],
    now_ms: u64,
) -> Result<DataGovernanceSnapshot, String> {
    project_data_governance_snapshot(policy, project_ref, events, now_ms)
}
impl ControlPlane {
    pub(crate) async fn begin_project_invalidation(
        &self,
        root: &str,
        kind: &str,
        data: Value,
    ) -> Result<(), CoreError> {
        let all = self.events.read_all().await?;
        let runs = all
            .iter()
            .filter(|event| {
                event.kind == "run.authorized"
                    && event.data["project_root"].as_str().is_some_and(|value| {
                        Self::canonical_project_root(value) == Self::canonical_project_root(root)
                    })
            })
            .filter_map(|event| event.data["run_id"].as_str().and_then(RunId::parse_str))
            .collect::<std::collections::HashSet<_>>();
        let command_id = RequestId::new();
        let mut versions = vec![kiana_domain::AggregateVersion::new(
            "request",
            command_id.to_string(),
            0,
        )];
        let mut events = vec![RuntimeEvent::new(command_id, 1, kind, data.clone())?
            .with_stream_metadata("request", command_id.to_string(), 1)];
        for run_id in runs {
            let run = self.events.read_stream("run", &run_id.to_string()).await?;
            let turn = run
                .iter()
                .rposition(|event| event.kind == "run.prompt")
                .unwrap_or(0);
            if run[turn..].iter().any(|event| {
                matches!(
                    event.kind.as_str(),
                    "run.completed" | "run.failed" | "run.cancelled" | "run.result_unknown"
                )
            }) {
                continue;
            }
            let version = run
                .iter()
                .filter_map(|event| event.stream_version)
                .max()
                .unwrap_or(0);
            versions.push(kiana_domain::AggregateVersion::new(
                "run",
                run_id.to_string(),
                version,
            ));
            events.push(
                RuntimeEvent::new(
                    command_id,
                    events.len() as u64 + 1,
                    "run.cancelling",
                    json!({"run_id":run_id,"reason":kind,"project_root":root}),
                )?
                .with_stream_metadata("run", run_id.to_string(), version + 1),
            );
        }
        super::dispatch::commit_confirmed(
            self.events.as_ref(),
            kiana_domain::TransitionBatch {
                command_id,
                command_digest: kiana_domain::json_digest(&json!({"kind":kind,"data":data})),
                expected_versions: versions,
                events,
            },
        )
        .await?;
        Ok(())
    }
    pub(crate) async fn data_epoch(&self, project_root: &str) -> Result<Option<String>, CoreError> {
        let events = self.events.read_all().await?;
        Ok(events
            .iter()
            .rev()
            .find(|event| {
                matches!(
                    event.kind.as_str(),
                    "data.revocation_requested"
                        | "workspace.restore_requested"
                        | "workspace.restored"
                ) && event.data["project_root"].as_str().is_some_and(|root| {
                    Self::canonical_project_root(root) == Self::canonical_project_root(project_root)
                })
            })
            .map(|event| event.event_id.to_string()))
    }
    pub(crate) async fn run_data_revoked(&self, run_id: RunId) -> Result<bool, CoreError> {
        let events = self.events.read_all().await?;
        let Some((index, author)) = events.iter().enumerate().find(|(_, event)| {
            event.kind == "run.authorized" && event.data["run_id"] == json!(run_id)
        }) else {
            return Ok(false);
        };
        let Some(root) = author.data["project_root"].as_str() else {
            return Ok(false);
        };
        Ok(events[index + 1..].iter().any(|event| {
            event.kind == "data.revocation_requested"
                && event.data["project_root"].as_str().is_some_and(|other| {
                    Self::canonical_project_root(root) == Self::canonical_project_root(other)
                })
        }))
    }

    pub(crate) async fn prepare_data_revocation(
        &self,
        request: &CapabilityRequest,
    ) -> Result<(), CoreError> {
        if request.operation != "data.governance"
            || !matches!(
                request.arguments["action"].as_str(),
                Some("delete" | "revoke" | "expire")
            )
        {
            return Ok(());
        }
        let root = request.arguments["project_root"]
            .as_str()
            .ok_or_else(|| PortError::Failed("governance_project_required".to_owned()))?;
        let actor = request.arguments["actor_id"]
            .as_str()
            .ok_or_else(|| PortError::Failed("governance_actor_required".to_owned()))?;
        self.begin_project_invalidation(root,"data.revocation_requested",json!({
            "project_root":root,"actor_id":actor,"request_id":request.request_id,"grant_id":request.arguments["grant_id"],
        })).await?;
        self.approvals
            .invalidate_project(root, "data_revoked")
            .await?;
        self.stop_project_runs(root, "data_revoked").await
    }

    pub(crate) async fn stop_project_runs(
        &self,
        root: &str,
        reason: &str,
    ) -> Result<(), CoreError> {
        let sessions = self
            .sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(_, binding)| {
                Self::canonical_project_root(&binding.project_root)
                    == Self::canonical_project_root(root)
            })
            .map(|(session, binding)| (session.clone(), binding.clone()))
            .collect::<Vec<_>>();
        for (session, binding) in sessions {
            let request_id = RequestId::new();
            let mut sequence = 1;
            let terminal = self.run_state(binding.run_id).await?.outcome;
            let pending = self
                .pending_invocations
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .values()
                .find(|pending| pending.run_id == binding.run_id)
                .cloned();
            let snapshot = self
                .events
                .read_all()
                .await?
                .into_iter()
                .rev()
                .filter(|event| {
                    event.kind == "run.snapshot" && event.data["run_id"] == json!(binding.run_id)
                })
                .find_map(|event| {
                    serde_json::from_value::<kiana_domain::RunSnapshot>(
                        event.data["snapshot"].clone(),
                    )
                    .ok()
                });
            if let Some(pending) = &pending {
                self.record_event(request_id,&mut sequence,"run.tool_result",json!({"run_id":binding.run_id,
                    "capability_request_id":pending.request_id,"call_id":pending.request.arguments["call_id"],
                    "result":{"error":format!("cancelled:{reason}"),"not_executed":true,"replay_safe":true},"not_executed":true})).await?;
            }
            let (mut context, sandbox) = if let Some(pending) = pending {
                (pending.context, pending.sandbox)
            } else if let Some(snapshot) = snapshot {
                (snapshot.context, snapshot.sandbox)
            } else {
                let mut context =
                    RequestContext::local(session.as_str(), binding.project_root.clone());
                context.actor_id = binding.actor_id.clone();
                context.role_id = binding.role_id.clone();
                context.department_id = binding.department_id.clone();
                context.cell_id = self.cell_registry.cell_for_run(binding.run_id).await?;
                (context, "read-only".to_owned())
            };
            context.request_id = request_id;
            self.signal_cancel(binding.run_id);
            let cancelled = self
                .runner
                .send(RunnerCommand::Cancel {
                    run_id: binding.run_id,
                    reason: reason.to_owned(),
                })
                .await;
            let runner_confirmed = cancelled.as_ref().is_ok_and(|events| {
                events.iter().all(|event| {
                    event.run_id() == binding.run_id
                        && !matches!(event,RunnerEvent::Failed {error,..}
                    if CapabilityErrorCode::from_reason(error) != CapabilityErrorCode::Cancelled
                        && !(error=="run_not_found"&&terminal.is_some()))
                })
            });
            let confirmed = self.await_capability_stop(binding.run_id).await && runner_confirmed;
            if let Ok(events) = cancelled {
                for event in events {
                    if let RunnerEvent::ToolCancelled {
                        run_id,
                        request_id: capability_id,
                        call_id,
                        result,
                    } = event
                    {
                        self.record_event(request_id,&mut sequence,"run.tool_result",json!({"run_id":run_id,
                            "capability_request_id":capability_id,"call_id":call_id,"result":result,"not_executed":true})).await?;
                    }
                }
            }
            self.pending_invocations
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .retain(|_, pending| pending.run_id != binding.run_id);
            if terminal.is_none() {
                self.record_terminal_event(request_id,&mut sequence,binding.run_id,
                    if confirmed {"run.cancelled"} else {"run.result_unknown"},
                    json!({"run_id":binding.run_id,"error":format!("{}:{reason}",if confirmed {"cancelled"}else{"result_unknown:stop_unconfirmed"})})).await?;
                self.settle_resumed_cell(
                    &context,
                    binding.run_id,
                    &sandbox,
                    if confirmed {
                        ExecutionStatus::Cancelled
                    } else {
                        ExecutionStatus::ResultUnknown
                    },
                    &mut sequence,
                )
                .await?;
            }
            self.forget_session(&session, binding.run_id);
            if !confirmed || terminal == Some(RunOutcome::ResultUnknown) {
                return Err(
                    PortError::Failed(format!("result_unknown:{reason}_stop_unconfirmed")).into(),
                );
            }
        }
        Ok(())
    }

    /// Rebuild a scoped data-governance projection from committed facts and a server-owned
    /// policy. The policy is supplied by the daemon adapter; this method never reads arbitrary
    /// files or mutates the policy itself.
    pub async fn data_governance_snapshot(
        &self,
        policy: &DataPolicy,
        project_ref: &str,
        now_ms: u64,
    ) -> Result<DataGovernanceSnapshot, CoreError> {
        let events = self.read_all_events().await?.ok_or_else(|| {
            PortError::Unavailable("data_governance_read_all_unsupported".to_owned())
        })?;
        project_data_governance_snapshot(policy, project_ref, &events, now_ms)
            .map_err(|error| PortError::Failed(error).into())
    }
}
