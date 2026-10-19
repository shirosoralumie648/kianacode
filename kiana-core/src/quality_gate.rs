//! ControlPlane admission for quality.promote / quality.rollback.
//!
//! The command is an authority-sensitive admission fact, never an effect. It validates the
//! candidate and gate decision digests, rechecks the current authority epoch, the server-derived
//! scope digest and that this scope actually covers the named target, then requires a separate
//! approved quality challenge whose request hash binds the exact operation. The approval is
//! consumed in the same atomic transition batch as the admission fact, so the same mutation can
//! be applied at most once; a later replay of the same command identity is answered from the
//! committed receipt instead of consuming a second time. It does not change a provider route or
//! grant by itself; that remains a later, separately authorized transition.

use super::*;
use crate::dispatch::commit_confirmed;
use kiana_domain::{
    allow_list_covers, derived_request_id, json_digest, normalize_role_path, AggregateVersion,
    ApprovalDecision, ApprovalId, ApprovalState, RequestContext, RuntimeEvent, TransitionBatch,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

pub const QUALITY_MUTATION_SCHEMA: &str = "kiana.quality-mutation.v1";
const QUALITY_MUTATION_OUTPUT_SCHEMA: &str = "kiana.quality-mutation-admission.v1";

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QualityMutationArguments {
    schema: String,
    candidate_digest: String,
    gate_decision_digest: String,
    approval_ref: String,
    approval_request_hash: String,
    scope_digest: String,
    /// Explicit mutation target, relative to the project root. The scope must cover it.
    target_path: String,
    expected_authority_epoch: u64,
}

impl QualityMutationArguments {
    fn parse(value: Value) -> Result<Self, &'static str> {
        let parsed: Self =
            serde_json::from_value(value).map_err(|_| "quality_mutation_arguments_invalid")?;
        if parsed.schema != QUALITY_MUTATION_SCHEMA
            || !valid_digest(&parsed.candidate_digest)
            || !valid_digest(&parsed.gate_decision_digest)
            || !valid_digest(&parsed.approval_request_hash)
            || !valid_digest(&parsed.scope_digest)
            || parsed.expected_authority_epoch == 0
            || normalize_role_path(&parsed.target_path).is_none()
            || !parsed
                .approval_ref
                .strip_prefix("approval:")
                .is_some_and(|value| ApprovalId::parse_str(value).is_some())
        {
            return Err("quality_mutation_arguments_invalid");
        }
        Ok(parsed)
    }

    fn approval_id(&self) -> ApprovalId {
        ApprovalId::parse_str(self.approval_ref.trim_start_matches("approval:"))
            .expect("validated approval ref")
    }

    fn normalized_target(&self) -> String {
        normalize_role_path(&self.target_path).expect("validated target path")
    }
}

