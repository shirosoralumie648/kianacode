//! Approval state is folded exclusively from the shared authority journal.
//! Redacted previews are never executable payloads. Undurable sensitive material is live-only.
use async_trait::async_trait;
use kiana_domain::{
    canonical_journal_bytes, journal_sha256, json_digest, redact_text, redact_value,
    AggregateVersion, ApprovalChallenge, ApprovalConsumptionFact, ApprovalDecision,
    ApprovalDecisionFact, ApprovalDecisionRecord, ApprovalExecutionMaterial, ApprovalId,
    ApprovalMaterialState, ApprovalState, CapabilityKind, CapabilityRequest, CommitOutcome,
    PendingApproval, PermissionProfile, PreparedApprovalConsumption, RequestContext, RequestId,
    RoleSpec, RuntimeEvent, SessionId, TransitionBatch, APPROVAL_CHALLENGE_SCHEMA,
};
use kiana_ports::{ApprovalStorePort, EventStorePort, PortError};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

const APPROVAL_STREAM: &str = "approval";
const APPROVAL_SCHEMA: &str = "kiana.approval-journal.v1";
const POLICY_VERSION: &str = "kiana.policy.v1";
const APPROVAL_TTL: Duration = Duration::from_secs(300);
const MAX_PAYLOAD_BYTES: usize = 256 * 1024;
const MAX_VOLATILE_PAYLOADS: usize = 128;

pub(crate) struct JournalApprovalStore {
    events: Arc<dyn EventStorePort>,
    // This cache has no authority. A matching, unexpired journal subject is always required.
    volatile: Mutex<HashMap<String, VolatilePayload>>,
}
struct VolatilePayload {
    request: CapabilityRequest,
    expires_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    session_id: SessionId,
    actor_id: String,
    project_root: String,
    project_trusted: bool,
    permission_profile: PermissionProfile,
    role_id: String,
    department_id: String,
    role_prompt_hash: String,
    work_packet_id: Option<String>,
    cell_id: Option<kiana_domain::CellId>,
    path_allow: Vec<String>,
    authority_versions: Vec<AggregateVersion>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Subject {
    schema: String,
    preview: PendingApproval,
    binding: Binding,
    payload_digest: String,
    recoverable: bool,
    #[serde(default)]
    material: ApprovalExecutionMaterial,
    issued_at_ms: u64,
    scope: String,
}
#[derive(Clone)]
struct Record {
    subject: Subject,
    state: ApprovalState,
    version: u64,
    at_unix_ms: u64,
    decision: Option<ApprovalDecision>,
    decision_command_id: Option<RequestId>,
    decided_by: Option<String>,
    dispatch_command_id: Option<RequestId>,
    decision_digest: Option<String>,
    consumption_digest: Option<String>,
}

impl JournalApprovalStore {
    pub(crate) fn new(events: Arc<dyn EventStorePort>) -> Result<Self, PortError> {
        if !events.supports_atomic_transitions() {
            return Err(PortError::Unavailable(
                "approval_atomic_journal_required".to_owned(),
            ));
        }
        Ok(Self {
            events,
            volatile: Mutex::new(HashMap::new()),
        })
    }

    async fn load(&self, id: ApprovalId) -> Result<Record, PortError> {
        let events = self
            .events
            .read_stream(APPROVAL_STREAM, &id.to_string())
            .await?;
        if events.is_empty()
            && self.events.read_all().await?.iter().any(|event| {
                event.kind == "approval.requested" && event.data["approval_id"] == json!(id)
            })
        {
            return Err(failed("approval_legacy_reauthorization_required"));
        }
        fold(id, &events)
    }

    async fn authority_version(&self, project_root: &str) -> Result<AggregateVersion, PortError> {
        let id = json_digest(&json!({"project_root":project_root}));
        let events = self.events.read_stream("authority", &id).await?;
        let event = events
            .iter()
            .max_by_key(|e| e.stream_version.unwrap_or(e.sequence))
            .ok_or_else(|| failed("approval_authority_missing"))?;
        if event.kind != "authority.revised"
            || event.data["project_root"] != project_root
            || event.data["project_trusted"] != true
            || event.data["revision_digest"]
                .as_str()
                .is_none_or(str::is_empty)
        {
            return Err(failed("approval_authority_invalid"));
        }
        let version = event
            .stream_version
            .filter(|v| *v > 0)
            .ok_or_else(|| failed("approval_authority_invalid"))?;
        Ok(AggregateVersion::new("authority", id, version))
    }

    async fn check_authority(&self, record: &Record) -> Result<(), PortError> {
        let current = self
            .authority_version(&record.subject.binding.project_root)
            .await?;
        if record.subject.binding.authority_versions != vec![current]
            || record.subject.preview.challenge.policy_version != POLICY_VERSION
            || RoleSpec::lookup(&record.subject.binding.role_id).is_none_or(|role| {
                role.prompt_hash != record.subject.binding.role_prompt_hash
                    || role.department_id != record.subject.binding.department_id
            })
        {
            return Err(failed("approval_authority_changed"));
        }
        Ok(())
    }

