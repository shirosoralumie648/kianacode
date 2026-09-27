//! DEP-23 restore activation: a new lease and fence, the previous root read-only, and a
//! readiness gate that keeps new commands out until activation actually happened.
//!
//! DEP-22 stops at "eligible for activation". This is the step that consumes that eligibility.
//! Activation is never a side effect of a restore finishing: it is an explicit operator decision
//! that needs a `Ready` root, a complete scan, no command admitted before readiness, and a
//! superseded instance that has actually been fenced. Activation mints a **new** lease and a
//! **new** fence token -- the old token is never reissued -- and it leaves the replaced root in
//! place as read-only so it stays auditable. It is never deleted, and the old instance cannot
//! write to it again.
//!
//! This module is a source contract over adapter-reported facts. It copies no file, writes no
//! byte, takes no OS lock, appends no event and dispatches no capability. The lease minted here
//! is a value a later step must persist and revalidate at the effect boundary, exactly as
//! `OperationLeaseCas` documents for itself.

use kiana_domain::{
    json_digest, redact_text, scan_secret_sentinels, FenceTokenId, InstanceId, OperationId,
    OperationLease, OperationLeaseCas, OperationLeaseState, QuarantineStage, RequestId,
    RestoreRoot, RestoreScanReport, RestoreScanStatus, SchemaVersion, SecretScanChannel,
    StorageLockId, StorageRootId, MAX_OPERATION_LEASE_TTL_MS,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const RESTORE_ACTIVATION_REQUEST_SCHEMA: &str = "kiana.restore-activation-request.v1";
pub const RESTORE_ACTIVATION_REPORT_SCHEMA: &str = "kiana.restore-activation-report.v1";
pub const RESTORE_ACTIVATION_RECORD_SCHEMA: &str = "kiana.restore-activation-record.v1";
pub const RESTORE_ACTIVATION_LEDGER_SCHEMA: &str = "kiana.restore-activation-ledger.v1";
pub const RESTORE_ACTIVATION_STATE_SCHEMA: &str = "kiana.restore-activation-state.v1";
pub const RESTORE_ACTIVATION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_RESTORE_ACTIVATION_TEXT: usize = 256;
pub const MAX_RESTORE_ACTIVATION_COMMAND_REFS: usize = 64;
pub const MAX_RESTORE_ACTIVATION_RECORDS: usize = 32;
pub const MAX_RESTORE_ACTIVATION_AUDIT_REFS: usize = 64;

/// How a root may be used. There is no third state on purpose: a root that is neither writable
/// nor read-only has not been decided, and an undecided root must not be reachable.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RootWriteMode {
    ReadWrite,
    ReadOnly,
}

impl RootWriteMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReadWrite => "read_write",
            Self::ReadOnly => "read_only",
        }
    }
}

/// Whether the restored root has been activated yet.
///
/// `Prepared` is the quarantine's end state carried forward: verified, rebuilt, and still not
/// serving. Only `Activated` opens the readiness gate.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivationStage {
    Prepared,
    Activated,
}

impl ActivationStage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Activated => "activated",
        }
    }
}

/// The instance and root being replaced, as the adapter observed them before activation.
///
/// This is the evidence that the old writer is actually fenced. A boolean somebody set is not
/// proof, so the lease state is checked structurally as well: an `Active` lease means the old
/// writer may still be writing, no matter what the flag says.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupersededRootWriter {
    pub instance_id: InstanceId,
    pub storage_root: StorageRootId,
    /// Fence token the replaced writer was last seen holding.
    pub fence_token: FenceTokenId,
    pub lease: OperationLease,
    /// The mode the replaced root is currently in, as observed.
    pub write_mode: RootWriteMode,
    /// Whether the replaced root is being kept rather than destroyed.
    pub retained: bool,
    /// Whether the authority confirmed the old fence token now refuses writes.
    pub writer_fenced: bool,
}

