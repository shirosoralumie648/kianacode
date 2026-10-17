//! BQ-19 cost allocation contracts.
//!
//! A `NormalizedUsage`/ledger leaf is charged once.  `CostAllocation` carries every reporting
//! dimension on that one leaf so project, organization, workflow, cell and run views can be
//! rebuilt without creating one charge per dimension.  Ownership comes from the server-owned
//! scope; wire supplied scope is only an assertion that must match it.  Cross-project attribution
//! requires an active, exact `SharingGrant` and never falls back to a caller supplied project.
//!
//! `CostAllocationLedger` is a deterministic, side-effect-free reducer that refuses a second
//! charge for one usage leaf, a duplicate allocation/source event or a source-cursor regression.
//! It does not append EventLog facts, settle reservations or authorize anything; callers persist
//! the accepted leaf through the existing ControlPlane/EventLog transaction.

use crate::{
    json_digest, BillingUnknownReason, CellId, CostBreakdown, CostBreakdownKind, EventId, Money,
    OrganizationId, ProjectId, ProviderReceiptRef, RateCardId, RunId, SchemaVersion, SharingGrant,
    SharingGrantId, UsageId, WorkflowInstanceId,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

pub const COST_ALLOCATION_SCHEMA: &str = "kiana.cost-allocation.v1";
pub const COST_ALLOCATION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const COST_ALLOCATION_EVENT: &str = "cost.allocation";
pub const COST_ALLOCATION_OPERATION: &str = "cost.allocate";

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// All dimensions describe one leaf. They are labels for independent read views, not amounts to
/// add together. Every field is server-owned and is required for BQ-19 allocation facts.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllocationScope {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub workflow_id: WorkflowInstanceId,
    pub cell_id: CellId,
    pub run_id: RunId,
}

impl AllocationScope {
    pub fn new(
        organization_id: OrganizationId,
        project_id: ProjectId,
        workflow_id: WorkflowInstanceId,
        cell_id: CellId,
        run_id: RunId,
    ) -> Result<Self, String> {
        let scope = Self {
            organization_id,
            project_id,
            workflow_id,
            cell_id,
            run_id,
        };
        scope.validate()?;
        Ok(scope)
    }

    pub fn validate(&self) -> Result<(), String> {
        if [
            self.organization_id.as_uuid(),
            self.project_id.as_uuid(),
            self.workflow_id.as_uuid(),
            self.cell_id.as_uuid(),
            self.run_id.as_uuid(),
        ]
        .iter()
        .any(|value| value.is_nil())
        {
            return Err("cost_allocation_scope_invalid".to_owned());
        }
        Ok(())
    }
}

/// A compact, secret-free reference to a cross-project SharingGrant. The full grant remains an
/// authority projection and is checked by ControlPlane before an allocation event is built.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharingGrantRef {
    pub grant_id: SharingGrantId,
    pub source_project: ProjectId,
    pub target_project: ProjectId,
    pub grant_digest: String,
}

impl SharingGrantRef {
    pub fn from_grant(grant: &SharingGrant) -> Result<Self, String> {
        grant.validate()?;
        let reference = Self {
            grant_id: grant.grant_id,
            source_project: grant.source_project,
            target_project: grant.target_project,
            grant_digest: grant.grant_digest.clone(),
        };
        reference.validate()?;
        Ok(reference)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.grant_id.as_uuid().is_nil()
            || self.source_project.as_uuid().is_nil()
            || self.target_project.as_uuid().is_nil()
            || self.source_project == self.target_project
        {
            return Err("cost_allocation_sharing_grant_binding_invalid".to_owned());
        }
        digest(&self.grant_digest, "cost_allocation_grant_digest")
    }

    pub fn matches(&self, grant: &SharingGrant) -> bool {
        self.grant_id == grant.grant_id
            && self.source_project == grant.source_project
            && self.target_project == grant.target_project
            && self.grant_digest == grant.grant_digest
    }
}

/// Estimated, measured and unknown are mutually exclusive. Unknown never becomes a numeric zero.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AllocationCostKind {
    Estimated {
        amount: Money,
        rate_card_id: RateCardId,
        rate_card_version: u64,
    },
    Measured {
        amount: Money,
        provider_receipt: ProviderReceiptRef,
    },
    Unknown {
        reason: BillingUnknownReason,
    },
}

