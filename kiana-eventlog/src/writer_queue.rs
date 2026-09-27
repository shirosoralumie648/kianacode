//! Bounded multi-writer queue admission, flush/shutdown, cancellation and resource caps.
//!
//! PD-06 already made a full writer queue refuse new work with a stable saturation code instead
//! of waiting without a bound, and ER-06 already made `flush`/`close` observable
//! acknowledgements. This module is the *source contract* for the questions those two leave
//! open: what a writer may enqueue, what a refusal looks like, when a closed queue stops
//! accepting, and what a cancelled or hard-killed writer must hand over.
//!
//! Two rules carry the whole card:
//!
//! 1. **A full queue rejects observably; it never drops silently.** A dropped event is invisible:
//!    the caller believes it committed, the store never receives it, and the two diverge with no
//!    error anywhere. Every refusal here carries a stable reason code, and a refused admission
//!    reports zero admitted depth and zero admitted bytes, so it can never be read as a success
//!    that looks like an append.
//! 2. **After shutdown returns, no writer may still be appending.** A late append is refused
//!    (`writer_queue_closed`), not parked in a drain that has already happened.
//!
//! The capacity vocabulary is reused rather than reinvented: [`WriterQueuePolicy::from_capacity_envelope`]
//! derives the queue bounds from the `CapacityEnvelope` that DEP-17 already registered, instead of
//! declaring a second set of numbers that could drift away from it.
//!
//! This module is read-only evidence. It enqueues nothing, flushes nothing, opens no file, takes
//! no OS lock and releases none; it decides and reports over facts an adapter supplies. It does
//! not prove that a real queue ever rejected anything.

use kiana_domain::{
    json_digest, redact_text, scan_secret_sentinels, CapacityEnvelope, EventCursor, FenceTokenId,
    InstanceId, RunId, SchemaVersion, SecretScanChannel, StorageLockId, StorageRootId,
};
use serde::{Deserialize, Serialize};

pub const WRITER_QUEUE_POLICY_SCHEMA: &str = "kiana.writer-queue-policy.v1";
pub const WRITER_REGISTRY_SCHEMA: &str = "kiana.writer-registry.v1";
pub const WRITER_TAKEOVER_SCHEMA: &str = "kiana.writer-takeover.v1";
pub const WRITER_TAKEOVER_REPORT_SCHEMA: &str = "kiana.writer-takeover-report.v1";
pub const WRITER_ADMISSION_SCHEMA: &str = "kiana.writer-admission.v1";
pub const WRITER_ADMISSION_REPORT_SCHEMA: &str = "kiana.writer-admission-report.v1";
pub const WRITER_SHUTDOWN_SCHEMA: &str = "kiana.writer-shutdown.v1";
pub const WRITER_QUEUE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// Upper bound on the writers one store registry may track at once.
pub const MAX_QUEUE_WRITERS: u32 = 16;
/// Upper bound on a writer label or lease reference length.
pub const MAX_WRITER_LABEL: usize = 128;
/// Upper bound on the lease references a shutdown record may carry.
pub const MAX_OUTSTANDING_LEASES: usize = 32;

/// The admission state of one store's writer queue.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriterQueueState {
    /// Accepting bounded appends.
    Open,
    /// New appends blocked; accepted frames are still being made durable.
    Draining,
    /// Shutdown returned. No writer may append now, and no late append is accepted later.
    Closed,
    /// The adapter could not establish the state.
    Unknown,
}

impl WriterQueueState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Draining => "draining",
            Self::Closed => "closed",
            Self::Unknown => "unknown",
        }
    }

    /// Only an open queue may take a new append. Every other state refuses, and an unestablished
    /// state is not an open one.
    pub const fn accepts_appends(self) -> bool {
        matches!(self, Self::Open)
    }
}