impl SupersededRootWriter {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instance_id: InstanceId,
        storage_root: StorageRootId,
        fence_token: FenceTokenId,
        lease: OperationLease,
        write_mode: RootWriteMode,
        retained: bool,
        writer_fenced: bool,
    ) -> Result<Self, String> {
        let value = Self {
            instance_id,
            storage_root,
            fence_token,
            lease,
            write_mode,
            retained,
            writer_fenced,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.instance_id.as_uuid().is_nil()
            || self.storage_root.as_uuid().is_nil()
            || self.fence_token.as_uuid().is_nil()
        {
            return Err("superseded_writer_header_invalid".to_owned());
        }
        self.lease.validate()?;
        // The observation and the lease must describe the same writer. A fence token quoted from
        // one lease and an identity quoted from another is not evidence about anything.
        if self.lease.owner_instance_id != self.instance_id
            || self.lease.storage_root != self.storage_root
            || self.lease.fence_token != self.fence_token
        {
            return Err("superseded_writer_lease_binding_invalid".to_owned());
        }
        Ok(())
    }

    /// Whether the replaced writer has lost its lease. Derived, never asserted.
    pub fn lease_is_terminal(&self) -> bool {
        self.lease.state != OperationLeaseState::Active
    }
}

/// One explicit request to activate a quarantined root.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreActivationRequest {
    pub schema: String,
    pub version: SchemaVersion,
    /// Stable identity of this activation. Two records under one identity must agree.
    pub activation_id: RequestId,
    pub actor_id: String,
    pub project_ref: String,
    /// The quarantined root to activate. Must already be `Ready`.
    pub restored_root: RestoreRoot,
    /// The scan of that root. Must be complete.
    pub scan_report: RestoreScanReport,
    /// The instance and root being replaced.
    pub superseded: SupersededRootWriter,
    pub new_instance_id: InstanceId,
    pub new_operation_id: OperationId,
    /// The fence token this activation mints. It may never equal the superseded one.
    pub new_fence_token: FenceTokenId,
    pub new_authority_epoch: u64,
    pub new_data_epoch: u64,
    pub issued_at_unix_ms: u64,
    pub lease_ttl_ms: u64,
    /// Whether the caller asked to activate. Activation is never implicit.
    pub explicit_activate: bool,
    /// Commands already admitted against the restored root while it was not ready.
    pub pending_command_count: u32,
    /// The evidence for those commands, one ref each.
    pub pending_command_refs: Vec<String>,
    pub request_digest: String,
}

impl RestoreActivationRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        activation_id: RequestId,
        actor_id: impl Into<String>,
        project_ref: impl Into<String>,
        restored_root: RestoreRoot,
        scan_report: RestoreScanReport,
        superseded: SupersededRootWriter,
        new_instance_id: InstanceId,
        new_operation_id: OperationId,
        new_fence_token: FenceTokenId,
        new_authority_epoch: u64,
        new_data_epoch: u64,
        issued_at_unix_ms: u64,
        lease_ttl_ms: u64,
        explicit_activate: bool,
        pending_command_count: u32,
        pending_command_refs: Vec<String>,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: RESTORE_ACTIVATION_REQUEST_SCHEMA.to_owned(),
            version: RESTORE_ACTIVATION_VERSION,
            activation_id,
            actor_id: actor_id.into(),
            project_ref: project_ref.into(),
            restored_root,
            scan_report,
            superseded,
            new_instance_id,
            new_operation_id,
            new_fence_token,
            new_authority_epoch,
            new_data_epoch,
            issued_at_unix_ms,
            lease_ttl_ms,
            explicit_activate,
            pending_command_count,
            pending_command_refs,
            request_digest: String::new(),
        };
        value.request_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RESTORE_ACTIVATION_REQUEST_SCHEMA
            || !self.version.is_compatible_with(&RESTORE_ACTIVATION_VERSION)
            || self.activation_id.as_uuid().is_nil()
            || self.new_instance_id.as_uuid().is_nil()
            || self.new_operation_id.as_uuid().is_nil()
            || self.new_fence_token.as_uuid().is_nil()
            || self.new_authority_epoch == 0
            || self.new_data_epoch == 0
            || self.issued_at_unix_ms == 0
            || self.lease_ttl_ms == 0
            || self.lease_ttl_ms > MAX_OPERATION_LEASE_TTL_MS
        {
            return Err("restore_activation_request_header_invalid".to_owned());
        }
        safe_text(&self.actor_id, "restore_activation_actor")?;
        safe_text(&self.project_ref, "restore_activation_project")?;
        self.superseded.validate()?;
        self.restored_root.validate()?;
        self.scan_report
            .validate_against(&self.restored_root)
            .map_err(|_| "restore_activation_scan_report_binding_invalid".to_owned())?;
        // The root was admitted against one active root; it may only be activated against that
        // same active root. Otherwise a root verified against root A could replace root B.
        if self.restored_root.active_storage_root != self.superseded.storage_root
            || self.restored_root.active_instance_id != self.superseded.instance_id
        {
            return Err("restore_activation_superseded_root_mismatch".to_owned());
        }
        if self.restored_root.storage_root == self.superseded.storage_root {
            return Err("restore_activation_would_overwrite_active".to_owned());
        }
        // The count may not float free of its evidence: an inflated count with no refs would be
        // a damage report that names nothing. The count is a u32 to match
        // `RestoreVerificationFact::pending_approval_count`; the widening is checked rather than
        // assumed, so a count that could not describe this many refs is refused too.
        if self.pending_command_refs.len() > MAX_RESTORE_ACTIVATION_COMMAND_REFS
            || usize::try_from(self.pending_command_count).ok()
                != Some(self.pending_command_refs.len())
        {
            return Err("restore_activation_command_refs_invalid".to_owned());
        }
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for reference in &self.pending_command_refs {
            safe_text(reference, "restore_activation_command_ref")?;
            if !seen.insert(reference.as_str()) {
                return Err("restore_activation_command_refs_duplicate".to_owned());
            }
        }
        valid_digest(&self.request_digest, "restore_activation_request_digest")?;
        if self.request_digest != self.digest() {
            return Err("restore_activation_request_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "activation_id": self.activation_id,
            "actor_id": self.actor_id,
            "project_ref": self.project_ref,
            "restored_root": self.restored_root,
            "scan_report": self.scan_report,
            "superseded": self.superseded,
            "new_instance_id": self.new_instance_id,
            "new_operation_id": self.new_operation_id,
            "new_fence_token": self.new_fence_token,
            "new_authority_epoch": self.new_authority_epoch,
            "new_data_epoch": self.new_data_epoch,
            "issued_at_unix_ms": self.issued_at_unix_ms,
            "lease_ttl_ms": self.lease_ttl_ms,
            "explicit_activate": self.explicit_activate,
            "pending_command_count": self.pending_command_count,
            "pending_command_refs": self.pending_command_refs,
        }))
    }
}