impl ControlPlane {
    pub(crate) async fn handle_quality_mutation(
        &self,
        context: RequestContext,
        command: String,
        arguments: Value,
    ) -> Result<CoreResponse, CoreError> {
        let parsed = match QualityMutationArguments::parse(arguments) {
            Ok(parsed) => parsed,
            Err(reason) => return self.reject_quality(&context, &command, reason).await,
        };
        if !context.project_trusted {
            return self
                .reject_quality(&context, &command, "project_untrusted")
                .await;
        }
        if context.actor_id.as_deref().is_none_or(str::is_empty) {
            return self
                .reject_quality(&context, &command, "quality_actor_required")
                .await;
        }
        if !matches!(
            context.role_id.as_str(),
            kiana_domain::ROLE_REVIEWER | kiana_domain::ROLE_QA
        ) {
            return self
                .reject_quality(&context, &command, "quality_reviewer_role_required")
                .await;
        }

        let current_epoch = self
            .authority_epoch(&context.project_root)
            .await?
            .unwrap_or(0);
        if current_epoch == 0 {
            return self
                .reject_quality(&context, &command, "quality_authority_epoch_missing")
                .await;
        }
        if parsed.expected_authority_epoch != current_epoch {
            return self
                .reject_quality(&context, &command, "quality_authority_epoch_stale")
                .await;
        }

        let target = parsed.normalized_target();
        let scope_digest = quality_scope_digest(&context, &target);
        if parsed.scope_digest != scope_digest {
            return self
                .reject_quality(&context, &command, "quality_scope_digest_mismatch")
                .await;
        }
        // The scope digest binds *which* target was approved; this second check proves the
        // caller's path scope actually reaches that target. A digest that matches a target the
        // actor may not touch is still not authority to touch it.
        let effective_scope = if context.path_allow.is_empty()
            || context
                .path_allow
                .iter()
                .any(|path| matches!(path.trim(), "." | "*"))
        {
            vec![".".to_owned()]
        } else {
            context.path_allow.clone()
        };
        if !allow_list_covers(&effective_scope, &target) {
            return self
                .reject_quality(&context, &command, "quality_target_outside_scope")
                .await;
        }
        let expected_request_hash = json_digest(&json!({
            "command": command.as_str(),
            "candidate_digest": parsed.candidate_digest,
            "gate_decision_digest": parsed.gate_decision_digest,
            "scope_digest": parsed.scope_digest,
            "authority_epoch": parsed.expected_authority_epoch,
        }));
        if parsed.approval_request_hash != expected_request_hash {
            return self
                .reject_quality(&context, &command, "quality_approval_request_hash_mismatch")
                .await;
        }

        // One stable command identity per exact mutation. The journal uses it to answer a
        // replay from the committed receipt, so an admitted mutation is never applied twice.
        let command_id = derived_request_id("quality.mutation", &expected_request_hash);
        let command_digest = json_digest(&json!({
            "command": command.as_str(),
            "candidate_digest": parsed.candidate_digest,
            "gate_decision_digest": parsed.gate_decision_digest,
            "approval_ref": parsed.approval_ref,
            "approval_request_hash": parsed.approval_request_hash,
            "scope_digest": parsed.scope_digest,
            "target_path": target,
            "authority_epoch": parsed.expected_authority_epoch,
        }));
        if let Some(receipt) = match self.events.read_command(&command_id).await {
            Ok(prior) => prior,
            Err(_) => {
                return self
                    .reject_quality(&context, &command, "quality_mutation_replay_unverifiable")
                    .await
            }
        } {
            if receipt.command_digest
                != command_digest
                    .strip_prefix("sha256:")
                    .unwrap_or(&command_digest)
            {
                return self
                    .reject_quality(&context, &command, "quality_mutation_replay_mismatch")
                    .await;
            }
            return Ok(quality_admission_response(
                &context,
                &command,
                &parsed,
                current_epoch,
                true,
            ));
        }

        let approval = match self
            .approvals
            .read_decision(&context, parsed.approval_id())
            .await
        {
            Ok(record) => record,
            Err(_) => {
                return self
                    .reject_quality(&context, &command, "quality_approval_unavailable")
                    .await
            }
        };
        if command == "quality.rollback" && approval.challenge.request_hash != expected_request_hash
        {
            return self
                .reject_quality(&context, &command, "quality_approval_request_hash_mismatch")
                .await;
        }
        if approval.challenge.request_hash != expected_request_hash {
            return self
                .reject_quality(&context, &command, "quality_secondary_approval_invalid")
                .await;
        }
        if approval.state != ApprovalState::Approved
            || approval.decision != Some(ApprovalDecision::Approve)
            || approval.decided_by.as_deref().is_none_or(str::is_empty)
            || approval.decided_by.as_deref() == context.actor_id.as_deref()
            || approval.challenge.expires_at_unix_ms <= now_unix_ms()
        {
            return self
                .reject_quality(&context, &command, "quality_secondary_approval_invalid")
                .await;
        }

        // Approved is not spent. The store prepares the Approved -> Consumed transition together
        // with its authority read set; nothing is written until the admission fact is committed
        // in the same batch below.
        let prepared = match self
            .approvals
            .prepare_consumption(&context, parsed.approval_id(), command_id)
            .await
        {
            Ok(prepared) => prepared,
            Err(PortError::Unavailable(_)) => {
                return self
                    .reject_quality(
                        &context,
                        &command,
                        "quality_approval_consumption_unavailable",
                    )
                    .await
            }
            Err(_) => {
                return self
                    .reject_quality(&context, &command, "quality_secondary_approval_invalid")
                    .await
            }
        };
        if prepared.pending.challenge.approval_id != parsed.approval_id()
            || prepared.pending.challenge.request_hash != expected_request_hash
        {
            return self
                .reject_quality(&context, &command, "quality_secondary_approval_invalid")
                .await;
        }

        let authority_key = json_digest(&json!({
            "project_root": Self::canonical_project_root(&context.project_root)
        }));
        let mut expected_versions: Vec<AggregateVersion> = Vec::new();
        for version in prepared
            .authority_versions
            .into_iter()
            .chain(std::iter::once(prepared.expected_version))
        {
            if let Some(existing) = expected_versions.iter().find(|existing| {
                existing.aggregate_type == version.aggregate_type
                    && existing.aggregate_id == version.aggregate_id
            }) {
                if existing.version != version.version {
                    return self
                        .reject_quality(&context, &command, "quality_secondary_approval_invalid")
                        .await;
                }
                continue;
            }
            expected_versions.push(version);
        }
        if let Some(existing) = expected_versions.iter().find(|version| {
            version.aggregate_type == "authority" && version.aggregate_id == authority_key
        }) {
            if existing.version != current_epoch {
                return self
                    .reject_quality(&context, &command, "quality_secondary_approval_invalid")
                    .await;
            }
        } else {
            expected_versions.push(AggregateVersion::new(
                "authority",
                authority_key,
                current_epoch,
            ));
        }
        expected_versions.push(AggregateVersion::new("quality", command_id.to_string(), 0));

        let data = quality_admission_data(&context, &command, &parsed, &target, current_epoch);
        let quality_event = match RuntimeEvent::new(context.request_id, 1, command.clone(), data) {
            Ok(event) => event.with_stream_metadata("quality", command_id.to_string(), 1),
            Err(error) => return Err(PortError::Failed(error.to_string()).into()),
        };
        let batch = TransitionBatch {
            command_id,
            command_digest,
            expected_versions,
            events: vec![prepared.event, quality_event],
        };
        // Committed means the approval is consumed and the admission fact exists exactly once.
        // Replayed means this exact command identity was already committed; the state is already
        // correct, so the response is the same and nothing is consumed again.
        let replayed = match commit_confirmed(self.events.as_ref(), batch).await {
            Ok(replayed) => replayed,
            Err(PortError::Conflict(_)) => {
                return self
                    .reject_quality(&context, &command, "quality_mutation_conflict")
                    .await
            }
            Err(error) => return Err(error.into()),
        };
        Ok(quality_admission_response(
            &context,
            &command,
            &parsed,
            current_epoch,
            replayed,
        ))
    }