impl AllocationCostKind {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Estimated {
                amount,
                rate_card_id,
                rate_card_version,
            } => {
                amount.validate()?;
                if amount.micros < 0 {
                    return Err("cost_allocation_estimated_amount_negative".to_owned());
                }
                if rate_card_id.as_uuid().is_nil() || *rate_card_version == 0 {
                    return Err("cost_allocation_rate_card_invalid".to_owned());
                }
            }
            Self::Measured {
                amount,
                provider_receipt,
            } => {
                amount.validate()?;
                if amount.micros < 0 {
                    return Err("cost_allocation_measured_amount_negative".to_owned());
                }
                ProviderReceiptRef::new(provider_receipt.as_str().to_owned())
                    .map(|_| ())
                    .map_err(|_| "cost_allocation_provider_receipt_invalid".to_owned())?;
            }
            Self::Unknown { .. } => {}
        }
        Ok(())
    }

    pub fn estimated_amount(&self) -> Option<&Money> {
        match self {
            Self::Estimated { amount, .. } => Some(amount),
            _ => None,
        }
    }

    pub fn measured_amount(&self) -> Option<&Money> {
        match self {
            Self::Measured { amount, .. } => Some(amount),
            _ => None,
        }
    }

    pub fn unknown_reason(&self) -> Option<BillingUnknownReason> {
        match self {
            Self::Unknown { reason } => Some(*reason),
            _ => None,
        }
    }
}

/// One immutable allocation fact. It has one usage leaf and one cost kind while carrying all
/// rollup labels. Parent/child allocations are represented by labels and never by extra entries.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CostAllocation {
    pub schema: String,
    pub version: SchemaVersion,
    pub allocation_id: crate::CostAllocationId,
    pub usage_id: UsageId,
    pub source_project_id: ProjectId,
    #[serde(flatten)]
    pub scope: AllocationScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sharing_grant: Option<SharingGrantRef>,
    pub cost: AllocationCostKind,
    pub source_event_id: EventId,
    pub source_cursor: u64,
    pub revision: u64,
    pub source_digest: String,
    pub allocation_digest: String,
}

