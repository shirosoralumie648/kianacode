use async_trait::async_trait;
use kiana_domain::{
    ApprovalChallenge, ApprovalId, CapabilityRequest, PendingApproval, PermissionProfile,
    RequestContext, SessionId, APPROVAL_CHALLENGE_SCHEMA,
};
use kiana_ports::{ApprovalStorePort, PortError};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

const DEFAULT_APPROVAL_TTL: Duration = Duration::from_secs(5 * 60);

pub(crate) struct MemoryApprovalStore {
    records: Mutex<HashMap<ApprovalId, ApprovalRecord>>,
    ttl: Duration,
}

impl MemoryApprovalStore {
    pub(crate) fn new() -> Self {
        Self::with_ttl(DEFAULT_APPROVAL_TTL)
    }

    fn with_ttl(ttl: Duration) -> Self {
        Self {
            records: Mutex::new(HashMap::new()),
            ttl,
        }
    }
}

struct ApprovalRecord {
    pending: PendingApproval,
    binding: ApprovalBinding,
    expires_at: Instant,
    active: bool,
    consumed: bool,
}

struct ApprovalBinding {
    session_id: SessionId,
    actor_id: String,
    project_root: String,
    project_trusted: bool,
    permission_profile: PermissionProfile,
}

#[async_trait]
impl ApprovalStorePort for MemoryApprovalStore {
    async fn stage(
        &self,
        context: &RequestContext,
        request: CapabilityRequest,
        reason: &str,
    ) -> Result<ApprovalChallenge, PortError> {
        if request.request_id != context.request_id {
            return Err(PortError::Failed(
                "approval_request_context_mismatch".to_owned(),
            ));
        }
        let actor_id = context
            .actor_id
            .as_deref()
            .map(str::trim)
            .filter(|actor| !actor.is_empty())
            .ok_or_else(|| PortError::Failed("approval_actor_required".to_owned()))?;
        let request_hash = capability_request_hash(&request)?;
        let approval_id = ApprovalId::new();
        let expires_at = Instant::now()
            .checked_add(self.ttl)
            .ok_or_else(|| PortError::Failed("approval_expiry_overflow".to_owned()))?;
        let expires_at_unix_ms = unix_time_ms()
            .checked_add(self.ttl.as_millis().min(u128::from(u64::MAX)) as u64)
            .ok_or_else(|| PortError::Failed("approval_expiry_overflow".to_owned()))?;
        let challenge = ApprovalChallenge {
            schema: APPROVAL_CHALLENGE_SCHEMA.to_owned(),
            approval_id,
            request_id: request.request_id,
            request_hash,
            expires_at_unix_ms,
            reason: reason.to_owned(),
        };
        let record = ApprovalRecord {
            pending: PendingApproval {
                challenge: challenge.clone(),
                request,
            },
            binding: ApprovalBinding {
                session_id: context.session_id.clone(),
                actor_id: actor_id.to_owned(),
                project_root: context.project_root.clone(),
                project_trusted: context.project_trusted,
                permission_profile: context.permission_profile,
            },
            expires_at,
            active: false,
            consumed: false,
        };
        self.records.lock().await.insert(approval_id, record);
        Ok(challenge)
    }

    async fn activate(&self, approval_id: ApprovalId) -> Result<(), PortError> {
        let mut records = self.records.lock().await;
        let record = records
            .get_mut(&approval_id)
            .ok_or_else(|| PortError::Failed("approval_not_found".to_owned()))?;
        if record.consumed {
            return Err(PortError::Conflict("approval_already_consumed".to_owned()));
        }
        if Instant::now() >= record.expires_at {
            record.consumed = true;
            return Err(PortError::Failed("approval_expired".to_owned()));
        }
        record.active = true;
        Ok(())
    }

    async fn consume(
        &self,
        context: &RequestContext,
        approval_id: ApprovalId,
    ) -> Result<PendingApproval, PortError> {
        let mut records = self.records.lock().await;
        let record = records
            .get_mut(&approval_id)
            .ok_or_else(|| PortError::Failed("approval_not_found".to_owned()))?;
        if record.consumed {
            return Err(PortError::Conflict("approval_already_consumed".to_owned()));
        }
        if !record.active {
            return Err(PortError::Failed("approval_not_active".to_owned()));
        }
        if Instant::now() >= record.expires_at {
            record.consumed = true;
            return Err(PortError::Failed("approval_expired".to_owned()));
        }
        let actor_id = context
            .actor_id
            .as_deref()
            .map(str::trim)
            .filter(|actor| !actor.is_empty())
            .ok_or_else(|| PortError::Failed("approval_actor_required".to_owned()))?;
        if record.binding.session_id != context.session_id
            || record.binding.actor_id != actor_id
            || record.binding.project_root != context.project_root
            || record.binding.project_trusted != context.project_trusted
            || record.binding.permission_profile != context.permission_profile
        {
            return Err(PortError::Failed("approval_context_mismatch".to_owned()));
        }
        if capability_request_hash(&record.pending.request)?
            != record.pending.challenge.request_hash
        {
            record.consumed = true;
            return Err(PortError::Failed(
                "approval_request_integrity_mismatch".to_owned(),
            ));
        }
        record.consumed = true;
        Ok(record.pending.clone())
    }
}