/// The declared bound for one store's writer queue.
///
/// These are the resource caps the card names. A queue with no declared cap is not "fast"; it is
/// unbounded, and an unbounded queue under load is how a store stops exposing new facts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriterQueuePolicy {
    pub schema: String,
    pub version: SchemaVersion,
    /// Frames the queue may hold across all writers.
    pub max_queue_depth: u64,
    /// Bytes the queue may buffer across all writers.
    pub max_queue_bytes: u64,
    /// Writers the registry admits at once.
    pub max_writers: u32,
    /// Frames a single writer may hold pending.
    pub max_pending_per_writer: u64,
    /// Largest single frame the queue accepts.
    pub max_frame_bytes: u64,
    /// Adapter-declared drain budget for a graceful close.
    pub shutdown_drain_timeout_micros: u64,
    pub policy_digest: String,
}

impl WriterQueuePolicy {
    pub fn new(
        max_queue_depth: u64,
        max_queue_bytes: u64,
        max_writers: u32,
        max_pending_per_writer: u64,
        max_frame_bytes: u64,
        shutdown_drain_timeout_micros: u64,
    ) -> Result<Self, String> {
        let mut policy = Self {
            schema: WRITER_QUEUE_POLICY_SCHEMA.to_owned(),
            version: WRITER_QUEUE_VERSION,
            max_queue_depth,
            max_queue_bytes,
            max_writers,
            max_pending_per_writer,
            max_frame_bytes,
            shutdown_drain_timeout_micros,
            policy_digest: String::new(),
        };
        policy.policy_digest = policy.digest();
        policy.validate()?;
        Ok(policy)
    }

    /// Derive the queue bounds from the capacity envelope DEP-17 already registered.
    ///
    /// The queue depth, the per-writer pending cap and the frame size are the *same* numbers the
    /// persistence capacity report already budgets, re-expressed as writer-queue bounds. This
    /// slice therefore adds no second capacity vocabulary; it only consumes the first one. The
    /// envelope's own `validate` runs first, so a policy can never be built from an envelope whose
    /// `backpressure_preserves_facts` guard is absent.
    pub fn from_capacity_envelope(
        envelope: &CapacityEnvelope,
        max_queue_bytes: u64,
        max_writers: u32,
        shutdown_drain_timeout_micros: u64,
    ) -> Result<Self, String> {
        envelope.validate()?;
        Self::new(
            envelope.observability_queue_capacity,
            max_queue_bytes,
            max_writers,
            envelope.max_batch_events,
            envelope.max_event_bytes,
            shutdown_drain_timeout_micros,
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WRITER_QUEUE_POLICY_SCHEMA || self.version != WRITER_QUEUE_VERSION {
            return Err("writer_queue_policy_header_invalid".to_owned());
        }
        if [
            self.max_queue_depth,
            self.max_queue_bytes,
            self.max_pending_per_writer,
            self.max_frame_bytes,
            self.shutdown_drain_timeout_micros,
        ]
        .into_iter()
        .any(|value| value == 0)
        {
            return Err("writer_queue_policy_bounds_missing".to_owned());
        }
        if self.max_writers == 0 || self.max_writers > MAX_QUEUE_WRITERS {
            return Err("writer_queue_policy_writer_count_invalid".to_owned());
        }
        // A per-writer cap above the queue cap is a cap that can never bind, which would leave the
        // per-writer resource limit looking enforced while it is not.
        if self.max_pending_per_writer > self.max_queue_depth {
            return Err("writer_queue_policy_pending_exceeds_queue".to_owned());
        }
        valid_digest(&self.policy_digest, "writer_queue_policy_digest")?;
        if self.policy_digest != self.digest() {
            return Err("writer_queue_policy_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "max_queue_depth": self.max_queue_depth,
            "max_queue_bytes": self.max_queue_bytes,
            "max_writers": self.max_writers,
            "max_pending_per_writer": self.max_pending_per_writer,
            "max_frame_bytes": self.max_frame_bytes,
            "shutdown_drain_timeout_micros": self.shutdown_drain_timeout_micros,
        }))
    }
}

/// Which writer instance currently holds the store's writer lease.
///
/// The registry is the answer to "who may append right now". It exists because a multi-process
/// store cannot rely on process-local state: the second process has to see the first one's lease,
/// epoch and fence token before it writes, and a hard-killed holder has to be distinguishable from
/// a live one.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriterRegistry {
    pub schema: String,
    pub version: SchemaVersion,
    pub storage_root_id: StorageRootId,
    pub lock_id: StorageLockId,
    pub holder_instance: InstanceId,
    pub holder_writer: String,
    pub data_epoch: u64,
    pub fence_token: FenceTokenId,
    pub acquired_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub registry_digest: String,
}

