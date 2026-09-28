//! DEP-26: connect retention, deletion and revocation to the audit, backup, artifact and memory
//! lifecycles they are supposed to protect.
//!
//! Four things go wrong in practice, and the card names all four:
//!
//! - something is deleted while a **legal hold** covers it;
//! - a derived layer is deleted while the **facts** it came from are not, so the tombstone goes and
//!   the payload it described is still served;
//! - an **artifact or backup reference dangles**, because the thing pointing at it was retained;
//! - a **revocation does not propagate**, so a layer that still answers with revoked data is
//!   treated as though it had acknowledged.
//!
//! This module plans. It deletes nothing, and the distinction is the point: a plan is a decision
//! that can be reviewed, refused and re-derived, and a deletion is an irreversible act. The
//! vocabulary is reused rather than forked — `RevocationLayer` and `RevocationLayerState` from
//! PD-26, `BackupLegalHold` and `DeletionMode` from the DEP-21 backup lifecycle — because a second
//! layer enum would be a second answer to "which layers are downstream of the facts".
//!
//! # The ordering that is not negotiable
//!
//! ```text
//! Facts → Artifact → Memory → Index → Cache → Checkpoint
//! ```
//!
//! Deleting downstream before upstream is how "只删 projection 不留事实" happens: the tombstone is
//! the fact, and if the fact goes first the layer that was supposed to stop serving the payload
//! has nothing left to be tombstoned against. The plan therefore orders targets by that sequence
//! and refuses a plan whose facts layer is absent while a derived layer is present.

use kiana_domain::{
    json_digest, redact_text, scan_secret_sentinels, BackupLegalHold, DeletionMode, SchemaVersion,
    SecretScanChannel,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

// Reused, not forked: the layer vocabulary and its states come from PD-26, and the per-layer
// observation is the same adapter-reported record a propagation run produces.
use crate::revocation_propagation::{
    RevocationLayer, RevocationLayerObservation, RevocationLayerState,
};

pub const RETENTION_DELETION_REQUEST_SCHEMA: &str = "kiana.retention-deletion-request.v1";
pub const RETENTION_DELETION_PLAN_SCHEMA: &str = "kiana.retention-deletion-plan.v1";
pub const RETENTION_DELETION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_RETENTION_TEXT: usize = 256;
pub const MAX_RETENTION_TARGETS: usize = 64;

/// The order deletions must happen in. Upstream first, always.
const DELETION_ORDER: [RevocationLayer; 6] = RevocationLayer::ALL;

/// One object somebody wants gone, and the layer that holds it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeletionTarget {
    pub layer: RevocationLayer,
    pub object_id: String,
    pub object_digest: String,
}

impl DeletionTarget {
    pub fn new(
        layer: RevocationLayer,
        object_id: impl Into<String>,
        object_digest: impl Into<String>,
    ) -> Self {
        Self {
            layer,
            object_id: object_id.into(),
            object_digest: object_digest.into(),
        }
    }

    fn validate(&self) -> Result<(), String> {
        safe_text(&self.object_id, "retention_object_id")?;
        valid_digest(&self.object_digest, "retention_object_digest")
    }

    /// Position in the deletion order. Lower goes first.
    fn order(&self) -> usize {
        DELETION_ORDER
            .iter()
            .position(|layer| *layer == self.layer)
            .unwrap_or(usize::MAX)
    }
}

/// An object that will be retained, and what it points at. A retained object pointing at a deleted
/// one is a dangling reference, which is the fourth failure the card names.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetainedReference {
    pub retained_object_id: String,
    pub retained_layer: RevocationLayer,
    pub references_layer: RevocationLayer,
    pub references_object_id: String,
}

/// Why a target is not being deleted.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtectedTarget {
    pub layer: RevocationLayer,
    pub object_id: String,
    pub reason: String,
}

/// The request: what somebody wants deleted, and what the world currently says.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetentionDeletionRequest {
    pub schema: String,
    pub version: SchemaVersion,
    pub plan_id: String,
    pub requested_by: String,
    pub mode: DeletionMode,
    /// Everything under consideration, in any order.
    pub targets: Vec<DeletionTarget>,
    /// What the caller insists on deleting. A held or dangling entry here is a refusal, not a
    /// preference: the point of a plan is that the caller can be told no.
    pub demanded: Vec<DeletionTarget>,
    pub legal_holds: Vec<BackupLegalHold>,
    pub observations: Vec<RevocationLayerObservation>,
    pub retained_references: Vec<RetainedReference>,
    /// The data epoch the tombstone was issued at. A layer still on an older epoch has not seen it.
    pub tombstone_data_epoch: u64,
    pub request_digest: String,
}