/// Whether an activation request may proceed.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestoreActivationStatus {
    /// Every precondition holds. A new lease and fence may be minted.
    Activated,
    /// At least one precondition fails. No lease is minted and no root changes role.
    Blocked,
}

impl RestoreActivationStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Activated => "activated",
            Self::Blocked => "blocked",
        }
    }
}

/// The ordered decision for one activation request.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreActivationReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub activation_id: RequestId,
    pub status: RestoreActivationStatus,
    pub activated_storage_root: StorageRootId,
    pub activated_instance_id: InstanceId,
    /// Fence token this activation would mint. Never the superseded one.
    pub fence_token: FenceTokenId,
    pub authority_epoch: u64,
    pub data_epoch: u64,
    /// Commands that had already been admitted before readiness.
    pub pending_command_count: u32,
    /// Stable denial code, empty on an activation decision.
    pub reason: String,
    pub remediation: String,
    pub request_digest: String,
    pub report_digest: String,
}

impl RestoreActivationReport {
    pub fn evaluate(request: &RestoreActivationRequest) -> Result<Self, String> {
        request.validate()?;
        let (status, reason, remediation) = derive(request);
        let mut report = Self {
            schema: RESTORE_ACTIVATION_REPORT_SCHEMA.to_owned(),
            version: RESTORE_ACTIVATION_VERSION,
            activation_id: request.activation_id,
            status,
            activated_storage_root: request.restored_root.storage_root,
            activated_instance_id: request.new_instance_id,
            fence_token: request.new_fence_token,
            authority_epoch: request.new_authority_epoch,
            data_epoch: request.new_data_epoch,
            pending_command_count: request.pending_command_count,
            reason,
            remediation,
            request_digest: request.request_digest.clone(),
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate_against(request)?;
        Ok(report)
    }

    pub fn validate_against(&self, request: &RestoreActivationRequest) -> Result<(), String> {
        request.validate()?;
        let (status, reason, remediation) = derive(request);
        if self.schema != RESTORE_ACTIVATION_REPORT_SCHEMA
            || !self.version.is_compatible_with(&RESTORE_ACTIVATION_VERSION)
            || self.activation_id != request.activation_id
            || self.status != status
            || self.activated_storage_root != request.restored_root.storage_root
            || self.activated_instance_id != request.new_instance_id
            || self.fence_token != request.new_fence_token
            || self.authority_epoch != request.new_authority_epoch
            || self.data_epoch != request.new_data_epoch
            || self.pending_command_count != request.pending_command_count
            || self.reason != reason
            || self.remediation != remediation
            || self.request_digest != request.request_digest
        {
            return Err("restore_activation_report_binding_invalid".to_owned());
        }
        // An activation decision names no reason; a blocked one always names one. Without this a
        // report could present itself as a plain refusal and hide which rule it hit.
        if (self.status == RestoreActivationStatus::Activated) != self.reason.is_empty() {
            return Err("restore_activation_report_reason_incoherent".to_owned());
        }
        if self.reason.len() > MAX_RESTORE_ACTIVATION_TEXT
            || self.remediation.len() > MAX_RESTORE_ACTIVATION_TEXT
        {
            return Err("restore_activation_report_text_too_long".to_owned());
        }
        valid_digest(&self.report_digest, "restore_activation_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("restore_activation_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Whether a new lease and fence may be minted for this request.
    pub fn may_activate(&self) -> bool {
        self.status == RestoreActivationStatus::Activated
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "activation_id": self.activation_id,
            "status": self.status,
            "activated_storage_root": self.activated_storage_root,
            "activated_instance_id": self.activated_instance_id,
            "fence_token": self.fence_token,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "pending_command_count": self.pending_command_count,
            "reason": self.reason,
            "remediation": self.remediation,
            "request_digest": self.request_digest,
        }))
    }
}