impl WriterRegistry {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        storage_root_id: StorageRootId,
        lock_id: StorageLockId,
        holder_instance: InstanceId,
        holder_writer: impl Into<String>,
        data_epoch: u64,
        fence_token: FenceTokenId,
        acquired_at_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut registry = Self {
            schema: WRITER_REGISTRY_SCHEMA.to_owned(),
            version: WRITER_QUEUE_VERSION,
            storage_root_id,
            lock_id,
            holder_instance,
            holder_writer: holder_writer.into(),
            data_epoch,
            fence_token,
            acquired_at_unix_ms,
            expires_at_unix_ms,
            registry_digest: String::new(),
        };
        registry.registry_digest = registry.digest();
        registry.validate()?;
        Ok(registry)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WRITER_REGISTRY_SCHEMA
            || self.version != WRITER_QUEUE_VERSION
            || self.storage_root_id.as_uuid().is_nil()
            || self.lock_id.as_uuid().is_nil()
            || self.holder_instance.as_uuid().is_nil()
            || self.fence_token.as_uuid().is_nil()
            || self.data_epoch == 0
        {
            return Err("writer_registry_header_invalid".to_owned());
        }
        // A lease that expires at or before it was acquired is not a lease; it is a claim that
        // never bounded anything, and every takeover decision downstream would rest on it.
        if self.expires_at_unix_ms <= self.acquired_at_unix_ms {
            return Err("writer_registry_lease_window_invalid".to_owned());
        }
        safe_label(&self.holder_writer, "writer_registry_holder")?;
        valid_digest(&self.registry_digest, "writer_registry_digest")?;
        if self.registry_digest != self.digest() {
            return Err("writer_registry_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "storage_root_id": self.storage_root_id,
            "lock_id": self.lock_id,
            "holder_instance": self.holder_instance,
            "holder_writer": self.holder_writer,
            "data_epoch": self.data_epoch,
            "fence_token": self.fence_token,
            "acquired_at_unix_ms": self.acquired_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }
}

/// A successor process's claim to inherit a writer lease, typically after the previous holder was
/// killed without releasing it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriterTakeover {
    pub schema: String,
    pub version: SchemaVersion,
    pub lock_id: StorageLockId,
    /// The registry the successor believes it is replacing.
    pub previous_holder: InstanceId,
    pub previous_registry_digest: String,
    pub new_holder: InstanceId,
    pub new_writer: String,
    /// Must be strictly greater than the previous holder's epoch.
    pub new_data_epoch: u64,
    /// Must differ from the previous holder's token, so a resurrected writer cannot pass it.
    pub new_fence_token: FenceTokenId,
    /// Whether the adapter established that the previous holder's lease has lapsed.
    pub previous_lease_expired: bool,
    pub takeover_digest: String,
}

impl WriterTakeover {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        lock_id: StorageLockId,
        previous_holder: InstanceId,
        previous_registry_digest: impl Into<String>,
        new_holder: InstanceId,
        new_writer: impl Into<String>,
        new_data_epoch: u64,
        new_fence_token: FenceTokenId,
        previous_lease_expired: bool,
    ) -> Result<Self, String> {
        let mut takeover = Self {
            schema: WRITER_TAKEOVER_SCHEMA.to_owned(),
            version: WRITER_QUEUE_VERSION,
            lock_id,
            previous_holder,
            previous_registry_digest: previous_registry_digest.into(),
            new_holder,
            new_writer: new_writer.into(),
            new_data_epoch,
            new_fence_token,
            previous_lease_expired,
            takeover_digest: String::new(),
        };
        takeover.takeover_digest = takeover.digest();
        takeover.validate()?;
        Ok(takeover)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WRITER_TAKEOVER_SCHEMA
            || self.version != WRITER_QUEUE_VERSION
            || self.lock_id.as_uuid().is_nil()
            || self.previous_holder.as_uuid().is_nil()
            || self.new_holder.as_uuid().is_nil()
            || self.new_fence_token.as_uuid().is_nil()
            || self.new_data_epoch == 0
            || self.new_holder == self.previous_holder
        {
            return Err("writer_takeover_header_invalid".to_owned());
        }
        safe_label(&self.new_writer, "writer_takeover_new_writer")?;
        valid_digest(
            &self.previous_registry_digest,
            "writer_takeover_previous_registry_digest",
        )?;
        valid_digest(&self.takeover_digest, "writer_takeover_digest")?;
        if self.takeover_digest != self.digest() {
            return Err("writer_takeover_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "lock_id": self.lock_id,
            "previous_holder": self.previous_holder,
            "previous_registry_digest": self.previous_registry_digest,
            "new_holder": self.new_holder,
            "new_writer": self.new_writer,
            "new_data_epoch": self.new_data_epoch,
            "new_fence_token": self.new_fence_token,
            "previous_lease_expired": self.previous_lease_expired,
        }))
    }
}

