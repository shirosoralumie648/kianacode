//! PD-30 storage fault-injection matrix.
//!
//! The card names seven faults — kill -9, disk full, permissions, lock contention, a corrupt
//! frame, crash timing and a network Unknown — and one invariant for all of them: *no fault may
//! produce `Completed`, a duplicate effect, or a recovery that exceeds the adapter's authority*.
//!
//! This module is that invariant, as data. It is a **source contract over classifications**, not
//! an injection harness: it does not `kill`, `fsync`, fill a disk, take a lock, corrupt a frame,
//! reopen a store or contact a network. Each [`StorageFaultCase`] is a decision over a supplied
//! [`StorageError`] and nothing else, so a green report here is `proof_level=source` — it says the
//! classification is coherent, not that any of the seven faults was ever produced.
//!
//! ## Vocabulary is reused, not reinvented
//!
//! * [`StorageErrorClass`] already keeps `empty` / `unavailable` / `corrupt` / `conflict` /
//!   `unknown` / `result_unknown` distinct at the storage boundary, and already derives
//!   `retryable()` and `requires_reconciliation()`. The state classification here *is* that class,
//!   taken straight off the `StorageError` rather than re-typed.
//! * [`FaultInjectionPoint`] is the existing crash-timing vocabulary from `kiana-domain`; PD-30
//!   binds one to a crash-timing case instead of naming new crash points.
//! * [`AdapterKind`] is PD-31's adapter set, so a fault case is stated against an adapter that
//!   already has a conformance declaration rather than against a parallel adapter registry.
//!
//! ## What the matrix refuses
//!
//! The decision order in [`StorageFaultMatrix::evaluate`] is fixed, so the reported reason is the
//! first violated rule and therefore the same for the same facts:
//!
//! 1. a case that claims the fault completed (`storage_fault_completion_claimed`),
//! 2. a duplicate effect (`storage_fault_duplicate_effect`),
//! 3. a recovery beyond the adapter's authority (`storage_fault_recovery_beyond_authority`),
//! 4. a reconciling class that is not fenced (`storage_fault_unknown_outcome_not_fenced`),
//! 5. a corrupt frame that is not quarantined (`storage_fault_corrupt_not_quarantined`),
//! 6. a class that does not match the fault it was filed under (`storage_fault_class_mismatch`),
//! 7. a missing or wrong exit code (`storage_fault_exit_code_invalid`),
//! 8. a case with no limitation evidence (`storage_fault_limitation_required`).
//!
//! A matrix is only reported as `Refused` — every case classified, none unsafe — when all seven
//! kinds are present exactly once.

use crate::adapter_conformance::AdapterKind;
use kiana_domain::{
    json_digest, redact_text, scan_secret_sentinels, FaultInjectionPoint, SchemaVersion,
    SecretScanChannel, StorageError, StorageErrorClass,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const STORAGE_FAULT_MATRIX_SCHEMA: &str = "kiana.pd30-storage-fault-matrix.v1";
pub const STORAGE_FAULT_CASE_SCHEMA: &str = "kiana.pd30-storage-fault-case.v1";
pub const STORAGE_FAULT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// One case per [`StorageFaultKind`], so a matrix is an evidence record and not a log.
pub const MAX_STORAGE_FAULT_CASES: usize = 7;
/// Per-case limitation evidence. A case with nothing to disclose is treated as unproven.
pub const MAX_STORAGE_FAULT_LIMITATIONS: usize = 8;
pub const MAX_STORAGE_FAULT_TEXT: usize = 256;

/// The seven faults the card names. `All` is the required coverage of a matrix.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageFaultKind {
    /// SIGKILL between an accepted write and its durable acknowledgement.
    KillNine,
    /// ENOSPC part-way through an append.
    DiskFull,
    /// EACCES/EPERM on the journal, the lock or the artifact directory.
    PermissionDenied,
    /// A second writer holds the single-writer lock.
    LockContention,
    /// A torn or malformed frame at the tail of the journal.
    CorruptFrame,
    /// A crash at a named point on the existing injection-point timeline.
    CrashTiming,
    /// A remote call whose outcome the writer never learned.
    NetworkUnknown,
}