    async fn material(&self, record: &Record) -> Result<PendingApproval, PortError> {
        let request = if record.subject.recoverable {
            record.subject.preview.request.clone()
        } else {
            let now = unix_ms()?;
            let mut payloads = self.volatile.lock().await;
            payloads.retain(|_, payload| payload.expires_at_ms > now);
            payloads
                .get(&record.subject.preview.challenge.request_hash)
                .map(|payload| payload.request.clone())
                .ok_or_else(|| failed("approval_payload_unrecoverable"))?
        };
        let raw = serde_json::to_value(&request).map_err(|_| failed("approval_payload_invalid"))?;
        let preview = serde_json::to_value(&record.subject.preview.request)
            .map_err(|_| failed("approval_preview_invalid"))?;
        if !record
            .subject
            .material
            .matches_payloads(&raw, &preview)
            .map_err(|error| failed(&error))?
            || digest(&request)? != record.subject.payload_digest
            || subject_hash(
                &request,
                &record.subject.binding,
                &record.subject.preview.challenge,
                record.subject.issued_at_ms,
            )? != record.subject.preview.challenge.request_hash
        {
            return Err(failed("approval_request_integrity_mismatch"));
        }
        Ok(PendingApproval {
            challenge: record.subject.preview.challenge.clone(),
            request,
        })
    }

    async fn require_unexpired(&self, record: &Record) -> Result<(), PortError> {
        let now = unix_ms()?;
        if record.state.is_terminal() {
            return terminal_error(record.state);
        }
        let reason = if now < record.at_unix_ms {
            Some("approval_clock_rollback")
        } else if now >= record.subject.preview.challenge.expires_at_unix_ms {
            Some("approval_expired")
        } else {
            None
        };
        if let Some(reason) = reason {
            let command_id = RequestId::new();
            let event = transition_event(
                record,
                ApprovalState::Expired,
                command_id,
                json!({"reason":reason}),
                now.max(record.at_unix_ms),
            )?;
            self.commit_one(
                record,
                event,
                command_id,
                &json!({"action":"expire","reason":reason}),
                false,
            )
            .await?;
            return Err(failed(reason));
        }
        Ok(())
    }

    async fn commit_one(
        &self,
        record: &Record,
        event: RuntimeEvent,
        command_id: RequestId,
        intent: &Value,
        check_authority: bool,
    ) -> Result<(), PortError> {
        let mut expected_versions = vec![AggregateVersion::new(
            APPROVAL_STREAM,
            record.subject.preview.challenge.approval_id.to_string(),
            record.version,
        )];
        if check_authority {
            expected_versions.extend(record.subject.binding.authority_versions.clone());
        }
        let command_digest = digest(
            &json!({"approval_id":record.subject.preview.challenge.approval_id,
            "request_hash":record.subject.preview.challenge.request_hash,"intent":intent}),
        )?;
        committed(
            self.events
                .commit_transition(TransitionBatch {
                    command_id,
                    command_digest,
                    expected_versions,
                    events: vec![event],
                })
                .await?,
        )
    }