/// Decide in a fixed order, so the reported reason is the first violated rule and therefore the
/// same for the same facts.
///
/// The first four are *positive observations* about the instance being replaced -- a writer that
/// still holds an active lease, a fence the authority has not confirmed, a root that is still
/// writable, a root that is about to be destroyed. They outrank everything else because each one
/// is a live writer or a lost audit trail rather than an absence of proof. Then come the identity
/// and epoch invariants, which no amount of operator intent can waive. Finally the last three are
/// absences of proof, ordered by how cheap the caller's fix is: an actual command already
/// admitted, a missing explicit flag, an ineligible root.
fn derive(request: &RestoreActivationRequest) -> (RestoreActivationStatus, String, String) {
    if !request.superseded.lease_is_terminal() {
        return blocked(
            "restore_activation_old_writer_not_fenced",
            "let the replaced instance's lease expire or release it before activating",
        );
    }
    if !request.superseded.writer_fenced {
        return blocked(
            "restore_activation_old_writer_fence_unconfirmed",
            "confirm the authority refuses the replaced instance's fence token before activating",
        );
    }
    if request.superseded.write_mode != RootWriteMode::ReadOnly {
        return blocked(
            "restore_activation_old_root_not_read_only",
            "switch the replaced root to read-only; it is kept for audit, never deleted",
        );
    }
    if !request.superseded.retained {
        return blocked(
            "restore_activation_old_root_not_retained",
            "retain the replaced root so the pre-restore state stays auditable",
        );
    }
    if request.new_fence_token == request.superseded.fence_token {
        return blocked(
            "restore_activation_fence_token_reused",
            "mint a new fence token; the replaced writer's token must never be reissued",
        );
    }
    if request.new_instance_id == request.superseded.instance_id {
        return blocked(
            "restore_activation_instance_identity_reused",
            "activate under a new instance identity, not the replaced one",
        );
    }
    if request.new_authority_epoch <= request.superseded.lease.authority_epoch {
        return blocked(
            "restore_authority_epoch_not_advanced",
            "raise the authority epoch past the replaced writer's before activating",
        );
    }
    if request.new_data_epoch <= request.superseded.lease.data_epoch {
        return blocked(
            "restore_data_epoch_not_advanced",
            "raise the data epoch past the replaced writer's before activating",
        );
    }
    if request.pending_command_count > 0 {
        return blocked(
            "restore_activation_command_admitted_before_ready",
            "hold command intake until the activated root is ready, then reconcile what was admitted",
        );
    }
    if !request.explicit_activate {
        return blocked(
            "restore_activation_explicit_activate_required",
            "activation is an explicit decision; re-issue the request with explicit_activate set",
        );
    }
    if request.restored_root.stage != QuarantineStage::Ready
        || request.scan_report.status != RestoreScanStatus::Complete
    {
        return blocked(
            "restore_activation_root_not_eligible",
            "advance the quarantined root to ready with a complete scan before activating",
        );
    }
    (
        RestoreActivationStatus::Activated,
        String::new(),
        "none".to_owned(),
    )
}