/// The two-valued answer a caller gets for any write-path request.
///
/// One vocabulary for both queue admission and lease takeover: a caller learns that its request
/// was refused, and the report's `reason` says why in a stable code.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriterAdmissionStatus {
    Accepted,
    Rejected,
}

impl WriterAdmissionStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
        }
    }
}

/// The ordered decision for a lease takeover.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriterTakeoverReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub lock_id: StorageLockId,
    pub new_holder: InstanceId,
    pub status: WriterAdmissionStatus,
    pub reason: String,
    pub remediation: String,
    pub previous_registry_digest: String,
    pub report_digest: String,
}

impl WriterTakeoverReport {
    pub fn evaluate(previous: &WriterRegistry, takeover: &WriterTakeover) -> Result<Self, String> {
        previous.validate()?;
        takeover.validate()?;
        let (status, reason, remediation) = derive_takeover(previous, takeover);
        let mut report = Self {
            schema: WRITER_TAKEOVER_REPORT_SCHEMA.to_owned(),
            version: WRITER_QUEUE_VERSION,
            lock_id: takeover.lock_id,
            new_holder: takeover.new_holder,
            status,
            reason: reason.to_owned(),
            remediation: remediation.to_owned(),
            previous_registry_digest: previous.registry_digest.clone(),
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate_against(previous, takeover)?;
        Ok(report)
    }

    pub fn validate_against(
        &self,
        previous: &WriterRegistry,
        takeover: &WriterTakeover,
    ) -> Result<(), String> {
        previous.validate()?;
        takeover.validate()?;
        let (status, reason, remediation) = derive_takeover(previous, takeover);
        if self.schema != WRITER_TAKEOVER_REPORT_SCHEMA
            || self.version != WRITER_QUEUE_VERSION
            || self.lock_id != takeover.lock_id
            || self.new_holder != takeover.new_holder
            || self.status != status
            || self.reason != reason
            || self.remediation != remediation
            || self.previous_registry_digest != previous.registry_digest
        {
            return Err("writer_takeover_report_binding_invalid".to_owned());
        }
        valid_digest(&self.report_digest, "writer_takeover_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("writer_takeover_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "lock_id": self.lock_id,
            "new_holder": self.new_holder,
            "status": self.status,
            "reason": self.reason,
            "remediation": self.remediation,
            "previous_registry_digest": self.previous_registry_digest,
        }))
    }
}

/// One append attempt against the bounded queue, with the queue occupancy the writer observed.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriterAdmission {
    pub schema: String,
    pub version: SchemaVersion,
    pub instance_id: InstanceId,
    pub run_id: RunId,
    pub writer: String,
    pub data_epoch: u64,
    pub queue_state: WriterQueueState,
    pub pending_depth_before: u64,
    pub pending_bytes_before: u64,
    pub writer_pending_before: u64,
    pub frame_count: u64,
    pub frame_bytes: u64,
    pub admission_digest: String,
}