    async fn reject_quality(
        &self,
        context: &RequestContext,
        command: &str,
        reason: &str,
    ) -> Result<CoreResponse, CoreError> {
        let accepted = self
            .append_event(
                context.request_id,
                1,
                "request.accepted",
                json!({"command":command}),
            )
            .await;
        if let Err(error) = accepted {
            match error {
                CoreError::Port(PortError::Conflict(reason))
                    if reason == "event_idempotency_key_payload_mismatch"
                        || reason == "event_idempotency_key_duplicate" => {}
                error => return Err(error),
            }
        }
        let rejected = self
            .append_event(
                context.request_id,
                2,
                "command.rejected",
                json!({"command":command,"reason":reason}),
            )
            .await;
        if let Err(error) = rejected {
            match error {
                CoreError::Port(PortError::Conflict(reason))
                    if reason == "event_idempotency_key_payload_mismatch"
                        || reason == "event_idempotency_key_duplicate" => {}
                error => return Err(error),
            }
        }
        Ok(CoreResponse::blocked(context.request_id, reason))
    }
}

fn quality_admission_data(
    context: &RequestContext,
    command: &str,
    parsed: &QualityMutationArguments,
    target: &str,
    authority_epoch: u64,
) -> Value {
    json!({
        "command": command,
        "operation": command.strip_prefix("quality.").unwrap_or(command),
        "request_id": context.request_id,
        "candidate_digest": parsed.candidate_digest,
        "gate_decision_digest": parsed.gate_decision_digest,
        "approval_ref": parsed.approval_ref,
        "approval_request_hash": parsed.approval_request_hash,
        "scope_digest": parsed.scope_digest,
        "target_path": target,
        "authority_epoch": authority_epoch,
        "status": "admitted",
    })
}

fn quality_admission_response(
    context: &RequestContext,
    command: &str,
    parsed: &QualityMutationArguments,
    authority_epoch: u64,
    replayed: bool,
) -> CoreResponse {
    CoreResponse::completed(
        context.request_id,
        json!({
            "schema": QUALITY_MUTATION_OUTPUT_SCHEMA,
            "status": "admitted",
            "operation": command.strip_prefix("quality.").unwrap_or(command),
            "candidate_digest": parsed.candidate_digest,
            "gate_decision_digest": parsed.gate_decision_digest,
            "approval_ref": parsed.approval_ref,
            "target_path": parsed.normalized_target(),
            "authority_epoch": authority_epoch,
            "replayed": replayed,
        }),
    )
}

fn quality_scope_digest(context: &RequestContext, target: &str) -> String {
    json_digest(&json!({
        "project_root": ControlPlane::canonical_project_root(&context.project_root),
        "session_id": context.session_id,
        "actor_id": context.actor_id,
        "role_id": context.role_id,
        "department_id": context.department_id,
        "target_path": target,
    }))
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}
