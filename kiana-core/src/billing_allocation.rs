//! ControlPlane admission for BQ-19 allocations.
//!
//! Allocation is an authorization-bound source fact. This module validates server-owned scope,
//! rejects wire ownership assertions and requires an active exact SharingGrant for cross-project
//! attribution. It only builds an immutable RuntimeEvent; EventStore append and any financial
//! authority remain on the existing DaemonHost → ControlPlane path.

use kiana_domain::{
    AllocationScope, CostAllocation, ProjectId, RuntimeEvent, SharingGrant, COST_ALLOCATION_EVENT,
    COST_ALLOCATION_OPERATION,
};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CostAllocationAdmissionError {
    #[error("cost_allocation_invalid:{0}")]
    AllocationInvalid(String),
    #[error("cost_allocation_server_binding_invalid:{0}")]
    ServerBindingInvalid(String),
    #[error("cost_allocation_wire_scope_invalid:{0}")]
    WireScopeInvalid(String),
    #[error("cost_allocation_sharing_grant_invalid:{0}")]
    SharingGrantInvalid(String),
    #[error("cost_allocation_event_invalid:{0}")]
    EventInvalid(String),
}

pub struct CostAllocationAdmission;

impl CostAllocationAdmission {
    pub fn authorize(
        allocation: &CostAllocation,
        trusted_scope: &AllocationScope,
        trusted_source_project: ProjectId,
        sharing_grant: Option<&SharingGrant>,
        now_unix_ms: u64,
        authority_epoch: u64,
    ) -> Result<(), CostAllocationAdmissionError> {
        allocation
            .validate()
            .map_err(CostAllocationAdmissionError::AllocationInvalid)?;
        allocation
            .validate_server_binding(trusted_scope, trusted_source_project)
            .map_err(CostAllocationAdmissionError::ServerBindingInvalid)?;
        Self::authorize_sharing_grant(allocation, sharing_grant, now_unix_ms, authority_epoch)?;
        Ok(())
    }

    /// Wire fields are assertions only. Any project/org/workflow/cell/run drift from the server
    /// context is denied before an event can be produced.
    pub fn authorize_wire(
        allocation: &CostAllocation,
        reported_scope: &AllocationScope,
        trusted_scope: &AllocationScope,
        trusted_source_project: ProjectId,
        sharing_grant: Option<&SharingGrant>,
        now_unix_ms: u64,
        authority_epoch: u64,
    ) -> Result<(), CostAllocationAdmissionError> {
        allocation
            .validate_wire_scope(reported_scope)
            .map_err(CostAllocationAdmissionError::WireScopeInvalid)?;
        Self::authorize(
            allocation,
            trusted_scope,
            trusted_source_project,
            sharing_grant,
            now_unix_ms,
            authority_epoch,
        )
    }

    fn authorize_sharing_grant(
        allocation: &CostAllocation,
        grant: Option<&SharingGrant>,
        now_unix_ms: u64,
        authority_epoch: u64,
    ) -> Result<(), CostAllocationAdmissionError> {
        if allocation.source_project_id == allocation.scope.project_id {
            if grant.is_some() {
                return Err(CostAllocationAdmissionError::SharingGrantInvalid(
                    "same_project_grant_unexpected".to_owned(),
                ));
            }
            return Ok(());
        }
        let Some(grant) = grant else {
            return Err(CostAllocationAdmissionError::SharingGrantInvalid(
                "cross_project_sharing_grant_required".to_owned(),
            ));
        };
        grant
            .validate()
            .map_err(CostAllocationAdmissionError::SharingGrantInvalid)?;
        let Some(reference) = allocation.sharing_grant.as_ref() else {
            return Err(CostAllocationAdmissionError::SharingGrantInvalid(
                "allocation_sharing_grant_ref_required".to_owned(),
            ));
        };
        if !reference.matches(grant)
            || !grant.active_at(now_unix_ms, authority_epoch)
            || !grant
                .operations
                .iter()
                .any(|operation| operation == COST_ALLOCATION_OPERATION)
        {
            return Err(CostAllocationAdmissionError::SharingGrantInvalid(
                "sharing_grant_scope_or_expiry_invalid".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn allocation_event(
        request_id: kiana_domain::RequestId,
        sequence: u64,
        allocation: &CostAllocation,
    ) -> Result<RuntimeEvent, CostAllocationAdmissionError> {
        allocation
            .validate()
            .map_err(CostAllocationAdmissionError::AllocationInvalid)?;
        let data = serde_json::to_value(allocation)
            .map_err(|_| CostAllocationAdmissionError::EventInvalid("encode_failed".to_owned()))?;
        RuntimeEvent::new(request_id, sequence, COST_ALLOCATION_EVENT, data)
            .map(|event| {
                event.with_stream_metadata(
                    "cost_allocation",
                    allocation.allocation_id.to_string(),
                    allocation.revision,
                )
            })
            .map_err(|error| CostAllocationAdmissionError::EventInvalid(error.to_string()))
    }
}

impl super::ControlPlane {
    pub fn admit_cost_allocation(
        &self,
        allocation: &CostAllocation,
        trusted_scope: &AllocationScope,
        trusted_source_project: ProjectId,
        sharing_grant: Option<&SharingGrant>,
        now_unix_ms: u64,
        authority_epoch: u64,
    ) -> Result<(), CostAllocationAdmissionError> {
        CostAllocationAdmission::authorize(
            allocation,
            trusted_scope,
            trusted_source_project,
            sharing_grant,
            now_unix_ms,
            authority_epoch,
        )
    }
}