    fn require_binding(
        record: &Record,
        context: &RequestContext,
        full: bool,
    ) -> Result<(), PortError> {
        let binding = &record.subject.binding;
        if !binding.matches_principal(context)? {
            return Err(failed("approval_context_mismatch"));
        }
        if full
            && (binding.project_trusted != context.project_trusted
                || !context.project_trusted
                || binding.permission_profile != context.permission_profile
                || binding.path_allow != context.path_allow
                || binding.cell_id != context.cell_id
                || binding.work_packet_id != context.work_packet_id)
        {
            return Err(failed("approval_context_mismatch"));
        }
        Ok(())
    }
}

#[async_trait]
impl ApprovalStorePort for JournalApprovalStore {
    async fn stage(
        &self,
        context: &RequestContext,
        request: CapabilityRequest,
        reason: &str,
    ) -> Result<ApprovalChallenge, PortError> {
        if request.request_id != context.request_id || request.cell_id != context.cell_id {
            return Err(failed("approval_request_context_mismatch"));
        }
        if !context.project_trusted {
            return Err(failed("project_untrusted"));
        }
        let actor = context
            .actor_id
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| failed("approval_actor_required"))?;
        let role = RoleSpec::lookup(&context.role_id).ok_or_else(|| failed("role_unknown"))?;
        if role.department_id != context.department_id {
            return Err(failed("role_department_mismatch"));
        }
        if canonical_journal_bytes(&request)
            .map_err(PortError::Failed)?
            .len()
            > MAX_PAYLOAD_BYTES
        {
            return Err(failed("approval_payload_too_large"));
        }
        let project_root = canonical_root(&context.project_root)?;
        let authority_version = self.authority_version(&project_root).await?;
        let binding = Binding {
            session_id: context.session_id.clone(),
            actor_id: actor.to_owned(),
            project_root,
            project_trusted: context.project_trusted,
            permission_profile: context.permission_profile,
            role_id: context.role_id.clone(),
            department_id: context.department_id.clone(),
            role_prompt_hash: role.prompt_hash,
            work_packet_id: context.work_packet_id.clone(),
            cell_id: context.cell_id,
            path_allow: context.path_allow.clone(),
            authority_versions: vec![authority_version],
        };
        let encoded_binding =
            serde_json::to_value(&binding).map_err(|_| failed("approval_binding_invalid"))?;
        if redact_value(&encoded_binding) != encoded_binding {
            return Err(failed("approval_binding_not_recoverable"));
        }
        // Stable subject identity permits exact request retry; nonce is separately random.
        let id = ApprovalId::from_uuid(request.request_id.as_uuid());
        let prior = self
            .events
            .read_stream(APPROVAL_STREAM, &id.to_string())
            .await?;
        if !prior.is_empty() {
            let record = fold(id, &prior)?;
            if record.subject.binding != binding
                || record.subject.payload_digest != digest(&request)?
                || record.subject.preview.challenge.reason != redact_text(reason)
            {
                return Err(failed("approval_subject_conflict"));
            }
            self.require_unexpired(&record).await?;
            if !matches!(record.state, ApprovalState::Staged | ApprovalState::Active) {
                return Err(failed("approval_already_decided"));
            }
            // Lost raw material is not silently reconstructed from a displayed preview.
            self.material(&record).await?;
            return Ok(record.subject.preview.challenge);
        }
        let now = unix_ms()?;
        let expires = now
            .checked_add(APPROVAL_TTL.as_millis() as u64)
            .ok_or_else(|| failed("approval_expiry_overflow"))?;
        let mut random = [0u8; 32];
        SystemRandom::new()
            .fill(&mut random)
            .map_err(|_| failed("approval_nonce_unavailable"))?;
        let nonce = random
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let mut challenge = ApprovalChallenge {
            schema: APPROVAL_CHALLENGE_SCHEMA.to_owned(),
            approval_id: id,
            request_id: request.request_id,
            request_hash: String::new(),
            risk: request.risk,
            expires_at_unix_ms: expires,
            reason: redact_text(reason),
            nonce,
            policy_version: POLICY_VERSION.to_owned(),
        };
        challenge.request_hash = subject_hash(&request, &binding, &challenge, now)?;
        let raw = serde_json::to_value(&request).map_err(|_| failed("approval_payload_invalid"))?;
        let mut safe = redact_value(&raw);
        if request.capability == CapabilityKind::Secret {
            safe["arguments"] = json!({"preview":"[REDACTED]"});
        }
        let recoverable = safe == raw;
        let preview_value = safe.clone();
        let preview: CapabilityRequest =
            serde_json::from_value(safe).map_err(|_| failed("approval_preview_invalid"))?;
        let material = ApprovalExecutionMaterial::from_payloads(
            &raw,
            &preview_value,
            if recoverable {
                ApprovalMaterialState::InlineRedacted
            } else {
                ApprovalMaterialState::VolatileProtected
            },
            expires,
        )
        .map_err(|error| failed(&error))?;
        let subject = Subject {
            schema: APPROVAL_SCHEMA.to_owned(),
            preview: PendingApproval {
                challenge: challenge.clone(),
                request: preview,
            },
            binding,
            payload_digest: digest(&request)?,
            recoverable,
            material,
            issued_at_ms: now,
            scope: "once".to_owned(),
        };
        if !recoverable {
            let mut payloads = self.volatile.lock().await;
            payloads.retain(|_, payload| payload.expires_at_ms > now);
            if payloads.len() >= MAX_VOLATILE_PAYLOADS {
                return Err(failed("approval_live_payload_limit"));
            }
            payloads.insert(
                challenge.request_hash.clone(),
                VolatilePayload {
                    request,
                    expires_at_ms: expires,
                },
            );
        }
        let data = json!({"schema":APPROVAL_SCHEMA,"approval_id":id,"subject":subject,"state":"staged","at_unix_ms":now});
        if redact_value(&data) != data {
            self.volatile.lock().await.remove(&challenge.request_hash);
            return Err(failed("approval_preview_redaction_unstable"));
        }
        let command_id = RequestId::new();
        let event = RuntimeEvent::new(command_id, 1, "approval.staged", data)
            .map_err(|e| failed(&e.to_string()))?
            .with_stream_metadata(APPROVAL_STREAM, id.to_string(), 1);
        let mut expected_versions = subject.binding.authority_versions.clone();
        expected_versions.push(AggregateVersion::new(APPROVAL_STREAM, id.to_string(), 0));
        let result = committed(
            self.events
                .commit_transition(TransitionBatch {
                    command_id,
                    command_digest: digest(&json!({"action":"stage","subject":subject}))?,
                    expected_versions,
                    events: vec![event],
                })
                .await?,
        );
        if result.is_err() {
            self.volatile.lock().await.remove(&challenge.request_hash);
        }
        result?;
        Ok(challenge)
    }

