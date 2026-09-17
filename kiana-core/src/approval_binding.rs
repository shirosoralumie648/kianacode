//! Exact approval subject binding and Human Inbox value projection.
//!
//! Approval is a decision about one immutable request, not a general permission.  This module
//! carries only typed identities/digests and a bounded status projection; the existing approval
//! store remains responsible for its CAS/persistence adapter and the ControlPlane remains the only
//! caller that can proceed to Broker dispatch.

use kiana_domain::{
    canonical_journal_bytes, json_digest, ApprovalChallenge, ApprovalDecision, ApprovalId,
    AuthenticatedPrincipalRef, GrantId, ProjectId, RequestContext, RequestId, SchemaVersion,
    SecurityReasonCode, SessionId,
};
use kiana_policy::GrantScope;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const APPROVAL_BINDING_SCHEMA: &str = "kiana.approval-binding.v1";
pub const HUMAN_INBOX_ITEM_SCHEMA: &str = "kiana.human-inbox-item.v1";
pub const APPROVAL_BINDING_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_APPROVAL_COMMAND_KIND: usize = 256;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalBinding {
    pub schema: String,
    pub version: SchemaVersion,
    pub approval_id: ApprovalId,
    pub subject_request_id: RequestId,
    pub principal: AuthenticatedPrincipalRef,
    pub session_id: SessionId,
    pub project_id: ProjectId,
    pub command_kind: String,
    pub target_digest: String,
    pub payload_digest: String,
    pub scope_digest: String,
    pub grant_id: Option<GrantId>,
    pub authority_epoch: u64,
    pub policy_revision: String,
    pub expires_at_unix_ms: u64,
    pub binding_digest: String,
}