fn capability_request_hash(request: &CapabilityRequest) -> Result<String, PortError> {
    let encoded = serde_json::to_vec(request)
        .map_err(|error| PortError::Failed(format!("approval_request_serialize:{error}")))?;
    let digest = Sha256::digest(encoded);
    Ok(format!("{digest:x}"))
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiana_domain::{CapabilityKind, RequestId};
    use serde_json::json;

    fn trusted_context(session: &str, actor: &str) -> RequestContext {
        let mut context = RequestContext::local(session, "/repo");
        context.actor_id = Some(actor.to_owned());
        context.project_trusted = true;
        context
    }

    fn local_write(context: &RequestContext) -> CapabilityRequest {
        CapabilityRequest::new(
            context.request_id,
            CapabilityKind::Filesystem,
            "context.index.cache.write",
            json!({ "path": ".kiana/context-index.json" }),
        )
        .with_risk(kiana_domain::RiskLevel::LocalWrite)
    }

    #[tokio::test]
    async fn approval_is_inactive_until_event_commit_then_consumed_once() {
        let store = MemoryApprovalStore::new();
        let context = trusted_context("session-1", "actor-1");
        let challenge = store
            .stage(
                &context,
                local_write(&context),
                "local_write_requires_approval",
            )
            .await
            .unwrap();
        assert_eq!(challenge.request_id, context.request_id);
        assert_eq!(challenge.request_hash.len(), 64);
        assert_eq!(
            store
                .consume(&context, challenge.approval_id)
                .await
                .unwrap_err(),
            PortError::Failed("approval_not_active".to_owned())
        );

        store.activate(challenge.approval_id).await.unwrap();
        let pending = store
            .consume(&context, challenge.approval_id)
            .await
            .unwrap();
        assert_eq!(
            pending.request.arguments["path"],
            ".kiana/context-index.json"
        );
        assert_eq!(
            store
                .consume(&context, challenge.approval_id)
                .await
                .unwrap_err(),
            PortError::Conflict("approval_already_consumed".to_owned())
        );
    }

    #[tokio::test]
    async fn wrong_context_does_not_consume_the_rightful_approval() {
        let store = MemoryApprovalStore::new();
        let context = trusted_context("session-1", "actor-1");
        let challenge = store
            .stage(&context, local_write(&context), "approval_required")
            .await
            .unwrap();
        store.activate(challenge.approval_id).await.unwrap();

        let wrong_actor = trusted_context("session-1", "actor-2");
        assert_eq!(
            store
                .consume(&wrong_actor, challenge.approval_id)
                .await
                .unwrap_err(),
            PortError::Failed("approval_context_mismatch".to_owned())
        );
        assert!(store.consume(&context, challenge.approval_id).await.is_ok());
    }

    #[tokio::test]
    async fn expired_and_tampered_approvals_fail_closed() {
        let expiring = MemoryApprovalStore::with_ttl(Duration::from_millis(1));
        let context = trusted_context("session-1", "actor-1");
        let challenge = expiring
            .stage(&context, local_write(&context), "approval_required")
            .await
            .unwrap();
        expiring.activate(challenge.approval_id).await.unwrap();
        tokio::time::sleep(Duration::from_millis(2)).await;
        assert_eq!(
            expiring
                .consume(&context, challenge.approval_id)
                .await
                .unwrap_err(),
            PortError::Failed("approval_expired".to_owned())
        );

        let tampered = MemoryApprovalStore::new();
        let challenge = tampered
            .stage(&context, local_write(&context), "approval_required")
            .await
            .unwrap();
        tampered.activate(challenge.approval_id).await.unwrap();
        tampered
            .records
            .lock()
            .await
            .get_mut(&challenge.approval_id)
            .unwrap()
            .pending
            .request
            .request_id = RequestId::new();
        assert_eq!(
            tampered
                .consume(&context, challenge.approval_id)
                .await
                .unwrap_err(),
            PortError::Failed("approval_request_integrity_mismatch".to_owned())
        );
    }
}