impl StorageFaultKind {
    pub const ALL: [Self; 7] = [
        Self::KillNine,
        Self::DiskFull,
        Self::PermissionDenied,
        Self::LockContention,
        Self::CorruptFrame,
        Self::CrashTiming,
        Self::NetworkUnknown,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::KillNine => "kill_nine",
            Self::DiskFull => "disk_full",
            Self::PermissionDenied => "permission_denied",
            Self::LockContention => "lock_contention",
            Self::CorruptFrame => "corrupt_frame",
            Self::CrashTiming => "crash_timing",
            Self::NetworkUnknown => "network_unknown",
        }
    }

    /// The one [`StorageErrorClass`] this fault is allowed to be filed under.
    ///
    /// This is the state classification the card asks for, expressed in the vocabulary
    /// `kiana-domain` already froze. A case filed under the wrong class is refused rather than
    /// silently reclassified, because a disk-full reported as `Conflict` would invite a retry that
    /// a full disk cannot satisfy.
    pub const fn expected_class(self) -> StorageErrorClass {
        match self {
            Self::KillNine => StorageErrorClass::Unknown,
            Self::DiskFull => StorageErrorClass::Unavailable,
            Self::PermissionDenied => StorageErrorClass::Unavailable,
            Self::LockContention => StorageErrorClass::Conflict,
            Self::CorruptFrame => StorageErrorClass::Corrupt,
            Self::CrashTiming => StorageErrorClass::ResultUnknown,
            Self::NetworkUnknown => StorageErrorClass::ResultUnknown,
        }
    }

    /// Only crash timing names a point on the existing injection timeline.
    pub const fn expects_crash_point(self) -> bool {
        matches!(self, Self::CrashTiming)
    }
}

/// The action a store may take after the fault. It is a *declared* action, not a performed one.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageFaultRecovery {
    /// The fault landed before any fact was accepted; the caller is told it never happened.
    RefuseWithoutEffect,
    /// The outcome is unknown and only a fact-level read can settle it.
    ReconcileFromFacts,
    /// Recovery restarts from the last committed prefix and re-probes the store.
    RestartFromCommittedPrefix,
    /// A fence is taken before anything else may write, then the facts are reconciled.
    FenceThenReconcile,
    /// The store stops and is quarantined for an operator.
    QuarantineForOperator,
}

impl StorageFaultRecovery {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RefuseWithoutEffect => "refuse_without_effect",
            Self::ReconcileFromFacts => "reconcile_from_facts",
            Self::RestartFromCommittedPrefix => "restart_from_committed_prefix",
            Self::FenceThenReconcile => "fence_then_reconcile",
            Self::QuarantineForOperator => "quarantine_for_operator",
        }
    }

    /// A reconciling class must not be answered by "nothing happened". Anything else is a
    /// fault the caller is invited to retry blind.
    pub const fn settles_reconciling_class(self) -> bool {
        !matches!(self, Self::RefuseWithoutEffect)
    }
}

/// The exit code a process would carry for this fault.
///
/// This is a *declared* vocabulary, not an observed one: no process was started and no code was
/// captured from a real exit. `Clean` exists so a fault case that claims success has something
/// concrete to be caught claiming, and it is not a value any fault case may carry.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageFaultExit {
    /// The process finished its work. No fault case may declare this.
    Clean,
    /// The fault was refused before any effect; the caller may retry from facts.
    Refused,
    /// The commit outcome is unknown; do not retry blindly.
    UnknownOutcome,
    /// The store is quarantined and an operator must act.
    Quarantined,
    /// The fault could not be classified; treat it as unknown, never as success.
    Unclassified,
}

impl StorageFaultExit {
    pub const fn as_i32(self) -> i32 {
        match self {
            Self::Clean => 0,
            Self::Refused => 2,
            Self::UnknownOutcome => 3,
            Self::Quarantined => 4,
            Self::Unclassified => 5,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Refused => "refused",
            Self::UnknownOutcome => "unknown_outcome",
            Self::Quarantined => "quarantined",
            Self::Unclassified => "unclassified",
        }
    }

