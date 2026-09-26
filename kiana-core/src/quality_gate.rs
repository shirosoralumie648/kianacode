//! ControlPlane admission for quality.promote / quality.rollback.
//!
//! The command is an authority-sensitive admission fact. It validates the candidate and gate
//! decision digests, rechecks the current authority epoch and scope, and requires a separate
//! approved quality challenge whose request hash binds the exact operation. It does not change a
//! provider route or grant by itself; that remains a later, separately authorized transition.

use super::*;
use kiana_domain::{json_digest, ApprovalDecision, ApprovalId, ApprovalState, RequestContext};
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

        let scope_digest = quality_scope_digest(&context);
        if parsed.scope_digest != scope_digest {
            return self
                .reject_quality(&context, &command, "quality_scope_digest_mismatch")
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
        if approval.state != ApprovalState::Approved
            || approval.decision != Some(ApprovalDecision::Approve)
            || approval.decided_by.as_deref().is_none_or(str::is_empty)
            || approval.decided_by.as_deref() == context.actor_id.as_deref()
            || approval.challenge.request_hash != expected_request_hash
            || approval.challenge.expires_at_unix_ms <= now_unix_ms()
        {
            return self
                .reject_quality(&context, &command, "quality_secondary_approval_invalid")
                .await;
        }

        let operation = command.strip_prefix("quality.").unwrap_or(&command);
        let data = json!({
            "command": command.as_str(),
            "operation": operation,
            "request_id": context.request_id,
            "candidate_digest": parsed.candidate_digest,
            "gate_decision_digest": parsed.gate_decision_digest,
            "approval_ref": parsed.approval_ref,
            "approval_request_hash": parsed.approval_request_hash,
            "scope_digest": parsed.scope_digest,
            "authority_epoch": current_epoch,
            "status": "admitted",
        });
        self.append_event(context.request_id, 1, &command, data.clone())
            .await?;
        Ok(CoreResponse::completed(
            context.request_id,
            json!({
                "schema": QUALITY_MUTATION_OUTPUT_SCHEMA,
                "status": "admitted",
                "operation": operation,
                "candidate_digest": parsed.candidate_digest,
                "gate_decision_digest": parsed.gate_decision_digest,
                "approval_ref": parsed.approval_ref,
                "authority_epoch": current_epoch,
            }),
        ))
    }

    async fn reject_quality(
        &self,
        context: &RequestContext,
        command: &str,
        reason: &str,
    ) -> Result<CoreResponse, CoreError> {
        self.append_event(
            context.request_id,
            1,
            "request.accepted",
            json!({"command":command}),
        )
        .await?;
        self.append_event(
            context.request_id,
            2,
            "command.rejected",
            json!({"command":command,"reason":reason}),
        )
        .await?;
        Ok(CoreResponse::blocked(context.request_id, reason))
    }
}

fn quality_scope_digest(context: &RequestContext) -> String {
    json_digest(&json!({
        "project_root": ControlPlane::canonical_project_root(&context.project_root),
        "session_id": context.session_id,
        "actor_id": context.actor_id,
        "role_id": context.role_id,
        "department_id": context.department_id,
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