impl WriterAdmission {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instance_id: InstanceId,
        run_id: RunId,
        writer: impl Into<String>,
        data_epoch: u64,
        queue_state: WriterQueueState,
        pending_depth_before: u64,
        pending_bytes_before: u64,
        writer_pending_before: u64,
        frame_count: u64,
        frame_bytes: u64,
    ) -> Result<Self, String> {
        let mut admission = Self {
            schema: WRITER_ADMISSION_SCHEMA.to_owned(),
            version: WRITER_QUEUE_VERSION,
            instance_id,
            run_id,
            writer: writer.into(),
            data_epoch,
            queue_state,
            pending_depth_before,
            pending_bytes_before,
            writer_pending_before,
            frame_count,
            frame_bytes,
            admission_digest: String::new(),
        };
        admission.admission_digest = admission.digest();
        admission.validate()?;
        Ok(admission)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WRITER_ADMISSION_SCHEMA
            || self.version != WRITER_QUEUE_VERSION
            || self.instance_id.as_uuid().is_nil()
            || self.run_id.as_uuid().is_nil()
            || self.data_epoch == 0
            // An admission of zero frames asserts nothing, so it may not be presented as an
            // admission at all.
            || self.frame_count == 0
            || self.frame_bytes == 0
        {
            return Err("writer_admission_header_invalid".to_owned());
        }
        safe_label(&self.writer, "writer_admission_writer")?;
        valid_digest(&self.admission_digest, "writer_admission_digest")?;
        if self.admission_digest != self.digest() {
            return Err("writer_admission_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "instance_id": self.instance_id,
            "run_id": self.run_id,
            "writer": self.writer,
            "data_epoch": self.data_epoch,
            "queue_state": self.queue_state,
            "pending_depth_before": self.pending_depth_before,
            "pending_bytes_before": self.pending_bytes_before,
            "writer_pending_before": self.writer_pending_before,
            "frame_count": self.frame_count,
            "frame_bytes": self.frame_bytes,
        }))
    }
}

/// The ordered decision for one append attempt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriterAdmissionReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub status: WriterAdmissionStatus,
    pub reason: String,
    pub remediation: String,
    /// Queue depth after the decision. Zero when the append was refused.
    pub accepted_depth_after: u64,
    /// Buffered bytes after the decision. Zero when the append was refused.
    pub accepted_bytes_after: u64,
    pub policy_digest: String,
    pub registry_digest: String,
    pub admission_digest: String,
    pub report_digest: String,
}

impl WriterAdmissionReport {
    pub fn evaluate(
        policy: &WriterQueuePolicy,
        registry: &WriterRegistry,
        admission: &WriterAdmission,
    ) -> Result<Self, String> {
        policy.validate()?;
        registry.validate()?;
        admission.validate()?;
        let (status, reason, remediation, depth, bytes) =
            derive_admission(policy, registry, admission);
        let mut report = Self {
            schema: WRITER_ADMISSION_REPORT_SCHEMA.to_owned(),
            version: WRITER_QUEUE_VERSION,
            status,
            reason: reason.to_owned(),
            remediation: remediation.to_owned(),
            accepted_depth_after: depth,
            accepted_bytes_after: bytes,
            policy_digest: policy.policy_digest.clone(),
            registry_digest: registry.registry_digest.clone(),
            admission_digest: admission.admission_digest.clone(),
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate_against(policy, registry, admission)?;
        Ok(report)
    }

    pub fn validate_against(
        &self,
        policy: &WriterQueuePolicy,
        registry: &WriterRegistry,
        admission: &WriterAdmission,
    ) -> Result<(), String> {
        policy.validate()?;
        registry.validate()?;
        admission.validate()?;
        // The structural invariant is checked before the binding one. A refusal that still carries
        // admitted depth or bytes is the silent drop this card rejects: the caller sees numbers it
        // can read as a successful append while the store never received the frame. The binding
        // check below would also catch a forged number, but it reports the generic code, so this
        // invariant is stated first and named.
        if self.status == WriterAdmissionStatus::Rejected
            && (self.accepted_depth_after != 0 || self.accepted_bytes_after != 0)
        {
            return Err("writer_report_admitted_depth_leak".to_owned());
        }
        let (status, reason, remediation, depth, bytes) =
            derive_admission(policy, registry, admission);
        if self.schema != WRITER_ADMISSION_REPORT_SCHEMA
            || self.version != WRITER_QUEUE_VERSION
            || self.status != status
            || self.reason != reason
            || self.remediation != remediation
            || self.accepted_depth_after != depth
            || self.accepted_bytes_after != bytes
            || self.policy_digest != policy.policy_digest
            || self.registry_digest != registry.registry_digest
            || self.admission_digest != admission.admission_digest
        {
            return Err("writer_admission_report_binding_invalid".to_owned());
        }
        valid_digest(&self.report_digest, "writer_admission_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("writer_admission_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "status": self.status,
            "reason": self.reason,
            "remediation": self.remediation,
            "accepted_depth_after": self.accepted_depth_after,
            "accepted_bytes_after": self.accepted_bytes_after,
            "policy_digest": self.policy_digest,
            "registry_digest": self.registry_digest,
            "admission_digest": self.admission_digest,
        }))
    }
}