    /// The exit a case's own classification implies. A case carrying anything else is refused, so
    /// a shell script cannot observe `0` from a store that lost a write.
    pub fn for_fault(class: StorageErrorClass, effect_started: bool) -> Self {
        if class == StorageErrorClass::Corrupt {
            Self::Quarantined
        } else if class == StorageErrorClass::Unknown || class == StorageErrorClass::ResultUnknown {
            Self::UnknownOutcome
        } else if effect_started {
            Self::Unclassified
        } else {
            Self::Refused
        }
    }
}

/// Whether a matrix is safe to report, or carries at least one unsafe classification.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageFaultMatrixStatus {
    /// Every case is classified, fenced, bounded and carries limitation evidence.
    Refused,
    /// At least one case claims a completion, a duplicate effect or an over-reaching recovery.
    Unsafe,
}

impl StorageFaultMatrixStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Refused => "refused",
            Self::Unsafe => "unsafe",
        }
    }
}

/// One classified fault, bound to the adapter that reported it and the error it reported.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageFaultCase {
    pub schema: String,
    pub version: SchemaVersion,
    pub kind: StorageFaultKind,
    /// Required for `CrashTiming`, forbidden for every other kind.
    pub crash_point: Option<FaultInjectionPoint>,
    /// The adapter-reported error. Its `class` is the state classification; nothing is retyped.
    pub error: StorageError,
    pub adapter: AdapterKind,
    pub recovery: StorageFaultRecovery,
    pub exit_code: StorageFaultExit,
    pub effect_started: bool,
    pub effect_confirmed: bool,
    /// The card's first rejection: a fault must never be reported as a completed command.
    pub completion_claimed: bool,
    /// The card's second rejection.
    pub duplicate_effect: bool,
    /// The card's third rejection: recovering past what the adapter's own authority covers.
    pub recovery_beyond_authority: bool,
    /// What this classification does **not** establish. Required, never empty.
    pub limitations: Vec<String>,
    pub case_digest: String,
}

impl StorageFaultCase {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        kind: StorageFaultKind,
        crash_point: Option<FaultInjectionPoint>,
        error: StorageError,
        adapter: AdapterKind,
        recovery: StorageFaultRecovery,
        exit_code: StorageFaultExit,
        effect_started: bool,
        effect_confirmed: bool,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: STORAGE_FAULT_CASE_SCHEMA.to_owned(),
            version: STORAGE_FAULT_VERSION,
            kind,
            crash_point,
            error,
            adapter,
            recovery,
            exit_code,
            effect_started,
            effect_confirmed,
            completion_claimed: false,
            duplicate_effect: false,
            recovery_beyond_authority: false,
            limitations,
            case_digest: String::new(),
        };
        value.case_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    /// The state classification, taken from the adapter's own error rather than restated here.
    pub fn class(&self) -> StorageErrorClass {
        self.error.class
    }

    pub fn validate(&self) -> Result<(), String> {
        self.error.validate()?;
        if self.schema != STORAGE_FAULT_CASE_SCHEMA
            || !self.version.is_compatible_with(&STORAGE_FAULT_VERSION)
        {
            return Err("storage_fault_case_header_invalid".to_owned());
        }
        if self.class() != self.kind.expected_class() {
            return Err("storage_fault_class_mismatch".to_owned());
        }
        // Crash timing is the only fault that names a point on the injection timeline, and it must
        // name one: an unnamed crash point is not a classification of anything.
        if self.kind.expects_crash_point() != self.crash_point.is_some() {
            return Err("storage_fault_crash_point_invalid".to_owned());
        }
        // A reconciled or unconfirmed effect may never be reported as a confirmed one, and a
        // confirmed effect is a `Completed` claim the card forbids for a fault.
        if self.effect_confirmed && !self.effect_started {
            return Err("storage_fault_effect_confirmation_invalid".to_owned());
        }
        if self.effect_confirmed && self.class().requires_reconciliation() {
            return Err("storage_fault_unknown_outcome_not_fenced".to_owned());
        }
        if self.completion_claimed || self.duplicate_effect || self.recovery_beyond_authority {
            return Err("storage_fault_case_unsafe".to_owned());
        }
        // A reconciling class must be fenced, never answered with "nothing happened".
        if self.class().requires_reconciliation() && !self.recovery.settles_reconciling_class() {
            return Err("storage_fault_unknown_outcome_not_fenced".to_owned());
        }
        if self.class() == StorageErrorClass::Corrupt
            && self.recovery != StorageFaultRecovery::QuarantineForOperator
        {
            return Err("storage_fault_corrupt_not_quarantined".to_owned());
        }
        if self.exit_code != StorageFaultExit::for_fault(self.class(), self.effect_started) {
            return Err("storage_fault_exit_code_invalid".to_owned());
        }
        if self.limitations.is_empty() || self.limitations.len() > MAX_STORAGE_FAULT_LIMITATIONS {
            return Err("storage_fault_limitation_required".to_owned());
        }
        for limitation in &self.limitations {
            safe_text(limitation, "storage_fault_limitation")?;
        }
        validate_digest(&self.case_digest, "storage_fault_case_digest")?;
        if self.case_digest != self.digest() {
            return Err("storage_fault_case_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "kind": self.kind,
            "crash_point": self.crash_point,
            "error": self.error,
            "adapter": self.adapter,
            "recovery": self.recovery,
            "exit_code": self.exit_code,
            "effect_started": self.effect_started,
            "effect_confirmed": self.effect_confirmed,
            "completion_claimed": self.completion_claimed,
            "duplicate_effect": self.duplicate_effect,
            "recovery_beyond_authority": self.recovery_beyond_authority,
            "limitations": self.limitations,
        }))
    }
}

