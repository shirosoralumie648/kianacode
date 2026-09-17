//! ControlPlane helpers for server-owned resource leases.
//!
//! The helpers only issue/revalidate immutable domain leases from the existing authority stream;
//! they never open a path, call a handler, or replace the kernel lock held by the adapter.

use crate::{ControlPlane, CoreError};
use kiana_domain::{
    canonical_resource_set, FenceTokenId, RequestContext, ResourceLease, RunId, StorageLockId,
};
use kiana_ports::PortError;

impl ControlPlane {
    /// Issue a short-lived resource observation bound to the authenticated session/run/cell.
    /// Actual OS lock acquisition remains in the existing path-lock adapter.
    pub async fn issue_resource_lease(
        &self,
        context: &RequestContext,
        run_id: RunId,
        cell_id: Option<kiana_domain::CellId>,
        resource: &str,
        issued_at_unix_ms: u64,
        ttl_ms: u64,
    ) -> Result<ResourceLease, CoreError> {
        if !context.project_trusted {
            return Err(PortError::Failed("project_untrusted".to_owned()).into());
        }
        if ttl_ms == 0 || resource.trim().is_empty() {
            return Err(PortError::Failed("resource_lease_invalid".to_owned()).into());
        }
        let resource = canonical_resource_set(&[resource.to_owned()])
            .map_err(PortError::Failed)?
            .into_iter()
            .next()
            .ok_or_else(|| PortError::Failed("resource_lease_invalid".to_owned()))?;
        let authority_epoch = self
            .authority_epoch(&context.project_root)
            .await?
            .unwrap_or(1);
        let expires_at_unix_ms = issued_at_unix_ms
            .checked_add(ttl_ms)
            .ok_or_else(|| PortError::Failed("resource_lease_expiry_overflow".to_owned()))?;
        ResourceLease::new(
            StorageLockId::new(),
            FenceTokenId::new(),
            resource,
            run_id,
            cell_id,
            context.session_id.as_str(),
            authority_epoch,
            issued_at_unix_ms,
            expires_at_unix_ms,
        )
        .map_err(PortError::Failed)
        .map_err(CoreError::from)
    }

    /// Revalidate the lease and requested path against current authority before an effect.
    pub async fn validate_resource_lease(
        &self,
        context: &RequestContext,
        lease: &ResourceLease,
        run_id: RunId,
        cell_id: Option<kiana_domain::CellId>,
        resource: &str,
        now_unix_ms: u64,
        fence_token: FenceTokenId,
    ) -> Result<(), CoreError> {
        if lease.owner_run_id != run_id
            || lease.owner_cell_id != cell_id
            || lease.session_id != context.session_id
            || !lease.covers(resource).map_err(PortError::Failed)?
        {
            return Err(
                PortError::Failed("resource_lease_owner_or_scope_mismatch".to_owned()).into(),
            );
        }
        let authority_epoch = self
            .authority_epoch(&context.project_root)
            .await?
            .unwrap_or(1);
        lease
            .validate_current(now_unix_ms, authority_epoch, fence_token)
            .map_err(PortError::Failed)
            .map_err(CoreError::from)
    }
}