impl RetentionDeletionRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        plan_id: impl Into<String>,
        requested_by: impl Into<String>,
        mode: DeletionMode,
        targets: Vec<DeletionTarget>,
        demanded: Vec<DeletionTarget>,
        legal_holds: Vec<BackupLegalHold>,
        observations: Vec<RevocationLayerObservation>,
        retained_references: Vec<RetainedReference>,
        tombstone_data_epoch: u64,
    ) -> Self {
        let mut value = Self {
            schema: RETENTION_DELETION_REQUEST_SCHEMA.to_owned(),
            version: RETENTION_DELETION_VERSION,
            plan_id: plan_id.into(),
            requested_by: requested_by.into(),
            mode,
            targets,
            demanded,
            legal_holds,
            observations,
            retained_references,
            tombstone_data_epoch,
            request_digest: String::new(),
        };
        value.request_digest = value.digest();
        value
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "plan_id": self.plan_id,
            "requested_by": self.requested_by,
            "mode": self.mode,
            "targets": self.targets,
            "demanded": self.demanded,
            "legal_holds": self.legal_holds.iter().map(|hold| hold.hold_digest.clone()).collect::<Vec<_>>(),
            "observations": self.observations.iter().map(|o| o.observation_digest.clone()).collect::<Vec<_>>(),
            "retained_references": self.retained_references,
            "tombstone_data_epoch": self.tombstone_data_epoch,
        }))
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema != RETENTION_DELETION_REQUEST_SCHEMA
            || !self.version.is_compatible_with(&RETENTION_DELETION_VERSION)
        {
            return Err("retention_deletion_header_invalid".to_owned());
        }
        if self.request_digest != self.digest() {
            return Err("retention_deletion_request_digest_mismatch".to_owned());
        }
        safe_text(&self.plan_id, "retention_plan_id")?;
        safe_text(&self.requested_by, "retention_requested_by")?;
        if self.tombstone_data_epoch == 0 {
            return Err("retention_tombstone_epoch_required".to_owned());
        }
        if self.targets.is_empty() || self.targets.len() > MAX_RETENTION_TARGETS {
            return Err("retention_deletion_targets_required".to_owned());
        }
        let mut seen: Vec<(RevocationLayer, String)> = Vec::new();
        for target in &self.targets {
            target.validate()?;
            let key = (target.layer, target.object_id.clone());
            if seen.contains(&key) {
                return Err("retention_deletion_target_duplicate".to_owned());
            }
            seen.push(key);
        }
        for target in &self.demanded {
            target.validate()?;
        }
        Ok(())
    }
}

/// The plan. What may be deleted, in what order, and why everything else may not.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetentionDeletionPlan {
    pub schema: String,
    pub version: SchemaVersion,
    pub plan_id: String,
    pub mode: DeletionMode,
    /// Always empty in `DryRun`. A dry run that names deletions is not a dry run.
    pub deletable: Vec<DeletionTarget>,
    pub protected: Vec<ProtectedTarget>,
    /// The order deletions must be executed in, upstream first.
    pub deletion_order: Vec<RevocationLayer>,
    /// True when the plan touches a derived layer, so a rebuild has to be verified afterwards.
    pub rebuild_required: bool,
    pub limitations: Vec<String>,
    pub plan_digest: String,
}

impl RetentionDeletionPlan {
    pub fn validate_against(&self, request: &RetentionDeletionRequest) -> Result<(), String> {
        request.validate()?;
        if self.schema != RETENTION_DELETION_PLAN_SCHEMA
            || !self.version.is_compatible_with(&RETENTION_DELETION_VERSION)
            || self.plan_id != request.plan_id
            || self.mode != request.mode
        {
            return Err("retention_deletion_plan_binding_invalid".to_owned());
        }
        if self.plan_digest != self.digest() {
            return Err("retention_deletion_plan_digest_mismatch".to_owned());
        }
        if self.limitations.is_empty() {
            return Err("retention_deletion_limitations_required".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "plan_id": self.plan_id,
            "mode": self.mode,
            "deletable": self.deletable,
            "protected": self.protected,
            "deletion_order": self.deletion_order,
            "rebuild_required": self.rebuild_required,
            "limitations": self.limitations,
        }))
    }
}