/// The matrix over one adapter set: one case per [`StorageFaultKind::ALL`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageFaultMatrix {
    pub schema: String,
    pub version: SchemaVersion,
    pub status: StorageFaultMatrixStatus,
    pub baseline_digest: String,
    pub cases: Vec<StorageFaultCase>,
    /// Stable denial code, suffixed with the offending fault kind. Empty on a refused matrix.
    pub reason: String,
    pub remediation: String,
    pub report_digest: String,
}

impl StorageFaultMatrix {
    pub fn evaluate(baseline_digest: impl Into<String>, cases: Vec<StorageFaultCase>) -> Self {
        let baseline_digest = baseline_digest.into();
        let (status, reason, remediation) = derive(&baseline_digest, &cases);
        let mut matrix = Self {
            schema: STORAGE_FAULT_MATRIX_SCHEMA.to_owned(),
            version: STORAGE_FAULT_VERSION,
            status,
            baseline_digest,
            cases,
            reason,
            remediation,
            report_digest: String::new(),
        };
        matrix.report_digest = matrix.digest();
        matrix
    }

    /// Whether the matrix may be presented as "this fault class is refused". A matrix that could
    /// not even be built is never a pass, so this is the only positive reading.
    pub fn all_faults_refused(&self) -> bool {
        self.status == StorageFaultMatrixStatus::Refused && self.reason.is_empty()
    }

    /// The stable code an operator-facing surface would print for the first unsafe case.
    pub fn refusal_code(&self) -> Option<&str> {
        (!self.reason.is_empty()).then_some(self.reason.as_str())
    }

    /// The exit code a process would carry for this matrix, taken from its decision.
    pub fn exit_code(&self) -> StorageFaultExit {
        if self.status == StorageFaultMatrixStatus::Refused {
            StorageFaultExit::Refused
        } else {
            StorageFaultExit::Unclassified
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STORAGE_FAULT_MATRIX_SCHEMA
            || !self.version.is_compatible_with(&STORAGE_FAULT_VERSION)
            || self.cases.is_empty()
            || self.cases.len() > MAX_STORAGE_FAULT_CASES
        {
            return Err("storage_fault_matrix_header_invalid".to_owned());
        }
        validate_digest(
            &self.baseline_digest,
            "storage_fault_matrix_baseline_digest",
        )?;
        validate_digest(&self.report_digest, "storage_fault_matrix_report_digest")?;
        let (status, reason, remediation) = derive(&self.baseline_digest, &self.cases);
        if self.status != status || self.reason != reason || self.remediation != remediation {
            return Err("storage_fault_matrix_binding_invalid".to_owned());
        }
        if self.status == StorageFaultMatrixStatus::Refused && !self.reason.is_empty() {
            return Err("storage_fault_matrix_refused_with_reason".to_owned());
        }
        if self.status == StorageFaultMatrixStatus::Unsafe && self.reason.is_empty() {
            return Err("storage_fault_matrix_unsafe_without_reason".to_owned());
        }
        if self.report_digest != self.digest() {
            return Err("storage_fault_matrix_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "status": self.status,
            "baseline_digest": self.baseline_digest,
            "cases": self.cases,
            "reason": self.reason,
            "remediation": self.remediation,
        }))
    }
}