    async fn prepare_activation(
        &self,
        id: ApprovalId,
        command_id: RequestId,
    ) -> Result<PreparedApprovalConsumption, PortError> {
        let record = self.load(id).await?;
        self.require_unexpired(&record).await?;
        if record.state != ApprovalState::Staged {
            return Err(failed("approval_not_staged"));
        }
        self.check_authority(&record).await?;
        let pending = self.material(&record).await?;
        let event = transition_event(
            &record,
            ApprovalState::Active,
            command_id,
            json!({"activation_command_id":command_id}),
            unix_ms()?,
        )?;
        Ok(PreparedApprovalConsumption {
            expected_version: AggregateVersion::new(
                APPROVAL_STREAM,
                id.to_string(),
                record.version,
            ),
            authority_versions: record.subject.binding.authority_versions,
            event,
            pending,
        })
    }

    async fn activate(&self, id: ApprovalId) -> Result<(), PortError> {
        let record = self.load(id).await?;
        self.require_unexpired(&record).await?;
        if record.state == ApprovalState::Active {
            return Ok(());
        }
        let command_id = RequestId::new();
        let prepared = self.prepare_activation(id, command_id).await?;
        let mut expected_versions = prepared.authority_versions;
        expected_versions.push(prepared.expected_version);
        committed(
            self.events
                .commit_transition(TransitionBatch {
                    command_id,
                    command_digest: digest(&json!({"action":"activate","approval_id":id}))?,
                    expected_versions,
                    events: vec![prepared.event],
                })
                .await?,
        )
    }

    async fn context_for_pending(
        &self,
        context: &RequestContext,
        id: ApprovalId,
    ) -> Result<RequestContext, PortError> {
        let record = self.load(id).await?;
        Self::require_binding(&record, context, false)?;
        let binding = record.subject.binding;
        let mut result = context.clone();
        result.path_allow = binding.path_allow;
        result.permission_profile = binding.permission_profile;
        result.cell_id = binding.cell_id;
        result.work_packet_id = binding.work_packet_id;
        // Current trust is never upgraded from an old pending snapshot.
        Ok(result)
    }

    async fn list_pending(
        &self,
        context: &RequestContext,
    ) -> Result<Vec<PendingApproval>, PortError> {
        let events = self.events.read_all().await?;
        let mut ids = events
            .iter()
            .filter(|e| {
                e.aggregate_type.as_deref() == Some(APPROVAL_STREAM) && e.kind == "approval.staged"
            })
            .filter_map(|e| {
                e.aggregate_id
                    .as_deref()
                    .and_then(|id| serde_json::from_value::<ApprovalId>(json!(id)).ok())
            })
            .collect::<Vec<_>>();
        ids.sort_by_key(ToString::to_string);
        ids.dedup();
        let mut pending = Vec::new();
        for id in ids {
            let record = self.load(id).await?;
            if record.state != ApprovalState::Active
                || !record.subject.binding.matches_principal(context)?
            {
                continue;
            }
            if let Err(error) = self.require_unexpired(&record).await {
                if matches!(&error,PortError::Failed(reason) if reason=="approval_expired"||reason=="approval_clock_rollback")
                {
                    continue;
                }
                return Err(error);
            }
            // Display-only: even a live cache is never serialized into a list response.
            pending.push(record.subject.preview);
        }
        pending.sort_by_key(|p| {
            (
                p.challenge.expires_at_unix_ms,
                p.challenge.approval_id.to_string(),
            )
        });
        Ok(pending)
    }

    async fn pending_with_proof(
        &self,
        context: &RequestContext,
        id: ApprovalId,
        request_hash: Option<&str>,
        nonce: Option<&str>,
    ) -> Result<PendingApproval, PortError> {
        let record = self.load(id).await?;
        Self::require_binding(&record, context, true)?;
        proof(&record, request_hash, nonce)?;
        self.require_unexpired(&record).await?;
        if !matches!(
            record.state,
            ApprovalState::Active | ApprovalState::Approved
        ) {
            return Err(failed("approval_not_active"));
        }
        self.check_authority(&record).await?;
        self.material(&record).await
    }

    async fn validate_with_proof(
        &self,
        context: &RequestContext,
        id: ApprovalId,
        request_hash: Option<&str>,
        nonce: Option<&str>,
    ) -> Result<(), PortError> {
        self.pending_with_proof(context, id, request_hash, nonce)
            .await
            .map(|_| ())
    }