fn blocked(
    reason: &'static str,
    remediation: &'static str,
) -> (RestoreActivationStatus, String, String) {
    (
        RestoreActivationStatus::Blocked,
        reason.to_owned(),
        remediation.to_owned(),
    )
}

/// The sealed outcome of one activation: a fresh lease under a fresh fence, and the replaced
/// root recorded as retained and read-only rather than deleted.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreActivationRecord {
    pub schema: String,
    pub version: SchemaVersion,
    pub activation_id: RequestId,
    pub stage: ActivationStage,
    pub activated_storage_root: StorageRootId,
    pub activated_instance_id: InstanceId,
    pub new_lease: OperationLease,
    pub new_lease_id: StorageLockId,
    pub new_fence_token: FenceTokenId,
    pub authority_epoch: u64,
    pub data_epoch: u64,
    /// The replaced root. It is never removed, so an audit can still read what was there.
    pub superseded_storage_root: StorageRootId,
    pub superseded_instance_id: InstanceId,
    pub superseded_fence_token: FenceTokenId,
    pub superseded_write_mode: RootWriteMode,
    pub superseded_retained: bool,
    /// Operator-settable audit references. Bounded, redacted, and never a path.
    pub audit_refs: Vec<String>,
    pub request_digest: String,
    pub report_digest: String,
    pub record_digest: String,
}