/// Decide in a fixed order, so the reported reason is the first violated rule and therefore the
/// same for the same facts. The first three rules are positive observations of the failure the card
/// names; the rest are absences of proof, ordered by how badly a caller would be misled.
fn derive(
    baseline_digest: &str,
    cases: &[StorageFaultCase],
) -> (StorageFaultMatrixStatus, String, String) {
    if validate_digest(baseline_digest, "storage_fault_matrix_baseline_digest").is_err()
        || cases.is_empty()
    {
        return (
            StorageFaultMatrixStatus::Unsafe,
            "storage_fault_matrix_header_invalid".to_owned(),
            "supply a sealed baseline digest and at most one case per named fault".to_owned(),
        );
    }
    // 【为什么数量上限要排在「同一故障重复」之后】
    // 卡片点名的故障恰好有 `StorageFaultKind::ALL.len()` 个，而 `MAX_STORAGE_FAULT_CASES`
    // 与它相等。于是**任何一份重复清单都必然先撞上数量上限**——上面那个检查若留在原处，
    // 下面 `storage_fault_kind_duplicate` 分支就成了**永远走不到的死代码**：它那句
    // 「a second case for the same fault is a second claim」永远不会对调用方说出口，
    // 重复声明只会被报成一句笼统的 header 错误。
    // 把数量上限挪到重复检测之后：重复是一条**更具体、更有诊断价值**的结论，
    // 应当先说；数量上限则退回到它本来的角色——挡住荒谬的大清单。
    // 两个检查都仍在，没有被删除或放宽。
    // Coverage first: a matrix that is missing a fault has not classified it, and a green report
    // over five of the seven would otherwise read as a green report over all of them.
    let mut kinds = BTreeSet::new();
    for case in cases {
        if case.validate().is_err() {
            return (
                StorageFaultMatrixStatus::Unsafe,
                format!("storage_fault_case_invalid:{}", case.kind.as_str()),
                "rebuild the case from the adapter-reported error and the declared limitations"
                    .to_owned(),
            );
        }
        if !kinds.insert(case.kind) {
            return (
                StorageFaultMatrixStatus::Unsafe,
                "storage_fault_kind_duplicate".to_owned(),
                "file one case per named fault; a second case for the same fault is a second claim"
                    .to_owned(),
            );
        }
    }
    if cases.len() > MAX_STORAGE_FAULT_CASES {
        return (
            StorageFaultMatrixStatus::Unsafe,
            "storage_fault_matrix_header_invalid".to_owned(),
            "supply a sealed baseline digest and at most one case per named fault".to_owned(),
        );
    }
    if kinds.len() != StorageFaultKind::ALL.len() {
        let missing = StorageFaultKind::ALL
            .into_iter()
            .find(|kind| !kinds.contains(kind))
            .map(|kind| kind.as_str())
            .unwrap_or("unknown");
        return (
            StorageFaultMatrixStatus::Unsafe,
            format!("storage_fault_kind_missing:{missing}"),
            "classify every fault the card names, including the ones this host cannot produce"
                .to_owned(),
        );
    }
    if let Some(case) = cases.iter().find(|case| case.completion_claimed) {
        return (
            StorageFaultMatrixStatus::Unsafe,
            format!("storage_fault_completion_claimed:{}", case.kind.as_str()),
            "report the outcome as unknown; a faulted command is never a completed command"
                .to_owned(),
        );
    }
    if let Some(case) = cases.iter().find(|case| case.duplicate_effect) {
        return (
            StorageFaultMatrixStatus::Unsafe,
            format!("storage_fault_duplicate_effect:{}", case.kind.as_str()),
            "reconcile the command against its fact before any effect is replayed".to_owned(),
        );
    }
    if let Some(case) = cases.iter().find(|case| case.recovery_beyond_authority) {
        return (
            StorageFaultMatrixStatus::Unsafe,
            format!(
                "storage_fault_recovery_beyond_authority:{}",
                case.kind.as_str()
            ),
            "recover only within the adapter's own authority; a restore is not a repair".to_owned(),
        );
    }
    if let Some(case) = cases
        .iter()
        .find(|case| case.class().requires_reconciliation() && case.effect_confirmed)
    {
        return (
            StorageFaultMatrixStatus::Unsafe,
            format!(
                "storage_fault_unknown_outcome_not_fenced:{}",
                case.kind.as_str()
            ),
            "an unreconciled outcome cannot be confirmed; settle it from committed facts"
                .to_owned(),
        );
    }
    if let Some(case) = cases.iter().find(|case| {
        case.class() == StorageErrorClass::Corrupt
            && case.recovery != StorageFaultRecovery::QuarantineForOperator
    }) {
        return (
            StorageFaultMatrixStatus::Unsafe,
            format!(
                "storage_fault_corrupt_not_quarantined:{}",
                case.kind.as_str()
            ),
            "quarantine the store and stop writing; a corrupt frame is not a retry".to_owned(),
        );
    }
    if let Some(case) = cases.iter().find(|case| {
        case.exit_code != StorageFaultExit::for_fault(case.class(), case.effect_started)
    }) {
        return (
            StorageFaultMatrixStatus::Unsafe,
            format!("storage_fault_exit_code_invalid:{}", case.kind.as_str()),
            "derive the exit code from the classification; a faulted store may not exit zero"
                .to_owned(),
        );
    }
    if let Some(case) = cases.iter().find(|case| case.limitations.is_empty()) {
        return (
            StorageFaultMatrixStatus::Unsafe,
            format!("storage_fault_limitation_required:{}", case.kind.as_str()),
            "state what this classification did not establish".to_owned(),
        );
    }
    (
        StorageFaultMatrixStatus::Refused,
        String::new(),
        String::new(),
    )
}

