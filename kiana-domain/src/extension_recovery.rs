//! Extension crash-point and recovery-decision contracts.
//!
//! These records classify what a fresh process must do after a package, registry, Hook or Broker
//! boundary is interrupted. They never reopen a store, retry a command, execute a Hook or resolve
//! an unknown external result; those actions require the existing EventLog/ControlPlane/Broker
//! adapters and a new authority check.

use crate::{canonical_journal_bytes, json_digest, redact_text, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const EXTENSION_RECOVERY_CASE_SCHEMA: &str = "kiana.extension-recovery-case.v1";
pub const EXTENSION_RECOVERY_MATRIX_SCHEMA: &str = "kiana.extension-recovery-matrix.v1";
pub const EXTENSION_RECOVERY_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
const MAX_CASES: usize = 16;

fn bounded(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    if redact_text(value) != value {
        return Err(format!("{field}_contains_secret"));
    }
    Ok(())
}

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn clear_digest<T: Serialize>(value: &T, field: &str) -> String {
    let mut value = serde_json::to_value(value).unwrap_or(serde_json::Value::Null);
    if let Some(object) = value.as_object_mut() {
        object.insert(field.to_owned(), serde_json::Value::String(String::new()));
    }
    json_digest(&value)
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionCrashPoint {
    PackageBeforeRename,
    RenameBeforeEvent,
    EventBeforeProjection,
    UpgradeSwitch,
    HookRunning,
    ApprovalExpiring,
    BrokerResultUnknown,
}

impl ExtensionCrashPoint {
    pub const ALL: [Self; 7] = [
        Self::PackageBeforeRename,
        Self::RenameBeforeEvent,
        Self::EventBeforeProjection,
        Self::UpgradeSwitch,
        Self::HookRunning,
        Self::ApprovalExpiring,
        Self::BrokerResultUnknown,
    ];
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionRecoveryAction {
    QuarantinePackage,
    RetryIdempotentCommand,
    RebuildRegistryAndSnapshot,
    AwaitApproval,
    ReconcileUnknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionRecoveryTerminal {
    Recovered,
    Blocked,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionRecoveryCase {
    pub schema: String,
    pub version: SchemaVersion,
    pub case_id: String,
    pub crash_point: ExtensionCrashPoint,
    pub source_cursor: u64,
    pub registry_revision: u64,
    pub pre_state_digest: String,
    pub snapshot_digest: String,
    pub idempotency_digest: String,
    pub fence_digest: String,
    pub pending_approval: bool,
    pub effect_started: bool,
    pub effect_known: bool,
    pub duplicate_effect: bool,
    pub action: ExtensionRecoveryAction,
    pub terminal: ExtensionRecoveryTerminal,
    pub recovery_receipt_digest: Option<String>,
    pub quarantine_package_digest: Option<String>,
    pub case_digest: String,
}

impl ExtensionRecoveryCase {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        case_id: impl Into<String>,
        crash_point: ExtensionCrashPoint,
        source_cursor: u64,
        registry_revision: u64,
        pre_state_digest: impl Into<String>,
        snapshot_digest: impl Into<String>,
        idempotency_digest: impl Into<String>,
        fence_digest: impl Into<String>,
        pending_approval: bool,
        effect_started: bool,
        effect_known: bool,
        duplicate_effect: bool,
        action: ExtensionRecoveryAction,
        terminal: ExtensionRecoveryTerminal,
        recovery_receipt_digest: Option<String>,
        quarantine_package_digest: Option<String>,
    ) -> Result<Self, String> {
        let mut case = Self {
            schema: EXTENSION_RECOVERY_CASE_SCHEMA.to_owned(),
            version: EXTENSION_RECOVERY_VERSION,
            case_id: case_id.into(),
            crash_point,
            source_cursor,
            registry_revision,
            pre_state_digest: pre_state_digest.into(),
            snapshot_digest: snapshot_digest.into(),
            idempotency_digest: idempotency_digest.into(),
            fence_digest: fence_digest.into(),
            pending_approval,
            effect_started,
            effect_known,
            duplicate_effect,
            action,
            terminal,
            recovery_receipt_digest,
            quarantine_package_digest,
            case_digest: String::new(),
        };
        case.case_digest = case.digest();
        case.validate()?;
        Ok(case)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_RECOVERY_CASE_SCHEMA
            || self.version != EXTENSION_RECOVERY_VERSION
            || self.source_cursor == 0
            || self.registry_revision == 0
            || self.duplicate_effect
        {
            return Err("extension_recovery_case_header_invalid".to_owned());
        }
        bounded(&self.case_id, "extension_recovery_case_id", 128)?;
        for (value, field) in [
            (
                &self.pre_state_digest,
                "extension_recovery_pre_state_digest",
            ),
            (&self.snapshot_digest, "extension_recovery_snapshot_digest"),
            (
                &self.idempotency_digest,
                "extension_recovery_idempotency_digest",
            ),
            (&self.fence_digest, "extension_recovery_fence_digest"),
            (&self.case_digest, "extension_recovery_case_digest"),
        ] {
            digest(value, field)?;
        }
        for (value, field) in [
            (
                self.recovery_receipt_digest.as_deref(),
                "extension_recovery_receipt_digest",
            ),
            (
                self.quarantine_package_digest.as_deref(),
                "extension_recovery_quarantine_package_digest",
            ),
        ] {
            if let Some(value) = value {
                digest(value, field)?;
            }
        }
        match self.action {
            ExtensionRecoveryAction::QuarantinePackage => {
                if self.effect_started
                    || self.terminal != ExtensionRecoveryTerminal::Blocked
                    || self.quarantine_package_digest.is_none()
                {
                    return Err("extension_recovery_quarantine_action_invalid".to_owned());
                }
            }
            ExtensionRecoveryAction::RetryIdempotentCommand => {
                if self.effect_started
                    || self.pending_approval
                    || self.terminal != ExtensionRecoveryTerminal::Recovered
                    || self.recovery_receipt_digest.is_none()
                {
                    return Err("extension_recovery_retry_action_invalid".to_owned());
                }
            }
            ExtensionRecoveryAction::RebuildRegistryAndSnapshot => {
                if self.terminal == ExtensionRecoveryTerminal::Unknown
                    || self.recovery_receipt_digest.is_none()
                {
                    return Err("extension_recovery_rebuild_action_invalid".to_owned());
                }
            }
            ExtensionRecoveryAction::AwaitApproval => {
                if !self.pending_approval
                    || self.effect_started
                    || self.terminal != ExtensionRecoveryTerminal::Blocked
                {
                    return Err("extension_recovery_approval_action_invalid".to_owned());
                }
            }
            ExtensionRecoveryAction::ReconcileUnknown => {
                if !self.effect_started
                    || self.effect_known
                    || self.terminal != ExtensionRecoveryTerminal::Unknown
                {
                    return Err("extension_recovery_unknown_action_invalid".to_owned());
                }
            }
        }
        if self.crash_point == ExtensionCrashPoint::BrokerResultUnknown
            && (self.action != ExtensionRecoveryAction::ReconcileUnknown
                || self.terminal != ExtensionRecoveryTerminal::Unknown)
        {
            return Err("extension_recovery_broker_unknown_must_reconcile".to_owned());
        }
        if self.crash_point == ExtensionCrashPoint::HookRunning
            && self.action == ExtensionRecoveryAction::RetryIdempotentCommand
        {
            return Err("extension_recovery_hook_must_not_auto_retry".to_owned());
        }
        if self.case_digest != self.digest() {
            return Err("extension_recovery_case_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        clear_digest(self, "case_digest")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionRecoveryMatrix {
    pub schema: String,
    pub version: SchemaVersion,
    pub source_snapshot_digest: String,
    pub cases: Vec<ExtensionRecoveryCase>,
    pub matrix_digest: String,
}

impl ExtensionRecoveryMatrix {
    pub fn new(
        source_snapshot_digest: impl Into<String>,
        cases: Vec<ExtensionRecoveryCase>,
    ) -> Result<Self, String> {
        let mut matrix = Self {
            schema: EXTENSION_RECOVERY_MATRIX_SCHEMA.to_owned(),
            version: EXTENSION_RECOVERY_VERSION,
            source_snapshot_digest: source_snapshot_digest.into(),
            cases,
            matrix_digest: String::new(),
        };
        matrix.matrix_digest = matrix.digest();
        matrix.validate()?;
        Ok(matrix)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_RECOVERY_MATRIX_SCHEMA
            || self.version != EXTENSION_RECOVERY_VERSION
            || self.cases.is_empty()
            || self.cases.len() > MAX_CASES
        {
            return Err("extension_recovery_matrix_header_invalid".to_owned());
        }
        digest(
            &self.source_snapshot_digest,
            "extension_recovery_source_snapshot_digest",
        )?;
        let mut points = BTreeSet::new();
        let mut ids = BTreeSet::new();
        for case in &self.cases {
            case.validate()?;
            if case.snapshot_digest != self.source_snapshot_digest
                || !points.insert(case.crash_point)
                || !ids.insert(&case.case_id)
            {
                return Err("extension_recovery_matrix_case_binding_invalid".to_owned());
            }
        }
        if ExtensionCrashPoint::ALL
            .iter()
            .any(|point| !points.contains(point))
        {
            return Err("extension_recovery_matrix_coverage_missing".to_owned());
        }
        digest(&self.matrix_digest, "extension_recovery_matrix_digest")?;
        if self.matrix_digest != self.digest() {
            return Err("extension_recovery_matrix_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn unknown_cases(&self) -> Result<Vec<String>, String> {
        self.validate()?;
        Ok(self
            .cases
            .iter()
            .filter(|case| case.terminal == ExtensionRecoveryTerminal::Unknown)
            .map(|case| case.case_id.clone())
            .collect())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    pub fn digest(&self) -> String {
        clear_digest(self, "matrix_digest")
    }
}