    async fn decide_with_proof(
        &self,
        context: &RequestContext,
        id: ApprovalId,
        decision: ApprovalDecision,
        request_hash: Option<&str>,
        nonce: Option<&str>,
    ) -> Result<PendingApproval, PortError> {
        let record = self.load(id).await?;
        Self::require_binding(&record, context, decision == ApprovalDecision::Approve)?;
        proof(&record, request_hash, nonce)?;
        if matches!(
            record.state,
            ApprovalState::Approved | ApprovalState::Denied | ApprovalState::Consumed
        ) {
            if record.decision != Some(decision) {
                return Err(PortError::Conflict("approval_decision_conflict".to_owned()));
            }
            // Repeated decisions expose only the recorded preview, never a fresh execution payload.
            return Ok(record.subject.preview);
        }
        self.require_unexpired(&record).await?;
        if record.state != ApprovalState::Active {
            return Err(failed("approval_not_active"));
        }
        let now = unix_ms()?;
        let (next, pending) = if decision == ApprovalDecision::Approve {
            self.check_authority(&record).await?;
            (ApprovalState::Approved, self.material(&record).await?)
        } else {
            (ApprovalState::Denied, record.subject.preview.clone())
        };
        let decision_fact = ApprovalDecisionFact::new(
            id,
            record.subject.preview.request.request_id,
            record.subject.preview.challenge.request_hash.clone(),
            decision,
            context.actor_id.clone().unwrap_or_default(),
            context.request_id,
            record.version,
            record.subject.binding.authority_versions[0].clone(),
            now,
            record.subject.preview.challenge.expires_at_unix_ms,
        )
        .map_err(|error| failed(&error))?;
        let event = transition_event(
            &record,
            next,
            context.request_id,
            json!({"decision":decision,
            "decision_command_id":context.request_id,"decided_by":context.actor_id}),
            now,
        )?;
        let event = with_fact(event, "decision_fact", &decision_fact)?;
        self.commit_one(
            &record,
            event,
            context.request_id,
            &json!({"action":"decide","decision":decision,
            "actor_id":context.actor_id,"session_id":context.session_id}),
            decision == ApprovalDecision::Approve,
        )
        .await?;
        if decision == ApprovalDecision::Deny {
            self.volatile
                .lock()
                .await
                .remove(&record.subject.preview.challenge.request_hash);
        }
        Ok(pending)
    }

    async fn decide_with_proof_and_version(
        &self,
        context: &RequestContext,
        id: ApprovalId,
        decision: ApprovalDecision,
        request_hash: Option<&str>,
        nonce: Option<&str>,
        expected_version: Option<u64>,
    ) -> Result<PendingApproval, PortError> {
        if let Some(expected_version) = expected_version {
            let record = self.load(id).await?;
            // A replay of the original command returns its durable decision even when the
            // caller retained the pre-decision version.  Only an undecided subject is rejected
            // for a stale expected version.
            if record.decision.is_none() && record.version != expected_version {
                return Err(PortError::Conflict(
                    "approval_expected_version_conflict".to_owned(),
                ));
            }
        }
        self.decide_with_proof(context, id, decision, request_hash, nonce)
            .await
    }

    async fn read_decision(
        &self,
        context: &RequestContext,
        id: ApprovalId,
    ) -> Result<ApprovalDecisionRecord, PortError> {
        let record = self.load(id).await?;
        Self::require_binding(&record, context, false)?;
        let payload_available = self.material(&record).await.is_ok();
        Ok(ApprovalDecisionRecord {
            approval_id: id,
            state: record.state,
            challenge: record.subject.preview.challenge,
            decision: record.decision,
            decision_command_id: record.decision_command_id,
            decided_by: record.decided_by,
            dispatch_command_id: record.dispatch_command_id,
            payload_available,
            version: record.version,
            decision_digest: record.decision_digest,
            consumption_digest: record.consumption_digest,
        })
    }

    async fn prepare_consumption(
        &self,
        context: &RequestContext,
        id: ApprovalId,
        dispatch_command_id: RequestId,
    ) -> Result<PreparedApprovalConsumption, PortError> {
        let record = self.load(id).await?;
        Self::require_binding(&record, context, true)?;
        self.require_unexpired(&record).await?;
        if record.state != ApprovalState::Approved {
            return Err(failed("approval_not_approved"));
        }
        self.check_authority(&record).await?;
        let pending = self.material(&record).await?;
        let now = unix_ms()?;
        let event = transition_event(
            &record,
            ApprovalState::Consumed,
            dispatch_command_id,
            json!({"dispatch_command_id":dispatch_command_id,
            "decision_command_id":record.decision_command_id,"decided_by":record.decided_by}),
            now,
        )?;
        let decision_command_id = record
            .decision_command_id
            .ok_or_else(|| failed("approval_decision_command_missing"))?;
        let authority_version = record.subject.binding.authority_versions[0].clone();
        let consumption_fact = ApprovalConsumptionFact::new(
            id,
            record.subject.preview.request.request_id,
            record.subject.preview.challenge.request_hash.clone(),
            decision_command_id,
            dispatch_command_id,
            record.version,
            authority_version,
            now,
            record.subject.preview.challenge.expires_at_unix_ms,
        )
        .map_err(|error| failed(&error))?;
        let event = with_fact(event, "consumption_fact", &consumption_fact)?;
        Ok(PreparedApprovalConsumption {
            expected_version: AggregateVersion::new(
                APPROVAL_STREAM,
                id.to_string(),
                record.version,
            ),
            authority_versions: record.subject.binding.authority_versions,
            event,
            pending,
        })
    }