/// Refuse a restart that would hand back a cursor the store never established.
///
/// A faulted store may legitimately say "I lost track"; it may not say "I am at cursor N" when the
/// fault was the reason it lost track. The durable cursor is the caller's to supply, and a case
/// that has not been reconciled cannot be given one.
pub fn admit_fault_restart(case: &StorageFaultCase, durable_cursor: u64) -> Result<u64, String> {
    case.validate()?;
    if case.class().requires_reconciliation() && !case.recovery.settles_reconciling_class() {
        return Err("storage_fault_restart_not_fenced".to_owned());
    }
    if durable_cursor == 0 && case.effect_started {
        return Err("storage_fault_restart_cursor_not_established".to_owned());
    }
    if case.effect_confirmed && durable_cursor == 0 {
        return Err("storage_fault_restart_cursor_not_established".to_owned());
    }
    Ok(durable_cursor)
}

/// The faults a host cannot produce are still named, so an unproven row is visible as a missing
/// one rather than absent from the matrix.
pub fn storage_fault_scope() -> BTreeSet<&'static str> {
    StorageFaultKind::ALL
        .into_iter()
        .map(|kind| kind.as_str())
        .collect()
}

fn safe_text(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_STORAGE_FAULT_TEXT
        || value.contains(['\0', '\r', '\n'])
    {
        return Err(format!("{field}_invalid"));
    }
    if redact_text(value) != value {
        return Err(format!("{field}_not_redacted"));
    }
    scan_secret_sentinels(SecretScanChannel::Receipt, value)
        .map_err(|_| format!("{field}_secret_detected"))
}

fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 && hex.len() != 71 {
        return Err(format!("{field}_invalid"));
    }
    if !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