/// How a writer stopped holding the queue.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriterShutdownMode {
    /// Ran the declared drain, flushed, then released.
    Graceful,
    /// Stopped on request and must hand back every lock and lease it held.
    Cancelled,
    /// Stopped without running anything. Nothing it claims after this point can be true.
    HardKill,
}

impl WriterShutdownMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Graceful => "graceful",
            Self::Cancelled => "cancelled",
            Self::HardKill => "hard_kill",
        }
    }

    /// A hard-killed writer ran no code after the kill, so it can neither have released a lock
    /// nor have made anything newly durable.
    pub const fn claims_post_kill_effects(self) -> bool {
        matches!(self, Self::Graceful | Self::Cancelled)
    }
}

/// The shutdown acknowledgement for one writer, and the residue it must not leave behind.
///
/// The card's two shutdown refusals live here. A record that returns with a writer still appending
/// is refused (`writer_shutdown_outstanding_writer`), and a cancelled writer that leaves its lock
/// or a lease behind is refused (`writer_shutdown_lease_residue`) because the next process would
/// then meet a lease nobody owns.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriterShutdown {
    pub schema: String,
    pub version: SchemaVersion,
    pub lock_id: StorageLockId,
    pub instance_id: InstanceId,
    pub data_epoch: u64,
    pub mode: WriterShutdownMode,
    pub queue_state_before: WriterQueueState,
    /// Frames still queued when the record was produced. Must be zero.
    pub pending_depth_after: u64,
    /// Bytes still buffered when the record was produced. Must be zero.
    pub pending_bytes_after: u64,
    pub durable_cursor_before: EventCursor,
    pub durable_cursor_after: EventCursor,
    /// Highest cursor ever admitted to this queue, durable or not.
    pub last_admitted_cursor: EventCursor,
    /// Writers that were still appending when the record was produced. Must be zero.
    pub outstanding_writers: u32,
    /// Leases this writer still holds. Empty unless the writer was hard-killed.
    pub outstanding_lease_refs: Vec<String>,
    pub lock_released: bool,
    pub shutdown_digest: String,
}

