//! Deny-first deletion planning over the SC-22 retention boundary.
//!
//! This module only prepares an appendable tombstone batch and an explicit propagation manifest.
//! It does not erase bytes, mutate projections or call an adapter. Every target starts as
//! `Unknown` until its own receipt is supplied by a later, authorized operation.

use kiana_domain::{
    DeleteRequest, DeletionManifest, DeletionPlan, DeletionPropagation, DeletionTombstone,
    RetentionDisposition, RetentionScan,
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
