//! Restore quarantine: admitting a restored root without ever overwriting the active one.
//!
//! A restore must not write where the instance is running. This module decides whether a restored
//! root may be *opened* in quarantine: a separate root that is scanned, rebuilt and verified
//! before anyone considers activating it. It never copies a file, creates a directory, acquires a
//! lease or publishes a root. The activation decision itself belongs to DEP-23.

use crate::{
    json_digest, redact_text, scan_secret_sentinels, InstanceId, SchemaVersion, SecretScanChannel,
    StorageRootId,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const RESTORE_QUARANTINE_SCHEMA: &str = "kiana.restore-quarantine.v1";
pub const RESTORE_ROOT_SCHEMA: &str = "kiana.restore-root.v1";
pub const RESTORE_SCAN_REPORT_SCHEMA: &str = "kiana.restore-scan-report.v1";
pub const RESTORE_QUARANTINE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_RESTORE_ROOTS: usize = 4;
pub const MAX_RESTORE_ARTIFACT_REFS: usize = 4_096;

/// Why a restored root is being opened.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestoreMode {
    /// Rebuild every projector and index from the restored facts.
    FullRebuild,
    /// Continue from the projection checkpoints in the backup.
    ResumeProjections,
}

impl RestoreMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FullRebuild => "full_rebuild",
            Self::ResumeProjections => "resume_projections",
        }
    }
}

/// The stage a quarantined root has reached.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuarantineStage {
    /// Copied in but not yet scanned. Nothing may read it.
    Staged,
    /// Manifest, hashes and schema verified.
    Verified,
    /// Projectors and indexes rebuilt from the verified facts.
    Rebuilt,
    /// Scanned and rebuilt; eligible for an activation decision.
    Ready,
    /// The scan found something disqualifying. Terminal.
    Rejected,
}

impl QuarantineStage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Staged => "staged",
            Self::Verified => "verified",
            Self::Rebuilt => "rebuilt",
            Self::Ready => "ready",
            Self::Rejected => "rejected",
        }
    }

    /// Only a fully scanned and rebuilt root may be considered for activation. A staged or
    /// half-verified root is not a restore, it is a directory.
    pub const fn is_activation_eligible(self) -> bool {
        matches!(self, Self::Ready)
    }
}

/// One restored root under quarantine.
///
/// A root is identified by its storage root id, never by a path, and it is always distinct from
/// the instance's active root. Overwriting the active root is the one thing a restore must never
/// do, so the two are compared structurally here rather than left to the adapter.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreRoot {
    pub schema: String,
    pub version: SchemaVersion,
    pub root_id: String,
    pub storage_root: StorageRootId,
    pub instance_id: InstanceId,
    /// The root the running instance is using. A restored root may never be this one.
    pub active_storage_root: StorageRootId,
    pub active_instance_id: InstanceId,
    /// Manifest digest the restore was performed from.
    pub manifest_digest: String,
    /// Cursor the backup covered.
    pub source_cursor: u64,
    /// Projector generation in the backup. It may never exceed the source cursor's generation.
    pub projection_generation: u64,
    /// Projector cursor found in the backup. A projector past the source is a fabricated position.
    pub projection_cursor: u64,
    pub data_epoch: u64,
    pub authority_epoch: u64,
    /// Data and authority epoch of the instance being replaced. A restore may only move forward.
    pub previous_data_epoch: Option<u64>,
    pub previous_authority_epoch: Option<u64>,
    /// Artifact references the restore expects to be present.
    pub artifact_refs: Vec<String>,
    /// Migration version the restored data is in. `None` when the backup carried no registry.
    pub migration_version: Option<u32>,
    /// Whether the migration registry was readable and applied.
    pub migration_applied: bool,
    pub mode: RestoreMode,
    pub stage: QuarantineStage,
    pub root_digest: String,
}