impl CostAllocation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        allocation_id: crate::CostAllocationId,
        usage_id: UsageId,
        source_project_id: ProjectId,
        scope: AllocationScope,
        sharing_grant: Option<SharingGrantRef>,
        cost: AllocationCostKind,
        source_event_id: EventId,
        source_cursor: u64,
        revision: u64,
        source_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut allocation = Self {
            schema: COST_ALLOCATION_SCHEMA.to_owned(),
            version: COST_ALLOCATION_VERSION,
            allocation_id,
            usage_id,
            source_project_id,
            scope,
            sharing_grant,
            cost,
            source_event_id,
            source_cursor,
            revision,
            source_digest: source_digest.into(),
            allocation_digest: String::new(),
        };
        allocation.allocation_digest = allocation.digest();
        allocation.validate()?;
        Ok(allocation)
    }

    /// Convert exactly one BQ-13 leaf observation into one allocation fact. Cost kind is copied,
    /// never recalculated or merged, and the BQ-13 breakdown digest becomes the source fence.
    pub fn from_cost_breakdown(
        allocation_id: crate::CostAllocationId,
        usage_id: UsageId,
        source_project_id: ProjectId,
        scope: AllocationScope,
        sharing_grant: Option<SharingGrantRef>,
        breakdown: &CostBreakdown,
        source_event_id: EventId,
        source_cursor: u64,
        revision: u64,
    ) -> Result<Self, String> {
        breakdown.validate()?;
        if breakdown.run_id != scope.run_id {
            return Err("cost_allocation_breakdown_run_mismatch".to_owned());
        }
        let cost = match &breakdown.kind {
            CostBreakdownKind::Estimated { estimate } => AllocationCostKind::Estimated {
                amount: estimate
                    .amount
                    .clone()
                    .ok_or_else(|| "cost_allocation_estimate_missing".to_owned())?,
                rate_card_id: estimate.rate_card_id,
                rate_card_version: estimate.rate_card_version,
            },
            CostBreakdownKind::Measured {
                amount,
                provider_receipt,
            } => AllocationCostKind::Measured {
                amount: amount.clone(),
                provider_receipt: provider_receipt.clone(),
            },
            CostBreakdownKind::Unknown { reason } => {
                AllocationCostKind::Unknown { reason: *reason }
            }
        };
        Self::new(
            allocation_id,
            usage_id,
            source_project_id,
            scope,
            sharing_grant,
            cost,
            source_event_id,
            source_cursor,
            revision,
            breakdown.breakdown_digest.clone(),
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != COST_ALLOCATION_SCHEMA
            || !self.version.is_compatible_with(&COST_ALLOCATION_VERSION)
            || self.allocation_id.as_uuid().is_nil()
            || self.usage_id.as_uuid().is_nil()
            || self.source_project_id.as_uuid().is_nil()
            || self.source_event_id.as_uuid().is_nil()
            || self.source_cursor == 0
            || self.revision == 0
        {
            return Err("cost_allocation_header_invalid".to_owned());
        }
        self.scope.validate()?;
        digest(&self.source_digest, "cost_allocation_source_digest")?;
        digest(&self.allocation_digest, "cost_allocation_digest")?;
        self.cost.validate()?;
        if let Some(grant) = &self.sharing_grant {
            grant.validate()?;
        }
        if self.source_project_id == self.scope.project_id {
            if self.sharing_grant.is_some() {
                return Err("cost_allocation_same_project_grant_unexpected".to_owned());
            }
        } else {
            let Some(grant) = self.sharing_grant.as_ref() else {
                return Err("cost_allocation_cross_project_sharing_grant_required".to_owned());
            };
            if grant.source_project != self.source_project_id
                || grant.target_project != self.scope.project_id
            {
                return Err("cost_allocation_sharing_grant_project_mismatch".to_owned());
            }
        }
        if self.allocation_digest != self.digest() {
            return Err("cost_allocation_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Check the server-owned sharing grant that this allocation claims.
    ///
    /// A same-project allocation must not carry a grant.  A cross-project allocation must carry
    /// exactly this grant and the grant must be active at `now_unix_ms` under `authority_epoch`
    /// and explicitly allow `cost.allocate`.  The reference binding was already compared in
    /// `validate`; this method adds the live grant checks so Domain and ControlPlane share one
    /// rule instead of drifting apart.
    pub fn validate_grant_binding(
        &self,
        grant: &SharingGrant,
        now_unix_ms: u64,
        authority_epoch: u64,
    ) -> Result<(), String> {
        if self.source_project_id == self.scope.project_id {
            return Err("cost_allocation_same_project_grant_unexpected".to_owned());
        }
        grant.validate()?;
        let Some(reference) = self.sharing_grant.as_ref() else {
            return Err("cost_allocation_sharing_grant_reference_required".to_owned());
        };
        if !reference.matches(grant) {
            return Err("cost_allocation_sharing_grant_reference_mismatch".to_owned());
        }
        if !grant.active_at(now_unix_ms, authority_epoch) {
            return Err("cost_allocation_sharing_grant_inactive".to_owned());
        }
        if !grant
            .operations
            .iter()
            .any(|operation| operation == COST_ALLOCATION_OPERATION)
        {
            return Err("cost_allocation_sharing_grant_operation_missing".to_owned());
        }
        Ok(())
    }

    /// Compare a wire-supplied scope with the immutable server-owned allocation scope. A wire
    /// actor/project/workflow/cell/run assertion never grants ownership.
    pub fn validate_wire_scope(&self, reported: &AllocationScope) -> Result<(), String> {
        self.validate_wire_attribution(
            reported,
            self.source_project_id,
            self.sharing_grant.as_ref(),
        )
    }

    /// Compare every wire-reported attribution field with the server-owned fact.
    ///
    /// Scope, source project and grant reference are assertions on the request; a mismatch in any
    /// of them is refused before an event is built and never silently rewrites the allocation.
    pub fn validate_wire_attribution(
        &self,
        reported_scope: &AllocationScope,
        reported_source_project: ProjectId,
        reported_grant: Option<&SharingGrantRef>,
    ) -> Result<(), String> {
        self.validate()?;
        reported_scope.validate()?;
        if let Some(grant) = reported_grant {
            grant.validate()?;
        }
        if reported_scope != &self.scope {
            return Err("cost_allocation_wire_scope_mismatch".to_owned());
        }
        if reported_source_project != self.source_project_id {
            return Err("cost_allocation_wire_source_project_mismatch".to_owned());
        }
        if reported_grant != self.sharing_grant.as_ref() {
            return Err("cost_allocation_wire_sharing_grant_mismatch".to_owned());
        }
        Ok(())
    }

    /// Bind this allocation to the server context before producing an event.
    pub fn validate_server_binding(
        &self,
        trusted_scope: &AllocationScope,
        trusted_source_project: ProjectId,
    ) -> Result<(), String> {
        self.validate()?;
        trusted_scope.validate()?;
        if &self.scope != trusted_scope {
            return Err("cost_allocation_server_scope_mismatch".to_owned());
        }
        if self.source_project_id != trusted_source_project {
            return Err("cost_allocation_server_source_project_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "allocation_id": self.allocation_id,
            "usage_id": self.usage_id,
            "source_project_id": self.source_project_id,
            "scope": self.scope,
            "sharing_grant": self.sharing_grant,
            "cost": self.cost,
            "source_event_id": self.source_event_id,
            "source_cursor": self.source_cursor,
            "revision": self.revision,
            "source_digest": self.source_digest,
        }))
    }
}

/// Result of appending one allocation leaf to the in-memory ledger.
#[derive(Clone, Debug, PartialEq)]
pub enum CostAllocationAppendOutcome {
    /// A new leaf was accepted once.
    Appended(CostAllocation),
    /// The same allocation id and digest were seen again; the original fact is returned and no
    /// second charge is created.
    Replayed(CostAllocation),
}

/// Deterministic, append-only reducer over allocation leaves.
///
/// It enforces the BQ-19 uniqueness rule: one `usage_id` may be charged exactly once, no matter
/// how many organization/project/workflow/cell/run labels the leaf carries and no matter whether a
/// later request calls itself parent or child. It stores no mutable amount, exposes no
/// update/delete operation and never appends EventLog facts itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CostAllocationLedger {
    allocations: BTreeMap<crate::CostAllocationId, CostAllocation>,
    usage_leaves: BTreeMap<UsageId, crate::CostAllocationId>,
    source_events: BTreeMap<EventId, crate::CostAllocationId>,
    source_cursor: u64,
}

impl CostAllocationLedger {
    /// Append one leaf or replay the original fact for an identical repeat.
    ///
    /// Refusals: a different fact for a known allocation id (`cost_allocation_replay_conflict`),
    /// a second charge for the same usage leaf (`cost_allocation_usage_already_charged`), a reused
    /// source event (`cost_allocation_source_event_duplicate`) and a source cursor that does not
    /// strictly increase (`cost_allocation_source_cursor_regression`).
    pub fn append(
        &mut self,
        allocation: CostAllocation,
    ) -> Result<CostAllocationAppendOutcome, String> {
        allocation.validate()?;
        if let Some(existing) = self.allocations.get(&allocation.allocation_id) {
            if existing.allocation_digest == allocation.allocation_digest {
                return Ok(CostAllocationAppendOutcome::Replayed(existing.clone()));
            }
            return Err("cost_allocation_replay_conflict".to_owned());
        }
        if self.usage_leaves.contains_key(&allocation.usage_id) {
            return Err("cost_allocation_usage_already_charged".to_owned());
        }
        if self.source_events.contains_key(&allocation.source_event_id) {
            return Err("cost_allocation_source_event_duplicate".to_owned());
        }
        if allocation.source_cursor <= self.source_cursor {
            return Err("cost_allocation_source_cursor_regression".to_owned());
        }
        self.source_cursor = allocation.source_cursor;
        self.usage_leaves
            .insert(allocation.usage_id, allocation.allocation_id);
        self.source_events
            .insert(allocation.source_event_id, allocation.allocation_id);
        self.allocations
            .insert(allocation.allocation_id, allocation.clone());
        Ok(CostAllocationAppendOutcome::Appended(allocation))
    }

    /// Borrow the single allocation charged for a usage leaf, if any.
    pub fn allocation_for_usage(&self, usage_id: UsageId) -> Option<&CostAllocation> {
        self.usage_leaves
            .get(&usage_id)
            .and_then(|allocation_id| self.allocations.get(allocation_id))
    }

    /// Borrow one allocation by its id.
    pub fn allocation(&self, allocation_id: crate::CostAllocationId) -> Option<&CostAllocation> {
        self.allocations.get(&allocation_id)
    }

    /// Iterate accepted leaves in allocation-id order.
    pub fn allocations(&self) -> impl Iterator<Item = &CostAllocation> {
        self.allocations.values()
    }

    /// Number of accepted (never duplicated) leaves.
    pub fn leaf_count(&self) -> usize {
        self.allocations.len()
    }

    /// Highest source cursor accepted so far.
    pub fn source_cursor(&self) -> u64 {
        self.source_cursor
    }
}
