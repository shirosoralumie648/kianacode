//! ControlPlane authority/session/policy fence helpers.
//!
//! Fence issuance is a read-only snapshot operation. The EventLog authority stream remains the
//! source of truth; this module only turns the observed epoch/revision into the typed domain fence
//! and rechecks it before a caller proceeds to an existing admission path.

use crate::{ControlPlane, CoreError};
use kiana_domain::{json_digest, AuthorityFence, FenceTokenId, RequestContext};
use kiana_ports::PortError;
use serde_json::json;

impl ControlPlane {
    /// Issue a short-lived fence from the current authority stream. Policy/config strings are
    /// normalized to digests; no authority is granted until the normal policy/gate/permit path.
    pub async fn issue_authority_fence(
        &self,
        context: &RequestContext,
        session_generation: u64,
        policy_revision: &str,
        config_revision: &str,
        issued_at_unix_ms: u64,
        ttl_ms: u64,
    ) -> Result<AuthorityFence, CoreError> {
        if ttl_ms == 0 {
            return Err(PortError::Failed("UNKNOWN_FENCE_EXPIRED".to_owned()).into());
        }
        let authority_epoch = self
            .authority_epoch(&context.project_root)
            .await?
            .unwrap_or(1);
        let policy_revision = normalize_digest(policy_revision);
        let config_revision = normalize_digest(config_revision);
        let expires_at = issued_at_unix_ms
            .checked_add(ttl_ms)
            .ok_or_else(|| PortError::Failed("UNKNOWN_FENCE_EXPIRED".to_owned()))?;
        AuthorityFence::new(
            FenceTokenId::new(),
            Self::canonical_project_root(&context.project_root)
                .to_string_lossy()
                .into_owned(),
            context.session_id.as_str(),
            authority_epoch,
            session_generation,
            policy_revision,
            config_revision,
            issued_at_unix_ms,
            expires_at,
        )
        .map_err(|error| PortError::Failed(error).into())
    }

    /// Recheck the EventLog authority epoch and caller-owned session/policy/config revisions.
    /// Stale fences return structured stable reason strings and never fall back to an older one.
    pub async fn validate_authority_fence(
        &self,
        context: &RequestContext,
        fence: &AuthorityFence,
        now_unix_ms: u64,
        session_generation: u64,
        policy_revision: &str,
        config_revision: &str,
    ) -> Result<(), CoreError> {
        let current_epoch = self
            .authority_epoch(&context.project_root)
            .await?
            .unwrap_or(1);
        let expected_scope = Self::canonical_project_root(&context.project_root)
            .to_string_lossy()
            .into_owned();
        if fence.scope != expected_scope || fence.session_id != context.session_id {
            return Err(PortError::Failed("FACT_FENCE_MISMATCH".to_owned()).into());
        }
        fence
            .validate_current(
                now_unix_ms,
                current_epoch,
                session_generation,
                &normalize_digest(policy_revision),
                &normalize_digest(config_revision),
            )
            .map_err(PortError::Failed)
            .map_err(CoreError::from)
    }

    /// Read-only current authority metadata for a diagnostics/refresh caller.
    pub async fn authority_fence_snapshot(
        &self,
        context: &RequestContext,
    ) -> Result<(u64, String), CoreError> {
        let epoch = self
            .authority_epoch(&context.project_root)
            .await?
            .unwrap_or(1);
        let revision = self
            .authority_revision(&context.project_root)
            .await?
            .map(|value| json_digest(&json!({"authority_revision":value})))
            .unwrap_or_else(|| json_digest(&json!({"authority_revision":"uninitialized"})));
        Ok((epoch, revision))
    }
}

fn normalize_digest(value: &str) -> String {
    if value.starts_with("sha256:") {
        value.to_owned()
    } else {
        json_digest(&json!({"revision": value}))
    }
}