impl RestoreActivationRecord {
    /// Mint the record for an activation that has already been decided.
    ///
    /// The new lease is produced through `OperationLeaseCas`, so it carries the same shape, the
    /// same digests and the same revalidation rules as any other single-writer lease. This module
    /// does not persist it: a later step must, and must revalidate at the effect boundary.
    pub fn activate(
        request: &RestoreActivationRequest,
        report: &RestoreActivationReport,
        audit_refs: Vec<String>,
    ) -> Result<Self, String> {
        request.validate()?;
        report.validate_against(request)?;
        if !report.may_activate() {
            return Err("restore_activation_blocked".to_owned());
        }
        if audit_refs.len() > MAX_RESTORE_ACTIVATION_AUDIT_REFS {
            return Err("restore_activation_audit_refs_unbounded".to_owned());
        }
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for reference in &audit_refs {
            safe_text(reference, "restore_activation_audit_ref")?;
            if !seen.insert(reference.as_str()) {
                return Err("restore_activation_audit_refs_duplicate".to_owned());
            }
        }

        // A fresh CAS for the restored root. The old root's CAS is never reopened here, so the
        // only way to write to it after this point is to have held a token that is now refused.
        let mut cas = OperationLeaseCas::new(
            request.restored_root.storage_root,
            request.new_authority_epoch,
            request.new_data_epoch,
        )?;
        let new_lease = cas.acquire(
            cas.revision(),
            request.new_operation_id,
            request.new_instance_id,
            request.new_fence_token,
            request.new_authority_epoch,
            request.new_data_epoch,
            request.issued_at_unix_ms,
            request.lease_ttl_ms,
        )?;
        if new_lease.fence_token == request.superseded.fence_token {
            return Err("restore_activation_fence_token_reused".to_owned());
        }

        let mut record = Self {
            schema: RESTORE_ACTIVATION_RECORD_SCHEMA.to_owned(),
            version: RESTORE_ACTIVATION_VERSION,
            activation_id: request.activation_id,
            stage: ActivationStage::Activated,
            activated_storage_root: request.restored_root.storage_root,
            activated_instance_id: request.new_instance_id,
            new_lease_id: new_lease.lease_id,
            new_lease,
            new_fence_token: request.new_fence_token,
            authority_epoch: request.new_authority_epoch,
            data_epoch: request.new_data_epoch,
            superseded_storage_root: request.superseded.storage_root,
            superseded_instance_id: request.superseded.instance_id,
            superseded_fence_token: request.superseded.fence_token,
            superseded_write_mode: RootWriteMode::ReadOnly,
            superseded_retained: true,
            audit_refs,
            request_digest: request.request_digest.clone(),
            report_digest: report.report_digest.clone(),
            record_digest: String::new(),
        };
        record.record_digest = record.digest();
        record.validate_against(request)?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RESTORE_ACTIVATION_RECORD_SCHEMA
            || !self.version.is_compatible_with(&RESTORE_ACTIVATION_VERSION)
            || self.activation_id.as_uuid().is_nil()
            || self.activated_storage_root.as_uuid().is_nil()
            || self.activated_instance_id.as_uuid().is_nil()
            || self.new_fence_token.as_uuid().is_nil()
            || self.superseded_storage_root.as_uuid().is_nil()
            || self.superseded_instance_id.as_uuid().is_nil()
            || self.superseded_fence_token.as_uuid().is_nil()
            || self.authority_epoch == 0
            || self.data_epoch == 0
            || self.audit_refs.len() > MAX_RESTORE_ACTIVATION_AUDIT_REFS
        {
            return Err("restore_activation_record_header_invalid".to_owned());
        }
        // A record is by definition an activated one; a record claiming otherwise is a forgery.
        if self.stage != ActivationStage::Activated {
            return Err("restore_activation_record_not_activated".to_owned());
        }
        // The replaced root is kept, read-only, and distinct from the one now serving.
        if self.superseded_write_mode != RootWriteMode::ReadOnly {
            return Err("restore_activation_record_old_root_writable".to_owned());
        }
        if !self.superseded_retained {
            return Err("restore_activation_record_old_root_not_retained".to_owned());
        }
        if self.activated_storage_root == self.superseded_storage_root {
            return Err("restore_activation_would_overwrite_active".to_owned());
        }
        if self.activated_instance_id == self.superseded_instance_id {
            return Err("restore_activation_instance_identity_reused".to_owned());
        }
        // The old token is burned. Reissuing it would let the replaced writer back in.
        if self.new_fence_token == self.superseded_fence_token {
            return Err("restore_activation_fence_token_reused".to_owned());
        }
        self.new_lease.validate()?;
        if self.new_lease.state != OperationLeaseState::Active
            || self.new_lease.lease_id != self.new_lease_id
            || self.new_lease.fence_token != self.new_fence_token
            || self.new_lease.storage_root != self.activated_storage_root
            || self.new_lease.owner_instance_id != self.activated_instance_id
            || self.new_lease.authority_epoch != self.authority_epoch
            || self.new_lease.data_epoch != self.data_epoch
        {
            return Err("restore_activation_record_lease_binding_invalid".to_owned());
        }
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for reference in &self.audit_refs {
            safe_text(reference, "restore_activation_audit_ref")?;
            if !seen.insert(reference.as_str()) {
                return Err("restore_activation_audit_refs_duplicate".to_owned());
            }
        }
        valid_digest(&self.request_digest, "restore_activation_request_digest")?;
        valid_digest(&self.report_digest, "restore_activation_report_digest")?;
        valid_digest(&self.record_digest, "restore_activation_record_digest")?;
        if self.record_digest != self.digest() {
            return Err("restore_activation_record_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate_against(&self, request: &RestoreActivationRequest) -> Result<(), String> {
        request.validate()?;
        self.validate()?;
        if self.activation_id != request.activation_id
            || self.request_digest != request.request_digest
            || self.activated_storage_root != request.restored_root.storage_root
            || self.activated_instance_id != request.new_instance_id
            || self.new_fence_token != request.new_fence_token
            || self.authority_epoch != request.new_authority_epoch
            || self.data_epoch != request.new_data_epoch
            || self.superseded_storage_root != request.superseded.storage_root
            || self.superseded_instance_id != request.superseded.instance_id
            || self.superseded_fence_token != request.superseded.fence_token
        {
            return Err("restore_activation_record_binding_invalid".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "activation_id": self.activation_id,
            "stage": self.stage,
            "activated_storage_root": self.activated_storage_root,
            "activated_instance_id": self.activated_instance_id,
            "new_lease": self.new_lease,
            "new_lease_id": self.new_lease_id,
            "new_fence_token": self.new_fence_token,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "superseded_storage_root": self.superseded_storage_root,
            "superseded_instance_id": self.superseded_instance_id,
            "superseded_fence_token": self.superseded_fence_token,
            "superseded_write_mode": self.superseded_write_mode,
            "superseded_retained": self.superseded_retained,
            "audit_refs": self.audit_refs,
            "request_digest": self.request_digest,
            "report_digest": self.report_digest,
        }))
    }
}

/// The bounded history of activations.
///
/// One activation identity may appear once. A second record under the same identity that does not
/// hash to the same value is a contradiction, not an update, and is refused rather than merged.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivationLedger {
    pub schema: String,
    pub version: SchemaVersion,
    pub records: Vec<RestoreActivationRecord>,
    pub ledger_digest: String,
}