    async fn consume(
        &self,
        _context: &RequestContext,
        _id: ApprovalId,
    ) -> Result<PendingApproval, PortError> {
        Err(PortError::Unavailable(
            "approval_atomic_consumption_required".to_owned(),
        ))
    }
    async fn consume_with_proof(
        &self,
        _context: &RequestContext,
        _id: ApprovalId,
        _request_hash: Option<&str>,
        _nonce: Option<&str>,
    ) -> Result<PendingApproval, PortError> {
        Err(PortError::Unavailable(
            "approval_atomic_consumption_required".to_owned(),
        ))
    }

    async fn invalidate(
        &self,
        context: &RequestContext,
        id: ApprovalId,
        reason: &str,
    ) -> Result<(), PortError> {
        let record = self.load(id).await?;
        Self::require_binding(&record, context, false)?;
        if record.state == ApprovalState::Cancelled {
            return Ok(());
        }
        if record.state.is_terminal() {
            return terminal_error(record.state);
        }
        let command_id = RequestId::new();
        let event = transition_event(
            &record,
            ApprovalState::Cancelled,
            command_id,
            json!({"reason":redact_text(reason),"revoked_by":context.actor_id}),
            unix_ms()?.max(record.at_unix_ms),
        )?;
        self.commit_one(
            &record,
            event,
            command_id,
            &json!({"action":"invalidate","reason":redact_text(reason)}),
            false,
        )
        .await?;
        self.volatile
            .lock()
            .await
            .remove(&record.subject.preview.challenge.request_hash);
        Ok(())
    }

    async fn invalidate_project(
        &self,
        project_root: &str,
        reason: &str,
    ) -> Result<Vec<ApprovalId>, PortError> {
        let project_root = canonical_root(project_root)?;
        let events = self.events.read_all().await?;
        let mut ids = events
            .iter()
            .filter(|e| {
                e.aggregate_type.as_deref() == Some(APPROVAL_STREAM) && e.kind == "approval.staged"
            })
            .filter_map(|e| {
                e.aggregate_id
                    .as_deref()
                    .and_then(|id| serde_json::from_value::<ApprovalId>(json!(id)).ok())
            })
            .collect::<Vec<_>>();
        ids.sort_by_key(ToString::to_string);
        ids.dedup();
        let mut invalidated = Vec::new();
        for id in ids {
            for _ in 0..4 {
                let record = self.load(id).await?;
                if record.subject.binding.project_root != project_root || record.state.is_terminal()
                {
                    break;
                }
                let command_id = RequestId::new();
                let event = transition_event(
                    &record,
                    ApprovalState::Cancelled,
                    command_id,
                    json!({"reason":redact_text(reason),"source":"project_invalidation"}),
                    unix_ms()?.max(record.at_unix_ms),
                )?;
                match self
                    .commit_one(
                        &record,
                        event,
                        command_id,
                        &json!({"action":"invalidate_project","reason":redact_text(reason)}),
                        false,
                    )
                    .await
                {
                    Ok(()) => {
                        invalidated.push(id);
                        self.volatile
                            .lock()
                            .await
                            .remove(&record.subject.preview.challenge.request_hash);
                        break;
                    }
                    Err(PortError::Conflict(_)) => continue,
                    Err(error) => return Err(error),
                }
            }
            let remaining = self.load(id).await?;
            if remaining.subject.binding.project_root == project_root
                && !remaining.state.is_terminal()
            {
                return Err(PortError::Conflict(
                    "approval_invalidation_contention".to_owned(),
                ));
            }
        }
        Ok(invalidated)
    }
}