impl RestoreRoot {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        root_id: impl Into<String>,
        storage_root: StorageRootId,
        instance_id: InstanceId,
        active_storage_root: StorageRootId,
        active_instance_id: InstanceId,
        manifest_digest: impl Into<String>,
        source_cursor: u64,
        projection_generation: u64,
        projection_cursor: u64,
        data_epoch: u64,
        authority_epoch: u64,
        previous_data_epoch: Option<u64>,
        previous_authority_epoch: Option<u64>,
        artifact_refs: Vec<String>,
        migration_version: Option<u32>,
        migration_applied: bool,
        mode: RestoreMode,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: RESTORE_ROOT_SCHEMA.to_owned(),
            version: RESTORE_QUARANTINE_VERSION,
            root_id: root_id.into(),
            storage_root,
            instance_id,
            active_storage_root,
            active_instance_id,
            manifest_digest: manifest_digest.into(),
            source_cursor,
            projection_generation,
            projection_cursor,
            data_epoch,
            authority_epoch,
            previous_data_epoch,
            previous_authority_epoch,
            artifact_refs,
            migration_version,
            migration_applied,
            mode,
            stage: QuarantineStage::Staged,
            root_digest: String::new(),
        };
        value.root_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RESTORE_ROOT_SCHEMA
            || self.version != RESTORE_QUARANTINE_VERSION
            || self.source_cursor == 0
            || self.projection_generation == 0
            || self.data_epoch == 0
            || self.authority_epoch == 0
            || self.storage_root.as_uuid().is_nil()
            || self.instance_id.as_uuid().is_nil()
        {
            return Err("restore_root_header_invalid".to_owned());
        }
        safe_text(&self.root_id, "restore_root_id")?;
        valid_digest(&self.manifest_digest, "restore_root_manifest_digest")?;
        // Restoring over the running root would destroy the only copy of the live data. This is
        // refused structurally: a root that claims to be the active one can never be admitted.
        if self.storage_root == self.active_storage_root
            || self.instance_id == self.active_instance_id
        {
            return Err("restore_root_would_overwrite_active".to_owned());
        }
        // A projector that consumed more than the store holds has regressed or fabricated a
        // position, so the backup's projection boundary cannot be trusted.
        if self.projection_cursor > self.source_cursor {
            return Err("restore_projection_cursor_ahead_of_source".to_owned());
        }
        // Epochs may only move forward. A restore that rewinds them would let a write made under
        // the old epoch be accepted again.
        if let Some(previous) = self.previous_data_epoch {
            if self.data_epoch <= previous {
                return Err("restore_data_epoch_not_advanced".to_owned());
            }
        }
        if let Some(previous) = self.previous_authority_epoch {
            if self.authority_epoch <= previous {
                return Err("restore_authority_epoch_not_advanced".to_owned());
            }
        }
        if self.migration_version.is_some() && !self.migration_applied {
            return Err("restore_migration_not_applied".to_owned());
        }
        if self.migration_applied && self.migration_version.is_none() {
            return Err("restore_migration_version_missing".to_owned());
        }
        valid_digest(&self.root_digest, "restore_root_digest")?;
        if self.root_digest != self.digest() {
            return Err("restore_root_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Resuming projections is only coherent when the backup carried a checkpoint. A full rebuild
    /// needs none, so the two modes are not interchangeable.
    pub fn mode_is_coherent(&self) -> Result<(), String> {
        if self.mode == RestoreMode::ResumeProjections && self.projection_cursor == 0 {
            return Err("restore_resume_without_projection_cursor".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "root_id": self.root_id,
            "storage_root": self.storage_root,
            "instance_id": self.instance_id,
            "active_storage_root": self.active_storage_root,
            "active_instance_id": self.active_instance_id,
            "manifest_digest": self.manifest_digest,
            "source_cursor": self.source_cursor,
            "projection_generation": self.projection_generation,
            "projection_cursor": self.projection_cursor,
            "data_epoch": self.data_epoch,
            "authority_epoch": self.authority_epoch,
            "previous_data_epoch": self.previous_data_epoch,
            "previous_authority_epoch": self.previous_authority_epoch,
            "artifact_refs": self.artifact_refs,
            "migration_version": self.migration_version,
            "migration_applied": self.migration_applied,
            "mode": self.mode,
            "stage": self.stage,
        }))
    }
}

/// What the scan of a quarantined root found.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestoreScanStatus {
    /// Everything the backup promised is present and consistent.
    Complete,
    /// A referenced artifact is absent.
    MissingArtifact,
    /// A hash or digest did not match.
    HashMismatch,
    /// The schema is one this build cannot read.
    UnknownSchema,
    /// A migration was required and not applied.
    MigrationPending,
    /// The scan could not complete.
    Unknown,
}