impl ApprovalBinding {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        approval_id: ApprovalId,
        subject_request_id: RequestId,
        principal: AuthenticatedPrincipalRef,
        session_id: SessionId,
        project_id: ProjectId,
        command_kind: impl Into<String>,
        target_digest: impl Into<String>,
        payload_digest: impl Into<String>,
        scope_digest: impl Into<String>,
        grant_id: Option<GrantId>,
        authority_epoch: u64,
        policy_revision: impl Into<String>,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut binding = Self {
            schema: APPROVAL_BINDING_SCHEMA.to_owned(),
            version: APPROVAL_BINDING_VERSION,
            approval_id,
            subject_request_id,
            principal,
            session_id,
            project_id,
            command_kind: command_kind.into(),
            target_digest: target_digest.into(),
            payload_digest: payload_digest.into(),
            scope_digest: scope_digest.into(),
            grant_id,
            authority_epoch,
            policy_revision: policy_revision.into(),
            expires_at_unix_ms,
            binding_digest: String::new(),
        };
        binding.binding_digest = binding.digest();
        binding.validate()?;
        Ok(binding)
    }

    /// Build an exact subject binding from a server-owned context and a prepared request. The
    /// request body is represented only by a digest; raw arguments never enter the binding.
    pub fn for_request(
        approval_id: ApprovalId,
        context: &RequestContext,
        project_id: ProjectId,
        request: &kiana_domain::CapabilityRequest,
        target_digest: impl Into<String>,
        grant: Option<&GrantScope>,
        authority_epoch: u64,
        policy_revision: impl Into<String>,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let scope_digest = json_digest(&json!({
            "session_id": context.session_id,
            "actor_id": context.actor_id,
            "project_root": context.project_root,
            "project_trusted": context.project_trusted,
            "permission_profile": context.permission_profile,
            "role_id": context.role_id,
            "department_id": context.department_id,
            "path_allow": context.path_allow,
            "cell_id": context.cell_id,
        }));
        let principal_id = context
            .actor_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| SecurityReasonCode::AuthPrincipalMissing.as_str().to_owned())?;
        let mut principal = AuthenticatedPrincipalRef::local();
        principal.principal_id = principal_id.to_owned();
        principal.principal_digest = principal.digest();
        Self::new(
            approval_id,
            request.request_id,
            principal,
            context.session_id.clone(),
            project_id,
            request.operation.clone(),
            target_digest,
            kiana_domain::json_digest(&serde_json::to_value(request).map_err(|_| {
                SecurityReasonCode::PolicyApprovalBindingMismatch
                    .as_str()
                    .to_owned()
            })?),
            scope_digest,
            grant.map(|grant| grant.grant_id),
            authority_epoch,
            policy_revision,
            expires_at_unix_ms,
        )
    }

    pub fn validate_request(
        &self,
        context: &RequestContext,
        project_id: ProjectId,
        request: &kiana_domain::CapabilityRequest,
        target_digest: &str,
        authority_epoch: u64,
        policy_revision: &str,
        now_unix_ms: u64,
    ) -> Result<(), String> {
        self.validate()?;
        if now_unix_ms >= self.expires_at_unix_ms {
            return Err(SecurityReasonCode::PolicyApprovalExpired
                .as_str()
                .to_owned());
        }
        if self.project_id != project_id
            || self.session_id != context.session_id
            || self.principal.principal_id != context.actor_id.as_deref().unwrap_or_default()
            || self.subject_request_id != request.request_id
            || self.command_kind != request.operation
            || self.target_digest != target_digest
            || self.payload_digest
                != kiana_domain::json_digest(&serde_json::to_value(request).map_err(|_| {
                    SecurityReasonCode::PolicyApprovalBindingMismatch
                        .as_str()
                        .to_owned()
                })?)
            || self.authority_epoch != authority_epoch
            || self.policy_revision != policy_revision
        {
            return Err(SecurityReasonCode::PolicyApprovalBindingMismatch
                .as_str()
                .to_owned());
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != APPROVAL_BINDING_SCHEMA
            || !self.version.is_compatible_with(&APPROVAL_BINDING_VERSION)
            || self.approval_id.as_uuid().is_nil()
            || self.subject_request_id.as_uuid().is_nil()
            || self.session_id.is_empty()
            || self.project_id.as_uuid().is_nil()
            || self.command_kind.trim().is_empty()
            || self.command_kind.len() > MAX_APPROVAL_COMMAND_KIND
            || self.command_kind.contains('\0')
            || self.authority_epoch == 0
            || self.expires_at_unix_ms == 0
        {
            return Err(SecurityReasonCode::PolicyApprovalBindingMismatch
                .as_str()
                .to_owned());
        }
        self.principal
            .validate()
            .map_err(|_| SecurityReasonCode::AuthPrincipalMissing.as_str().to_owned())?;
        for (value, field) in [
            (&self.target_digest, "approval_target_digest"),
            (&self.payload_digest, "approval_payload_digest"),
            (&self.scope_digest, "approval_scope_digest"),
            (&self.policy_revision, "approval_policy_revision"),
        ] {
            validate_digest(value, field)?;
        }
        if self.grant_id.is_some_and(|id| id.as_uuid().is_nil()) {
            return Err("approval_grant_id_invalid".to_owned());
        }
        validate_digest(&self.binding_digest, "approval_binding_digest")?;
        if self.binding_digest != self.digest() {
            return Err("approval_binding_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let binding: Self = serde_json::from_value(value.clone())
            .map_err(|_| "approval_binding_decode_failed".to_owned())?;
        binding.validate()?;
        Ok(binding)
    }

    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "approval_binding_encode_failed".to_owned())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "approval_id": self.approval_id,
            "subject_request_id": self.subject_request_id,
            "principal": self.principal,
            "session_id": self.session_id,
            "project_id": self.project_id,
            "command_kind": self.command_kind,
            "target_digest": self.target_digest,
            "payload_digest": self.payload_digest,
            "scope_digest": self.scope_digest,
            "grant_id": self.grant_id,
            "authority_epoch": self.authority_epoch,
            "policy_revision": self.policy_revision,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HumanInboxStatus {
    Pending,
    Approved,
    Denied,
    Expired,
    Consumed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanInboxItem {
    pub schema: String,
    pub version: SchemaVersion,
    pub item_id: ApprovalId,
    pub binding: ApprovalBinding,
    pub status: HumanInboxStatus,
    #[serde(default)]
    pub decided_by: Option<AuthenticatedPrincipalRef>,
    pub revision: u64,
    pub item_digest: String,
}

impl HumanInboxItem {
    pub fn new(binding: ApprovalBinding) -> Result<Self, String> {
        binding.validate()?;
        let mut item = Self {
            schema: HUMAN_INBOX_ITEM_SCHEMA.to_owned(),
            version: APPROVAL_BINDING_VERSION,
            item_id: binding.approval_id,
            binding,
            status: HumanInboxStatus::Pending,
            decided_by: None,
            revision: 1,
            item_digest: String::new(),
        };
        item.item_digest = item.digest();
        item.validate()?;
        Ok(item)
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let item: Self = serde_json::from_value(value.clone())
            .map_err(|_| "approval_inbox_decode_failed".to_owned())?;
        item.validate()?;
        Ok(item)
    }

    pub fn decide(
        &self,
        approver: AuthenticatedPrincipalRef,
        decision: ApprovalDecision,
        now_unix_ms: u64,
    ) -> Result<Self, String> {
        self.validate()?;
        approver
            .validate()
            .map_err(|_| SecurityReasonCode::AuthPrincipalMissing.as_str().to_owned())?;
        if approver.principal_id == self.binding.principal.principal_id {
            return Err(SecurityReasonCode::PolicySelfApprovalForbidden
                .as_str()
                .to_owned());
        }
        if self.status != HumanInboxStatus::Pending {
            return Err("approval_inbox_terminal".to_owned());
        }
        if now_unix_ms >= self.binding.expires_at_unix_ms {
            let mut expired = self.clone();
            expired.status = HumanInboxStatus::Expired;
            expired.revision = expired.revision.saturating_add(1);
            expired.item_digest = expired.digest();
            expired.validate()?;
            return Err(SecurityReasonCode::PolicyApprovalExpired
                .as_str()
                .to_owned());
        }
        let mut next = self.clone();
        next.status = match decision {
            ApprovalDecision::Approve => HumanInboxStatus::Approved,
            ApprovalDecision::Deny => HumanInboxStatus::Denied,
        };
        next.decided_by = Some(approver);
        next.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| "approval_inbox_revision_exhausted".to_owned())?;
        next.item_digest = next.digest();
        next.validate()?;
        Ok(next)
    }

    pub fn consume(&self) -> Result<Self, String> {
        self.validate()?;
        if self.status != HumanInboxStatus::Approved {
            return Err("approval_inbox_not_approved".to_owned());
        }
        let mut next = self.clone();
        next.status = HumanInboxStatus::Consumed;
        next.revision = self.revision.saturating_add(1);
        next.item_digest = next.digest();
        next.validate()?;
        Ok(next)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != HUMAN_INBOX_ITEM_SCHEMA
            || !self.version.is_compatible_with(&APPROVAL_BINDING_VERSION)
            || self.item_id != self.binding.approval_id
            || self.revision == 0
        {
            return Err("approval_inbox_header_invalid".to_owned());
        }
        self.binding.validate()?;
        if let Some(decided_by) = &self.decided_by {
            decided_by
                .validate()
                .map_err(|_| "approval_inbox_decider_invalid".to_owned())?;
            if self.status == HumanInboxStatus::Pending {
                return Err("approval_inbox_decider_unexpected".to_owned());
            }
        }
        if self.status == HumanInboxStatus::Approved && self.decided_by.is_none() {
            return Err("approval_inbox_decider_required".to_owned());
        }
        validate_digest(&self.item_digest, "approval_inbox_digest")?;
        if self.item_digest != self.digest() {
            return Err("approval_inbox_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "item_id": self.item_id,
            "binding": self.binding,
            "status": self.status,
            "decided_by": self.decided_by,
            "revision": self.revision,
        }))
    }
}

#[allow(dead_code)]
fn _approval_challenge_is_subject_only(challenge: &ApprovalChallenge) -> bool {
    !challenge.reason.contains("bearer ") && challenge.request_hash.starts_with("sha256:")
}

fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
