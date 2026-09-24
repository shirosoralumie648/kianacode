//! Deny-first deletion planning over the SC-22 retention boundary.
//!
//! This module only prepares an appendable tombstone batch and an explicit propagation manifest.
//! It does not erase bytes, mutate projections or call an adapter. Every target starts as
//! `Unknown` until its own receipt is supplied by a later, authorized operation.

use kiana_domain::{
    DataGovernanceSnapshot, DataPropagationPlan, DeleteRequest, DeletionManifest, DeletionPlan,
    DeletionPropagation, DeletionTombstone, ReceiptDataBinding, RetentionDisposition,
    RetentionScan,
};

const DELETION_TARGETS: [&str; 9] = [
    "receipt", "audit", "artifact", "memory", "index", "cache", "export", "backup", "external",
];

/// Prepare deletion facts only when the request and retention scan are the same authority view.
pub fn plan_deletion(
    request: &DeleteRequest,
    scan: &RetentionScan,
) -> Result<DeletionPlan, String> {
    request.validate()?;
    scan.validate()?;
    if request.project_ref != scan.project_ref {
        return Err("deletion_project_boundary_mismatch".to_owned());
    }
    if request.policy_revision != scan.policy_revision {
        return Err("deletion_policy_revision_stale".to_owned());
    }
    if request.data_epoch != scan.data_epoch {
        return Err("deletion_data_epoch_stale".to_owned());
    }
    if request.source_cursor != scan.source_cursor {
        return Err("deletion_source_cursor_stale".to_owned());
    }

    let next_data_epoch = request
        .data_epoch
        .checked_add(1)
        .ok_or_else(|| "deletion_data_epoch_exhausted".to_owned())?;
    let mut tombstones = Vec::with_capacity(request.object_refs.len());
    for object_ref in &request.object_refs {
        let decision = scan
            .decisions
            .iter()
            .find(|decision| decision.object_ref == *object_ref)
            .ok_or_else(|| "deletion_target_not_in_scan".to_owned())?;
        match decision.disposition {
            RetentionDisposition::Held => {
                return Err("deletion_legal_hold_active".to_owned());
            }
            RetentionDisposition::Unknown => {
                return Err("deletion_retention_unknown".to_owned());
            }
            RetentionDisposition::Retain => {
                return Err("deletion_retention_not_eligible".to_owned());
            }
            RetentionDisposition::Eligible => {}
        }
        tombstones.push(DeletionTombstone::new(
            request,
            object_ref.clone(),
            decision.source_digest.clone(),
            next_data_epoch,
        )?);
    }
    let propagation = DELETION_TARGETS
        .into_iter()
        .map(|target| DeletionPropagation::unknown(target, "adapter_receipt_required"))
        .collect();
    let manifest = DeletionManifest::new(request, next_data_epoch, &tombstones, propagation)?;
    DeletionPlan::new(request.clone(), next_data_epoch, tombstones, manifest)
}

/// Couple the SC-23 tombstone with ER-29 target propagation. The tombstone is planned first and
/// every derived target starts Unknown; no projection, artifact or cache is considered deleted
/// merely because a receipt was redacted.
pub fn plan_deletion_propagation(
    request: &DeleteRequest,
    scan: &RetentionScan,
    snapshot: &DataGovernanceSnapshot,
    tombstone_digest: &str,
    observed_at_ms: u64,
) -> Result<(DeletionPlan, DataPropagationPlan), String> {
    let deletion = plan_deletion(request, scan)?;
    snapshot.validate()?;
    if snapshot.project_ref != request.project_ref
        || snapshot.policy_revision != request.policy_revision
        || snapshot.data_epoch != request.data_epoch
        || snapshot.source_cursor != request.source_cursor
    {
        return Err("deletion_governance_snapshot_stale".to_owned());
    }
    let mut next_snapshot = snapshot.clone();
    next_snapshot.data_epoch = deletion.next_data_epoch;
    next_snapshot.snapshot_digest = next_snapshot.digest();
    next_snapshot.validate()?;
    let propagation = DataPropagationPlan::from_snapshot(
        &next_snapshot,
        request.data_epoch,
        "delete",
        tombstone_digest,
        observed_at_ms,
    )?;
    Ok((deletion, propagation))
}

/// Payload redaction changes presentation only. Authorization still requires a current epoch and
/// an Available payload state, so a redacted receipt cannot be used to bypass revocation.
pub fn receipt_redaction_is_not_authorization(
    binding: &ReceiptDataBinding,
    current_data_epoch: u64,
) -> Result<(), String> {
    binding.authorize_payload(current_data_epoch)
}