impl RestoreScanStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::MissingArtifact => "missing_artifact",
            Self::HashMismatch => "hash_mismatch",
            Self::UnknownSchema => "unknown_schema",
            Self::MigrationPending => "migration_pending",
            Self::Unknown => "unknown",
        }
    }

    pub const fn is_disqualifying(self) -> bool {
        !matches!(self, Self::Complete)
    }
}

/// The ordered scan result for one quarantined root.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreScanReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub root_id: String,
    pub status: RestoreScanStatus,
    /// Artifacts the root expected but did not contain.
    pub missing_artifacts: Vec<String>,
    pub stage: QuarantineStage,
    pub reason: String,
    pub remediation: String,
    pub root_digest: String,
    pub report_digest: String,
}

impl RestoreScanReport {
    /// Decide the scan outcome from the facts the adapter reported.
    ///
    /// `present_artifacts` is what the root actually contained. Comparing it against the
    /// inventory the manifest promised is what makes a missing artifact detectable here,
    /// without the domain ever reading a filesystem.
    ///
    /// Order is fixed so the same facts always give the same reason: a missing artifact before a
    /// hash mismatch, because the hash of a chunk that is not there means nothing, and an
    /// unknown schema before a pending migration, because a schema this build cannot read has no
    /// migration path to speak of.
    pub fn scan(root: &RestoreRoot, present_artifacts: &[String]) -> Result<Self, String> {
        root.validate()?;
        root.mode_is_coherent()?;
        for present in present_artifacts {
            safe_text(present, "restore_present_artifact")?;
        }

        let missing: Vec<String> = root
            .artifact_refs
            .iter()
            .filter(|expected| !present_artifacts.contains(expected))
            .cloned()
            .collect();

        let (status, reason, remediation) = if !missing.is_empty() {
            return Ok(Self::rejected(
                root,
                RestoreScanStatus::MissingArtifact,
                missing,
                "restore_artifact_missing",
                "restore the missing artifact from the backup, or take a newer one",
            ));
        } else if root.artifact_refs.len() > MAX_RESTORE_ARTIFACT_REFS {
            (
                RestoreScanStatus::Unknown,
                "restore_artifact_inventory_unbounded",
                "re-read the artifact inventory from the backup manifest",
            )
        } else if root.migration_version.is_some() && !root.migration_applied {
            (
                RestoreScanStatus::MigrationPending,
                "restore_migration_not_applied",
                "run the ordered migration registry against the quarantined root",
            )
        } else {
            (RestoreScanStatus::Complete, "restore_scan_complete", "none")
        };

        let stage = if status == RestoreScanStatus::Complete {
            QuarantineStage::Verified
        } else {
            QuarantineStage::Rejected
        };
        let mut report = Self {
            schema: RESTORE_SCAN_REPORT_SCHEMA.to_owned(),
            version: RESTORE_QUARANTINE_VERSION,
            root_id: root.root_id.clone(),
            status,
            missing_artifacts: Vec::new(),
            stage,
            reason: reason.to_owned(),
            remediation: remediation.to_owned(),
            root_digest: root.root_digest.clone(),
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate_against(root)?;
        Ok(report)
    }

    fn rejected(
        root: &RestoreRoot,
        status: RestoreScanStatus,
        missing_artifacts: Vec<String>,
        reason: &'static str,
        remediation: &'static str,
    ) -> Self {
        let mut report = Self {
            schema: RESTORE_SCAN_REPORT_SCHEMA.to_owned(),
            version: RESTORE_QUARANTINE_VERSION,
            root_id: root.root_id.clone(),
            status,
            missing_artifacts,
            stage: QuarantineStage::Rejected,
            reason: reason.to_owned(),
            remediation: remediation.to_owned(),
            root_digest: root.root_digest.clone(),
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report
    }

    /// Whether this root may now be considered for activation. A rejected or incomplete scan
    /// never is, and the root digest must still match the root it was scanned from.
    pub fn activation_eligible(&self, root: &RestoreRoot) -> Result<bool, String> {
        root.validate()?;
        self.validate_against(root)?;
        Ok(self.status == RestoreScanStatus::Complete
            && self.stage == QuarantineStage::Ready
            && root.stage == QuarantineStage::Ready)
    }

    pub fn validate_against(&self, root: &RestoreRoot) -> Result<(), String> {
        root.validate()?;
        if self.schema != RESTORE_SCAN_REPORT_SCHEMA
            || self.version != RESTORE_QUARANTINE_VERSION
            || self.root_id != root.root_id
            || self.root_digest != root.root_digest
        {
            return Err("restore_scan_report_binding_invalid".to_owned());
        }
        if self.status.is_disqualifying() && self.stage == QuarantineStage::Ready {
            return Err("restore_scan_report_rejected_but_ready".to_owned());
        }
        valid_digest(&self.report_digest, "restore_scan_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("restore_scan_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "root_id": self.root_id,
            "status": self.status,
            "missing_artifacts": self.missing_artifacts,
            "stage": self.stage,
            "reason": self.reason,
            "remediation": self.remediation,
            "root_digest": self.root_digest,
        }))
    }
}