impl WriterShutdown {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        lock_id: StorageLockId,
        instance_id: InstanceId,
        data_epoch: u64,
        mode: WriterShutdownMode,
        queue_state_before: WriterQueueState,
        pending_depth_after: u64,
        pending_bytes_after: u64,
        durable_cursor_before: EventCursor,
        durable_cursor_after: EventCursor,
        last_admitted_cursor: EventCursor,
        outstanding_writers: u32,
        outstanding_lease_refs: Vec<String>,
        lock_released: bool,
    ) -> Result<Self, String> {
        let mut shutdown = Self {
            schema: WRITER_SHUTDOWN_SCHEMA.to_owned(),
            version: WRITER_QUEUE_VERSION,
            lock_id,
            instance_id,
            data_epoch,
            mode,
            queue_state_before,
            pending_depth_after,
            pending_bytes_after,
            durable_cursor_before,
            durable_cursor_after,
            last_admitted_cursor,
            outstanding_writers,
            outstanding_lease_refs,
            lock_released,
            shutdown_digest: String::new(),
        };
        shutdown.shutdown_digest = shutdown.digest();
        shutdown.validate()?;
        Ok(shutdown)
    }

    /// Fixed order: an unestablished state, then a queue that is already closed, then the
    /// leftovers, then cursor claims, then the mode-specific truth about a dead writer.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WRITER_SHUTDOWN_SCHEMA
            || self.version != WRITER_QUEUE_VERSION
            || self.lock_id.as_uuid().is_nil()
            || self.instance_id.as_uuid().is_nil()
            || self.data_epoch == 0
        {
            return Err("writer_shutdown_header_invalid".to_owned());
        }
        if self.queue_state_before == WriterQueueState::Unknown {
            return Err("writer_shutdown_state_unknown".to_owned());
        }
        if self.queue_state_before == WriterQueueState::Closed {
            return Err("writer_shutdown_already_closed".to_owned());
        }
        if self.pending_depth_after != 0 || self.pending_bytes_after != 0 {
            return Err("writer_shutdown_pending_remainder".to_owned());
        }
        if self.outstanding_writers != 0 {
            return Err("writer_shutdown_outstanding_writer".to_owned());
        }
        if self.durable_cursor_after < self.durable_cursor_before {
            return Err("writer_shutdown_durable_cursor_regression".to_owned());
        }
        if self.durable_cursor_after < self.last_admitted_cursor {
            return Err("writer_shutdown_unflushed_remainder".to_owned());
        }
        if self.outstanding_lease_refs.len() > MAX_OUTSTANDING_LEASES {
            return Err("writer_shutdown_lease_list_oversize".to_owned());
        }
        for reference in &self.outstanding_lease_refs {
            safe_label(reference, "writer_shutdown_lease_ref")?;
        }
        if self.mode.claims_post_kill_effects() && !self.lock_released {
            return Err("writer_shutdown_lock_not_released".to_owned());
        }
        // A cancelled writer must hand back every lease, not just the queue lock: a lease left
        // behind is an ownership nobody can revoke, and the next process inherits the ambiguity.
        if self.mode.claims_post_kill_effects() && !self.outstanding_lease_refs.is_empty() {
            return Err("writer_shutdown_lease_residue".to_owned());
        }
        if self.mode == WriterShutdownMode::HardKill && self.lock_released {
            return Err("writer_shutdown_dead_writer_released_lock".to_owned());
        }
        if self.mode == WriterShutdownMode::HardKill
            && self.durable_cursor_after > self.durable_cursor_before
        {
            return Err("writer_shutdown_dead_writer_cannot_flush".to_owned());
        }
        valid_digest(&self.shutdown_digest, "writer_shutdown_digest")?;
        if self.shutdown_digest != self.digest() {
            return Err("writer_shutdown_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "lock_id": self.lock_id,
            "instance_id": self.instance_id,
            "data_epoch": self.data_epoch,
            "mode": self.mode,
            "queue_state_before": self.queue_state_before,
            "pending_depth_after": self.pending_depth_after,
            "pending_bytes_after": self.pending_bytes_after,
            "durable_cursor_before": self.durable_cursor_before,
            "durable_cursor_after": self.durable_cursor_after,
            "last_admitted_cursor": self.last_admitted_cursor,
            "outstanding_writers": self.outstanding_writers,
            "outstanding_lease_refs": self.outstanding_lease_refs,
            "lock_released": self.lock_released,
        }))
    }
}

/// Decide a takeover in a fixed order, so the reported reason is the first violated rule and
/// therefore deterministic for the same facts.
///
/// The identity binding is checked first: a takeover bound to a registry other than the one on
/// disk is not reasoning about the right state at all. Liveness comes next, because two live
/// writers on one store is the actual hazard. Monotonic authority is checked last, because a
/// stale epoch or a reused fence token only matters once the lease is genuinely free.
fn derive_takeover(
    previous: &WriterRegistry,
    takeover: &WriterTakeover,
) -> (WriterAdmissionStatus, &'static str, &'static str) {
    if takeover.previous_registry_digest != previous.registry_digest {
        return (
            WriterAdmissionStatus::Rejected,
            "writer_takeover_registry_digest_mismatch",
            "re-read the registry and rebuild the takeover from the digest on disk",
        );
    }
    if takeover.lock_id != previous.lock_id || takeover.previous_holder != previous.holder_instance
    {
        return (
            WriterAdmissionStatus::Rejected,
            "writer_takeover_lock_mismatch",
            "take over the lease of the store actually being opened, not another one",
        );
    }
    if !takeover.previous_lease_expired {
        return (
            WriterAdmissionStatus::Rejected,
            "writer_takeover_live_holder",
            "wait for the holder's lease to lapse before inheriting the writer role",
        );
    }
    if takeover.new_data_epoch <= previous.data_epoch {
        return (
            WriterAdmissionStatus::Rejected,
            "writer_takeover_epoch_not_advanced",
            "claim a strictly higher data epoch so a resurrected writer is fenced",
        );
    }
    if takeover.new_fence_token == previous.fence_token {
        return (
            WriterAdmissionStatus::Rejected,
            "writer_takeover_fence_token_reused",
            "mint a fresh fence token; a reused token admits the writer it replaced",
        );
    }
    (
        WriterAdmissionStatus::Accepted,
        "writer_takeover_accepted",
        "none",
    )
}