impl ActivationLedger {
    pub fn new(records: Vec<RestoreActivationRecord>) -> Result<Self, String> {
        if records.len() > MAX_RESTORE_ACTIVATION_RECORDS {
            return Err("restore_activation_ledger_unbounded".to_owned());
        }
        let mut ledger = Self {
            schema: RESTORE_ACTIVATION_LEDGER_SCHEMA.to_owned(),
            version: RESTORE_ACTIVATION_VERSION,
            records,
            ledger_digest: String::new(),
        };
        ledger.ledger_digest = ledger.digest();
        ledger.validate()?;
        Ok(ledger)
    }

    /// Append one record. Replaying the identical record is a no-op; a different record under the
    /// same identity is refused.
    pub fn record(&mut self, incoming: RestoreActivationRecord) -> Result<(), String> {
        self.validate()?;
        incoming.validate()?;
        if let Some(existing) = self
            .records
            .iter()
            .find(|record| record.activation_id == incoming.activation_id)
        {
            if existing.record_digest != incoming.record_digest {
                return Err("restore_activation_identity_digest_conflict".to_owned());
            }
            return Ok(());
        }
        if self
            .records
            .iter()
            .any(|record| record.data_epoch >= incoming.data_epoch)
        {
            return Err("restore_activation_epoch_regression".to_owned());
        }
        if self.records.len() >= MAX_RESTORE_ACTIVATION_RECORDS {
            return Err("restore_activation_ledger_unbounded".to_owned());
        }
        self.records.push(incoming);
        self.ledger_digest = self.digest();
        self.validate()?;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RESTORE_ACTIVATION_LEDGER_SCHEMA
            || !self.version.is_compatible_with(&RESTORE_ACTIVATION_VERSION)
            || self.records.len() > MAX_RESTORE_ACTIVATION_RECORDS
        {
            return Err("restore_activation_ledger_header_invalid".to_owned());
        }
        let mut identities: BTreeSet<RequestId> = BTreeSet::new();
        let mut previous_epoch = 0;
        for record in &self.records {
            record.validate()?;
            if !identities.insert(record.activation_id) {
                return Err("restore_activation_identity_digest_conflict".to_owned());
            }
            if record.data_epoch <= previous_epoch {
                return Err("restore_activation_epoch_regression".to_owned());
            }
            previous_epoch = record.data_epoch;
        }
        valid_digest(&self.ledger_digest, "restore_activation_ledger_digest")?;
        if self.ledger_digest != self.digest() {
            return Err("restore_activation_ledger_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// The latest activated root, or `None` when nothing has been activated yet.
    pub fn serving(&self) -> Option<&RestoreActivationRecord> {
        self.records.last()
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "records": self.records,
        }))
    }
}

/// The readiness gate a command meets. It exists before activation as well as after, because
/// "no new work until ready" has to hold in both windows.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivationState {
    pub schema: String,
    pub version: SchemaVersion,
    pub stage: ActivationStage,
    pub activated_storage_root: StorageRootId,
    pub activated_instance_id: InstanceId,
    pub new_fence_token: FenceTokenId,
    pub authority_epoch: u64,
    pub data_epoch: u64,
    pub superseded_storage_root: StorageRootId,
    pub superseded_instance_id: InstanceId,
    pub state_digest: String,
}

impl ActivationState {
    /// The state of a restored root that has not been activated. It is not ready.
    pub fn prepared(request: &RestoreActivationRequest) -> Result<Self, String> {
        request.validate()?;
        let mut state = Self {
            schema: RESTORE_ACTIVATION_STATE_SCHEMA.to_owned(),
            version: RESTORE_ACTIVATION_VERSION,
            stage: ActivationStage::Prepared,
            activated_storage_root: request.restored_root.storage_root,
            activated_instance_id: request.new_instance_id,
            new_fence_token: request.new_fence_token,
            authority_epoch: request.new_authority_epoch,
            data_epoch: request.new_data_epoch,
            superseded_storage_root: request.superseded.storage_root,
            superseded_instance_id: request.superseded.instance_id,
            state_digest: String::new(),
        };
        state.state_digest = state.digest();
        state.validate()?;
        Ok(state)
    }