/// Produce the plan. This is the whole slice: a decision, reviewable and re-derivable.
pub fn plan_retention_deletion(
    request: &RetentionDeletionRequest,
) -> Result<RetentionDeletionPlan, String> {
    request.validate()?;

    // Legal hold first. It is the only reason here that comes from outside the system, and a plan
    // that quietly reinterprets it is not a plan.
    for target in &request.demanded {
        if covered_by_hold(&request.legal_holds, &target.object_id) {
            return Err("retention_deletion_legal_hold".to_owned());
        }
    }

    // Propagation second: a layer that has not acknowledged the tombstone cannot have its object
    // deleted, whatever anybody asked for.
    for target in &request.targets {
        let Some(observation) = request
            .observations
            .iter()
            .find(|observation| observation.layer == target.layer)
        else {
            return Err("retention_revocation_missing".to_owned());
        };
        if observation.state != RevocationLayerState::Tombstoned {
            return Err("retention_revocation_incomplete".to_owned());
        }
        if observation.data_epoch < request.tombstone_data_epoch {
            return Err("retention_revocation_epoch_stale".to_owned());
        }
        if observation.receipt_digest.is_none() {
            return Err("retention_revocation_receipt_missing".to_owned());
        }
    }

    // Facts first. Deleting a derived layer while the facts layer stays is the "只删 projection
    // 不留事实" failure, and it is refused rather than reordered.
    let facts_targeted = request
        .targets
        .iter()
        .any(|target| target.layer == RevocationLayer::Facts);
    if !facts_targeted
        && request.targets.iter().any(|target| {
            target.layer != RevocationLayer::Facts
        })
    {
        return Err("retention_deletion_facts_missing".to_owned());
    }

    // A retained object must not be left pointing at something that no longer exists.
    for target in &request.demanded {
        let dangling = request.retained_references.iter().any(|reference| {
            reference.references_layer == target.layer
                && reference.references_object_id == target.object_id
        });
        if dangling {
            return Err("retention_deletion_dangling_reference".to_owned());
        }
    }

    let mut deletable: Vec<DeletionTarget> = request
        .targets
        .iter()
        .filter(|target| !covered_by_hold(&request.legal_holds, &target.object_id))
        .cloned()
        .collect();
    let mut protected: Vec<ProtectedTarget> = request
        .targets
        .iter()
        .filter(|target| covered_by_hold(&request.legal_holds, &target.object_id))
        .map(|target| ProtectedTarget {
            layer: target.layer,
            object_id: target.object_id.clone(),
            reason: "legal_hold".to_owned(),
        })
        .collect();

    // A dry run that names deletions is not a dry run. The plan says what it decided and then does
    // not do it, so the deletable set has to be empty for it to mean anything.
    if request.mode == DeletionMode::DryRun {
        for target in &deletable {
            protected.push(ProtectedTarget {
                layer: target.layer,
                object_id: target.object_id.clone(),
                reason: "dry_run".to_owned(),
            });
        }
        deletable.clear();
    } else {
        deletable.sort_by_key(|target| target.order());
    }

    let mut order: Vec<RevocationLayer> = deletable
        .iter()
        .map(|target| target.layer)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    order.sort_by_key(|layer| {
        DELETION_ORDER
            .iter()
            .position(|candidate| candidate == layer)
            .unwrap_or(usize::MAX)
    });

    let rebuild_required = deletable
        .iter()
        .any(|target| target.layer != RevocationLayer::Facts);

    let mut plan = RetentionDeletionPlan {
        schema: RETENTION_DELETION_PLAN_SCHEMA.to_owned(),
        version: RETENTION_DELETION_VERSION,
        plan_id: request.plan_id.clone(),
        mode: request.mode,
        deletable,
        protected,
        deletion_order: order,
        rebuild_required,
        limitations: vec![
            "no object was deleted; this is a plan".to_owned(),
            "hold and propagation state are supplied by the caller, not read here".to_owned(),
        ],
        plan_digest: String::new(),
    };
    plan.plan_digest = plan.digest();
    plan.validate_against(request)?;
    Ok(plan)
}

fn covered_by_hold(holds: &[BackupLegalHold], object_id: &str) -> bool {
    holds
        .iter()
        .any(|hold| hold.backup_ids.iter().any(|id| id == object_id))
}

fn safe_text(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_RETENTION_TEXT
        || value.contains(['\0', '\r', '\n'])
    {
        return Err(format!("{field}_invalid"));
    }
    if redact_text(value) != value {
        return Err(format!("{field}_not_redacted"));
    }
    scan_secret_sentinels(SecretScanChannel::Event, value)
        .map_err(|_| format!("{field}_secret_detected"))
}

fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