/// Decide one append attempt in a fixed order.
///
/// `Unknown` is checked first, before `Closed` and before the capacity rules, because an
/// unestablished state cannot be diagnosed and must not be reported as a specific refusal. A
/// refused append always reports zero admitted depth and bytes.
fn derive_admission(
    policy: &WriterQueuePolicy,
    registry: &WriterRegistry,
    admission: &WriterAdmission,
) -> (WriterAdmissionStatus, &'static str, &'static str, u64, u64) {
    let refused = |reason: &'static str, remediation: &'static str| {
        (WriterAdmissionStatus::Rejected, reason, remediation, 0, 0)
    };
    if admission.queue_state == WriterQueueState::Unknown {
        return refused(
            "writer_queue_state_unknown",
            "establish the queue state before admitting or refusing an append",
        );
    }
    // A queue that already returned from shutdown takes no late append. Queueing it anyway would
    // place the frame behind a drain boundary that no longer exists.
    if admission.queue_state == WriterQueueState::Closed {
        return refused(
            "writer_queue_closed",
            "reopen through a new writer lease; the closed queue takes no more frames",
        );
    }
    if admission.queue_state == WriterQueueState::Draining {
        return refused(
            "writer_queue_draining",
            "wait for the drain to finish or the shutdown to return, then re-admit",
        );
    }
    // Only the instance the registry names as holder may append. A foreign instance appending
    // here is the second writer this card refuses to allow.
    if admission.instance_id != registry.holder_instance {
        return refused(
            "writer_instance_not_registry_holder",
            "take over the writer lease explicitly instead of appending as a second writer",
        );
    }
    if admission.data_epoch != registry.data_epoch {
        return refused(
            "writer_epoch_stale",
            "re-read the registry and retry under the current data epoch",
        );
    }
    if admission.frame_bytes > policy.max_frame_bytes {
        return refused(
            "writer_frame_oversize",
            "split the frame; a single frame may not exceed the declared frame cap",
        );
    }
    if admission.frame_count > policy.max_pending_per_writer {
        return refused(
            "writer_batch_oversize",
            "submit a smaller batch; one append may not exceed the per-writer pending cap",
        );
    }
    if admission
        .writer_pending_before
        .saturating_add(admission.frame_count)
        > policy.max_pending_per_writer
    {
        return refused(
            "writer_per_writer_limit_exceeded",
            "drain this writer's pending frames before submitting more",
        );
    }
    if admission
        .pending_depth_before
        .saturating_add(admission.frame_count)
        > policy.max_queue_depth
    {
        return refused(
            "writer_queue_full",
            "apply backpressure and retry; the queue is bounded and the refusal is explicit",
        );
    }
    if admission
        .pending_bytes_before
        .saturating_add(admission.frame_bytes)
        > policy.max_queue_bytes
    {
        return refused(
            "writer_queue_bytes_exceeded",
            "drain buffered bytes before submitting more; the byte cap is a hard bound",
        );
    }
    (
        WriterAdmissionStatus::Accepted,
        "writer_append_admitted",
        "none",
        admission
            .pending_depth_before
            .saturating_add(admission.frame_count),
        admission
            .pending_bytes_before
            .saturating_add(admission.frame_bytes),
    )
}

fn safe_label(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_WRITER_LABEL
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