    pub fn from_record(record: &RestoreActivationRecord) -> Result<Self, String> {
        record.validate()?;
        let mut state = Self {
            schema: RESTORE_ACTIVATION_STATE_SCHEMA.to_owned(),
            version: RESTORE_ACTIVATION_VERSION,
            stage: ActivationStage::Activated,
            activated_storage_root: record.activated_storage_root,
            activated_instance_id: record.activated_instance_id,
            new_fence_token: record.new_fence_token,
            authority_epoch: record.authority_epoch,
            data_epoch: record.data_epoch,
            superseded_storage_root: record.superseded_storage_root,
            superseded_instance_id: record.superseded_instance_id,
            state_digest: String::new(),
        };
        state.state_digest = state.digest();
        state.validate()?;
        Ok(state)
    }

    /// Only an activated root is ready to serve.
    pub fn ready(&self) -> bool {
        self.stage == ActivationStage::Activated
    }

    /// The one root that may take writes right now.
    pub fn writable_root(&self) -> Option<&StorageRootId> {
        self.ready().then_some(&self.activated_storage_root)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RESTORE_ACTIVATION_STATE_SCHEMA
            || !self.version.is_compatible_with(&RESTORE_ACTIVATION_VERSION)
            || self.activated_storage_root.as_uuid().is_nil()
            || self.activated_instance_id.as_uuid().is_nil()
            || self.new_fence_token.as_uuid().is_nil()
            || self.superseded_storage_root.as_uuid().is_nil()
            || self.superseded_instance_id.as_uuid().is_nil()
            || self.authority_epoch == 0
            || self.data_epoch == 0
        {
            return Err("restore_activation_state_header_invalid".to_owned());
        }
        // One root cannot be both the serving root and the replaced one, in any stage.
        if self.activated_storage_root == self.superseded_storage_root {
            return Err("restore_activation_would_overwrite_active".to_owned());
        }
        valid_digest(&self.state_digest, "restore_activation_state_digest")?;
        if self.state_digest != self.digest() {
            return Err("restore_activation_state_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "stage": self.stage,
            "activated_storage_root": self.activated_storage_root,
            "activated_instance_id": self.activated_instance_id,
            "new_fence_token": self.new_fence_token,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "superseded_storage_root": self.superseded_storage_root,
            "superseded_instance_id": self.superseded_instance_id,
        }))
    }
}

/// The readiness gate for command intake.
///
/// A command is refused outright before activation, refused against the replaced root once
/// activation has happened, and accepted only against the activated root under the new fence and
/// epochs. The order is fixed: readiness first, because nothing new is admitted in either window
/// until it is answered; then the replaced root, because writing where the pre-restore state
/// still lives is the failure this step exists to prevent; then the fence and the epochs.
pub fn admit_command_after_activation(
    state: &ActivationState,
    command_root: &StorageRootId,
    command_instance_id: &InstanceId,
    command_fence_token: FenceTokenId,
    command_authority_epoch: u64,
    command_data_epoch: u64,
) -> Result<(), String> {
    state.validate()?;
    if !state.ready() {
        return Err("restore_activation_not_ready".to_owned());
    }
    if command_root == &state.superseded_storage_root {
        return Err("restore_activation_superseded_root_write".to_owned());
    }
    if command_root != &state.activated_storage_root {
        return Err("restore_activation_command_root_unknown".to_owned());
    }
    if *command_instance_id != state.activated_instance_id {
        return Err("restore_activation_command_instance_unknown".to_owned());
    }
    if command_fence_token != state.new_fence_token {
        return Err("restore_activation_command_fence_mismatch".to_owned());
    }
    if command_authority_epoch != state.authority_epoch {
        return Err("restore_activation_command_authority_epoch_mismatch".to_owned());
    }
    if command_data_epoch != state.data_epoch {
        return Err("restore_activation_command_data_epoch_mismatch".to_owned());
    }
    Ok(())
}

/// Audit reads stay available on the replaced root.
///
/// Activation keeps the old root so the pre-restore state can still be examined. That is a
/// deliberate exception to the write refusal above, and it is bounded to roots this activation
/// actually names; anything else is refused rather than served.
pub fn admit_audit_read_after_activation(
    state: &ActivationState,
    reader_root: &StorageRootId,
) -> Result<(), String> {
    state.validate()?;
    if reader_root == &state.superseded_storage_root || reader_root == &state.activated_storage_root
    {
        return Ok(());
    }
    Err("restore_activation_audit_root_unknown".to_owned())
}

fn safe_text(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_RESTORE_ACTIVATION_TEXT
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