/// The quarantine holding one instance's restored roots.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreQuarantine {
    pub schema: String,
    pub version: SchemaVersion,
    pub active_storage_root: StorageRootId,
    pub active_instance_id: InstanceId,
    pub roots: Vec<RestoreRoot>,
    pub quarantine_digest: String,
}

impl RestoreQuarantine {
    pub fn new(
        active_storage_root: StorageRootId,
        active_instance_id: InstanceId,
        roots: Vec<RestoreRoot>,
    ) -> Result<Self, String> {
        if roots.is_empty() || roots.len() > MAX_RESTORE_ROOTS {
            return Err("restore_quarantine_root_count_invalid".to_owned());
        }
        let mut value = Self {
            schema: RESTORE_QUARANTINE_SCHEMA.to_owned(),
            version: RESTORE_QUARANTINE_VERSION,
            active_storage_root,
            active_instance_id,
            roots,
            quarantine_digest: String::new(),
        };
        value.quarantine_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RESTORE_QUARANTINE_SCHEMA || self.version != RESTORE_QUARANTINE_VERSION {
            return Err("restore_quarantine_header_invalid".to_owned());
        }
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for root in &self.roots {
            root.validate()?;
            if !seen.insert(root.root_id.as_str()) {
                return Err("restore_quarantine_duplicate_root".to_owned());
            }
            // The quarantine's own idea of the active root must match what each root recorded, or
            // a root could be admitted against one active root and activated against another.
            if root.active_storage_root != self.active_storage_root
                || root.active_instance_id != self.active_instance_id
            {
                return Err("restore_quarantine_active_root_mismatch".to_owned());
            }
            if root.storage_root == self.active_storage_root {
                return Err("restore_root_would_overwrite_active".to_owned());
            }
        }
        valid_digest(&self.quarantine_digest, "restore_quarantine_digest")?;
        if self.quarantine_digest != self.digest() {
            return Err("restore_quarantine_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Advance a root to a later stage. Stages only move forward, so a scan cannot be undone by
    /// replaying an older decision.
    pub fn advance(&self, root_id: &str, to: QuarantineStage) -> Result<Self, String> {
        self.validate()?;
        let Some(root) = self.roots.iter().find(|root| root.root_id == root_id) else {
            return Err("restore_quarantine_root_missing".to_owned());
        };
        if to <= root.stage {
            return Err("restore_quarantine_stage_not_advancing".to_owned());
        }
        if root.stage == QuarantineStage::Rejected {
            return Err("restore_quarantine_root_rejected".to_owned());
        }
        let mut roots = self.roots.clone();
        let target = roots
            .iter_mut()
            .find(|candidate| candidate.root_id == root_id)
            .ok_or("restore_quarantine_root_missing")?;
        target.stage = to;
        target.root_digest = target.digest();
        Self::new(self.active_storage_root, self.active_instance_id, roots)
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "active_storage_root": self.active_storage_root,
            "active_instance_id": self.active_instance_id,
            "roots": self.roots,
        }))
    }
}

fn safe_text(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > 256
        || value.contains(['\0', '\r', '\n'])
        || value.contains("..")
        || value.contains("://")
    {
        return Err(format!("{field}_invalid"));
    }
    if redact_text(value) != value {
        return Err(format!("{field}_not_redacted"));
    }
    scan_secret_sentinels(SecretScanChannel::Receipt, value)
        .map_err(|_| format!("{field}_secret_detected"))
}

fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