impl Binding {
    fn matches_principal(&self, context: &RequestContext) -> Result<bool, PortError> {
        Ok(context.actor_id.as_deref() == Some(self.actor_id.as_str())
            && self.session_id == context.session_id
            && self.project_root == canonical_root(&context.project_root)?
            && self.role_id == context.role_id
            && self.department_id == context.department_id)
    }
}
fn canonical_root(root: &str) -> Result<String, PortError> {
    std::fs::canonicalize(Path::new(root))
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|_| failed("approval_project_root_unavailable"))
}
fn failed(reason: &str) -> PortError {
    PortError::Failed(reason.to_owned())
}
fn unix_ms() -> Result<u64, PortError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().min(u128::from(u64::MAX)) as u64)
        .map_err(|_| failed("approval_clock_invalid"))
}
fn digest(value: &impl Serialize) -> Result<String, PortError> {
    canonical_journal_bytes(value)
        .map(|bytes| journal_sha256(&bytes))
        .map_err(PortError::Failed)
}
fn subject_hash(
    request: &CapabilityRequest,
    binding: &Binding,
    challenge: &ApprovalChallenge,
    issued: u64,
) -> Result<String, PortError> {
    Ok(format!(
        "sha256:{}",
        digest(
            &json!({"schema":APPROVAL_SCHEMA,"request":request,"binding":binding,
        "action_digest":kiana_domain::capability_action_digest(request),
        "issued_at_ms":issued,"expires_at_unix_ms":challenge.expires_at_unix_ms,"nonce":challenge.nonce,
        "policy_version":challenge.policy_version,"scope":"once"})
        )?
    ))
}
fn proof(
    record: &Record,
    request_hash: Option<&str>,
    nonce: Option<&str>,
) -> Result<(), PortError> {
    let (Some(hash), Some(nonce)) = (request_hash, nonce) else {
        return Err(failed("approval_proof_required"));
    };
    if hash != record.subject.preview.challenge.request_hash {
        return Err(failed("approval_request_hash_mismatch"));
    }
    if nonce != record.subject.preview.challenge.nonce {
        return Err(failed("approval_nonce_mismatch"));
    }
    Ok(())
}
fn terminal_error(state: ApprovalState) -> Result<(), PortError> {
    Err(match state {
        ApprovalState::Consumed => PortError::Conflict("approval_already_consumed".to_owned()),
        ApprovalState::Expired => failed("approval_expired"),
        ApprovalState::Cancelled => failed("approval_cancelled"),
        ApprovalState::Denied => failed("approval_denied"),
        _ => failed("approval_not_active"),
    })
}
fn committed(outcome: CommitOutcome) -> Result<(), PortError> {
    match outcome {
        CommitOutcome::Committed { .. } | CommitOutcome::Replayed { .. } => Ok(()),
        CommitOutcome::Conflict { .. } => {
            Err(PortError::Conflict("approval_version_conflict".to_owned()))
        }
        CommitOutcome::Unknown { reason, .. } => {
            Err(failed(&format!("result_unknown:approval_commit:{reason}")))
        }
    }
}
fn transition_event(
    record: &Record,
    next: ApprovalState,
    command_id: RequestId,
    details: Value,
    now: u64,
) -> Result<RuntimeEvent, PortError> {
    record
        .state
        .transition(next)
        .map_err(|e| failed(&e.to_string()))?;
    let kind = match next {
        ApprovalState::Active => "approval.activated",
        ApprovalState::Approved => "approval.approved",
        ApprovalState::Denied => "approval.denied",
        ApprovalState::Expired => "approval.expired",
        ApprovalState::Cancelled => "approval.cancelled",
        ApprovalState::Consumed => "approval.consumed",
        _ => return Err(failed("approval_transition_invalid")),
    };
    let id = record.subject.preview.challenge.approval_id;
    let mut data = json!({"schema":APPROVAL_SCHEMA,"approval_id":id,"previous_state":record.state,"state":next,
        "request_hash":record.subject.preview.challenge.request_hash,"at_unix_ms":now});
    for (key, value) in details
        .as_object()
        .ok_or_else(|| failed("approval_transition_invalid"))?
    {
        data[key] = value.clone();
    }
    Ok(
        RuntimeEvent::new(command_id, record.version + 1, kind, redact_value(&data))
            .map_err(|e| failed(&e.to_string()))?
            .with_stream_metadata(APPROVAL_STREAM, id.to_string(), record.version + 1),
    )
}

