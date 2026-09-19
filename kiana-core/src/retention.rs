//! Pure retention evaluation over a committed data-governance projection.
//!
//! The evaluator never deletes data or changes the EventLog. It refuses policy/projection epoch
//! drift, emits a bounded source-cursor-bound scan, and creates a receipt for every active legal
//! hold so later adapters cannot silently treat a held object as purgeable.

use kiana_domain::{
    DataGovernanceSnapshot, DataPayloadState, LegalHold, LegalHoldReceipt, RetentionDecision,
    RetentionDisposition, RetentionPolicy, RetentionScan,
};
use std::collections::BTreeSet;

/// Evaluate one finite retention policy against a server-derived governance snapshot.
pub fn scan_retention(
    policy: &RetentionPolicy,
    holds: &[LegalHold],
    snapshot: &DataGovernanceSnapshot,
    projection_cursor: u64,
    now_ms: u64,
) -> Result<RetentionScan, String> {
    policy.validate()?;
    snapshot.validate()?;
    if snapshot.project_ref != policy.project_ref {
        return Err("retention_project_boundary_mismatch".to_owned());
    }
    if snapshot.policy_revision != policy.revision {
        return Err("retention_policy_revision_stale".to_owned());
    }
    if snapshot.data_epoch != policy.data_epoch {
        return Err("retention_data_epoch_stale".to_owned());
    }
    if projection_cursor > snapshot.source_cursor {
        return Err("retention_projection_cursor_ahead".to_owned());
    }

    let mut hold_ids = BTreeSet::new();
    for hold in holds {
        hold.validate()?;
        if hold.project_ref != policy.project_ref {
            return Err("retention_legal_hold_project_mismatch".to_owned());
        }
        if hold.policy_revision != policy.revision {
            return Err("retention_legal_hold_revision_stale".to_owned());
        }
        if !hold_ids.insert(hold.hold_id.clone()) {
            return Err("retention_legal_hold_duplicate".to_owned());
        }
    }

    let active_holds = holds.iter().filter(|hold| hold.active).collect::<Vec<_>>();
    let hold_receipts = active_holds
        .iter()
        .map(|hold| {
            LegalHoldReceipt::new(
                hold,
                snapshot.source_cursor,
                projection_cursor,
                snapshot.source_event_ids.clone(),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    let decisions = snapshot
        .observations
        .iter()
        .map(|observation| {
            let hold_id = active_holds
                .iter()
                .find(|hold| hold.covers(&policy.project_ref, &observation.source_ref))
                .map(|hold| hold.hold_id.clone());
            let disposition = if hold_id.is_some() {
                RetentionDisposition::Held
            } else if observation.payload == DataPayloadState::Unknown {
                RetentionDisposition::Unknown
            } else if now_ms >= policy.retain_until_ms(&observation.source_ref) {
                RetentionDisposition::Eligible
            } else {
                RetentionDisposition::Retain
            };
            RetentionDecision {
                object_ref: observation.source_ref.clone(),
                class: observation.class,
                purpose_id: observation.purpose_id.clone(),
                source_digest: observation.source_digest.clone(),
                payload: observation.payload,
                disposition,
                retain_until_ms: policy.retain_until_ms(&observation.source_ref),
                hold_id,
            }
        })
        .collect();

    RetentionScan::new(
        policy.project_ref.clone(),
        policy.revision,
        policy.data_epoch,
        snapshot.source_cursor,
        projection_cursor,
        snapshot.source_event_ids.clone(),
        decisions,
        hold_receipts,
    )
}