fn with_fact<T: Serialize>(
    mut event: RuntimeEvent,
    field: &str,
    fact: &T,
) -> Result<RuntimeEvent, PortError> {
    let object = event
        .data
        .as_object_mut()
        .ok_or_else(|| failed("approval_transition_invalid"))?;
    object.insert(
        field.to_owned(),
        serde_json::to_value(fact).map_err(|_| failed("approval_transition_invalid"))?,
    );
    event.data = redact_value(&event.data);
    Ok(event)
}
fn fold(id: ApprovalId, events: &[RuntimeEvent]) -> Result<Record, PortError> {
    let first = events.first().ok_or_else(|| failed("approval_not_found"))?;
    if first.kind != "approval.staged"
        || first.stream_version != Some(1)
        || first.data["schema"] != APPROVAL_SCHEMA
        || first.data["approval_id"] != json!(id)
    {
        return Err(failed("approval_legacy_reauthorization_required"));
    }
    let subject: Subject = serde_json::from_value(first.data["subject"].clone())
        .map_err(|_| failed("approval_journal_corrupt"))?;
    if subject.schema != APPROVAL_SCHEMA
        || subject.scope != "once"
        || subject.preview.challenge.approval_id != id
        || subject.preview.challenge.schema != APPROVAL_CHALLENGE_SCHEMA
        || subject.preview.challenge.nonce.len() != 64
        || !subject
            .preview
            .challenge
            .nonce
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
        || subject.preview.challenge.expires_at_unix_ms <= subject.issued_at_ms
        || subject.binding.authority_versions.len() != 1
        || subject.material.validate().is_err()
        || subject.material.expires_at_unix_ms != subject.preview.challenge.expires_at_unix_ms
        || subject.material.payload_digest != format!("sha256:{}", subject.payload_digest)
        || subject.preview.challenge.request_id != subject.preview.request.request_id
        || subject.preview.challenge.risk != subject.preview.request.risk
        || subject.preview.request.cell_id != subject.binding.cell_id
        || first.data["state"] != "staged"
        || first.data["at_unix_ms"] != subject.issued_at_ms
        || redact_value(&first.data) != first.data
    {
        return Err(failed("approval_journal_corrupt"));
    }
    let authority = &subject.binding.authority_versions[0];
    if authority.aggregate_type != "authority"
        || authority.version == 0
        || authority.aggregate_id
            != json_digest(&json!({"project_root":subject.binding.project_root}))
    {
        return Err(failed("approval_journal_authority_invalid"));
    }
    let mut record = Record {
        at_unix_ms: subject.issued_at_ms,
        subject,
        state: ApprovalState::Staged,
        version: 1,
        decision: None,
        decision_command_id: None,
        decided_by: None,
        dispatch_command_id: None,
        decision_digest: None,
        consumption_digest: None,
    };
    for event in &events[1..] {
        if event.stream_version != Some(record.version + 1)
            || event.data["schema"] != APPROVAL_SCHEMA
            || event.data["approval_id"] != json!(id)
            || event.data["request_hash"] != record.subject.preview.challenge.request_hash
            || event.data["previous_state"] != json!(record.state)
        {
            return Err(failed("approval_journal_corrupt"));
        }
        let next = match event.kind.as_str() {
            "approval.activated" => ApprovalState::Active,
            "approval.approved" => ApprovalState::Approved,
            "approval.denied" => ApprovalState::Denied,
            "approval.expired" => ApprovalState::Expired,
            "approval.cancelled" => ApprovalState::Cancelled,
            "approval.consumed" => ApprovalState::Consumed,
            _ => return Err(failed("approval_journal_corrupt")),
        };
        if event.data["state"] != json!(next) {
            return Err(failed("approval_journal_corrupt"));
        }
        let at = event.data["at_unix_ms"]
            .as_u64()
            .ok_or_else(|| failed("approval_journal_corrupt"))?;
        if at < record.at_unix_ms {
            return Err(failed("approval_journal_clock_invalid"));
        }
        record.state = record
            .state
            .transition(next)
            .map_err(|_| failed("approval_journal_transition_invalid"))?;
        record.version += 1;
        record.at_unix_ms = at;
        if matches!(next, ApprovalState::Approved | ApprovalState::Denied) {
            let decision = if next == ApprovalState::Approved {
                ApprovalDecision::Approve
            } else {
                ApprovalDecision::Deny
            };
            let fact: ApprovalDecisionFact =
                serde_json::from_value(event.data["decision_fact"].clone())
                    .map_err(|_| failed("approval_journal_decision_invalid"))?;
            if fact.validate().is_err()
                || fact.approval_id != id
                || fact.subject_request_id != record.subject.preview.request.request_id
                || fact.request_hash != record.subject.preview.challenge.request_hash
                || fact.decision != decision
                || fact.command_id != event.request_id
                || fact.expected_version != record.version - 1
                || fact.authority_version != record.subject.binding.authority_versions[0]
                || fact.expires_at_unix_ms != record.subject.preview.challenge.expires_at_unix_ms
                || event.data["decision"] != json!(decision)
                || event.data["decided_by"] != json!(fact.actor_id)
                || event.data["decision_command_id"] != json!(event.request_id)
            {
                return Err(failed("approval_journal_decision_invalid"));
            }
            record.decision = Some(decision);
            record.decision_command_id = Some(event.request_id);
            record.decided_by = Some(fact.actor_id);
            record.decision_digest = Some(fact.decision_digest);
        }
        if next == ApprovalState::Consumed {
            let fact: ApprovalConsumptionFact =
                serde_json::from_value(event.data["consumption_fact"].clone())
                    .map_err(|_| failed("approval_consumption_fact_invalid"))?;
            let decision_command_id = record
                .decision_command_id
                .ok_or_else(|| failed("approval_decision_command_missing"))?;
            if fact.validate().is_err()
                || fact.approval_id != id
                || fact.subject_request_id != record.subject.preview.request.request_id
                || fact.request_hash != record.subject.preview.challenge.request_hash
                || fact.decision_command_id != decision_command_id
                || fact.dispatch_command_id != event.request_id
                || fact.expected_version != record.version - 1
                || fact.authority_version != record.subject.binding.authority_versions[0]
                || fact.expires_at_unix_ms != record.subject.preview.challenge.expires_at_unix_ms
                || fact.consumed_at_unix_ms != at
                || event.data["dispatch_command_id"] != json!(event.request_id)
            {
                return Err(failed("approval_dispatch_identity_mismatch"));
            }
            record.dispatch_command_id = Some(fact.dispatch_command_id);
            record.consumption_digest = Some(fact.consumption_digest);
        }
    }
    Ok(record)
}
