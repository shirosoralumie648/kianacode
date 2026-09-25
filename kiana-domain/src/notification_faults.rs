//! Deterministic notification-chain fault matrix contracts.
//!
//! This is a replay-only classification matrix. It records the safety response expected for
//! duplicate/out-of-order input, cursor loss, crash windows, slow consumers, queue/disk pressure,
//! secret sentinels and projection loss. It never kills a process, writes an event, sends a
//! notification or invokes a connector.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const NOTIFICATION_FAULT_MATRIX_SCHEMA: &str = "kiana.notification-fault-matrix.v1";
pub const NOTIFICATION_FAULT_CASE_SCHEMA: &str = "kiana.notification-fault-case.v1";
pub const NOTIFICATION_FAULT_MAX_CASES: usize = 32;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationFaultScenario {
    Duplicate,
    OutOfOrder,
    CursorGap,
    CrashAfterClaim,
    SlowConsumer,
    QueueFull,
    DiskFull,
    SecretSentinel,
    ProjectionLoss,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationFaultDisposition {
    Rejected,
    Unknown,
    SnapshotRequired,
    ReconcileRequired,
    Backpressure,
    Quarantined,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationFaultCase {
    pub schema: String,
    pub scenario: NotificationFaultScenario,
    pub source_cursor: u64,
    pub source_event_ids: Vec<String>,
    pub disposition: NotificationFaultDisposition,
    pub critical_preserved: bool,
    pub duplicate_suppressed: bool,
    pub snapshot_required: bool,
    pub reconcile_required: bool,
    pub effect_started: bool,
    pub secret_free: bool,
    pub case_digest: String,
}

impl NotificationFaultCase {
    fn new(
        scenario: NotificationFaultScenario,
        source_cursor: u64,
        source_event_ids: Vec<String>,
    ) -> Result<Self, String> {
        let (
            disposition,
            critical_preserved,
            duplicate_suppressed,
            snapshot_required,
            reconcile_required,
            secret_free,
        ) = match scenario {
            NotificationFaultScenario::Duplicate => (
                NotificationFaultDisposition::Rejected,
                true,
                true,
                false,
                false,
                true,
            ),
            NotificationFaultScenario::OutOfOrder
            | NotificationFaultScenario::CursorGap
            | NotificationFaultScenario::ProjectionLoss => (
                NotificationFaultDisposition::SnapshotRequired,
                true,
                false,
                true,
                true,
                true,
            ),
            NotificationFaultScenario::CrashAfterClaim | NotificationFaultScenario::DiskFull => (
                NotificationFaultDisposition::ReconcileRequired,
                true,
                false,
                false,
                true,
                true,
            ),
            NotificationFaultScenario::SlowConsumer | NotificationFaultScenario::QueueFull => (
                NotificationFaultDisposition::Backpressure,
                true,
                false,
                true,
                false,
                true,
            ),
            NotificationFaultScenario::SecretSentinel => (
                NotificationFaultDisposition::Quarantined,
                true,
                false,
                false,
                true,
                true,
            ),
        };
        let mut case = Self {
            schema: NOTIFICATION_FAULT_CASE_SCHEMA.to_owned(),
            scenario,
            source_cursor,
            source_event_ids,
            disposition,
            critical_preserved,
            duplicate_suppressed,
            snapshot_required,
            reconcile_required,
            effect_started: false,
            secret_free,
            case_digest: String::new(),
        };
        case.case_digest = case.digest();
        case.validate()?;
        Ok(case)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_FAULT_CASE_SCHEMA
            || self.source_cursor == 0
            || self.source_event_ids.is_empty()
            || self
                .source_event_ids
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            || self.effect_started
            || !self.critical_preserved
            || !self.secret_free
        {
            return Err("notification_fault_case_invariant_failed".to_owned());
        }
        if matches!(
            self.disposition,
            NotificationFaultDisposition::SnapshotRequired
                | NotificationFaultDisposition::ReconcileRequired
                | NotificationFaultDisposition::Quarantined
        ) && !self.reconcile_required
        {
            return Err("notification_fault_reconcile_marker_missing".to_owned());
        }
        if self.disposition == NotificationFaultDisposition::SnapshotRequired
            && !self.snapshot_required
        {
            return Err("notification_fault_snapshot_marker_missing".to_owned());
        }
        if self.case_digest != self.digest() {
            return Err("notification_fault_case_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "scenario": self.scenario,
            "source_cursor": self.source_cursor,
            "source_event_ids": self.source_event_ids,
            "disposition": self.disposition,
            "critical_preserved": self.critical_preserved,
            "duplicate_suppressed": self.duplicate_suppressed,
            "snapshot_required": self.snapshot_required,
            "reconcile_required": self.reconcile_required,
            "effect_started": self.effect_started,
            "secret_free": self.secret_free,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationFaultMatrix {
    pub schema: String,
    pub seed: u64,
    pub source_cursor: u64,
    pub source_event_ids: Vec<String>,
    pub cases: Vec<NotificationFaultCase>,
    pub matrix_digest: String,
}

impl NotificationFaultMatrix {
    pub fn new(
        seed: u64,
        source_cursor: u64,
        mut source_event_ids: Vec<String>,
    ) -> Result<Self, String> {
        if seed == 0 || source_cursor == 0 || source_event_ids.is_empty() {
            return Err("notification_fault_matrix_header_invalid".to_owned());
        }
        source_event_ids.sort();
        source_event_ids.dedup();
        let scenarios = [
            NotificationFaultScenario::Duplicate,
            NotificationFaultScenario::OutOfOrder,
            NotificationFaultScenario::CursorGap,
            NotificationFaultScenario::CrashAfterClaim,
            NotificationFaultScenario::SlowConsumer,
            NotificationFaultScenario::QueueFull,
            NotificationFaultScenario::DiskFull,
            NotificationFaultScenario::SecretSentinel,
            NotificationFaultScenario::ProjectionLoss,
        ];
        let cases = scenarios
            .into_iter()
            .map(|scenario| {
                NotificationFaultCase::new(scenario, source_cursor, source_event_ids.clone())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut matrix = Self {
            schema: NOTIFICATION_FAULT_MATRIX_SCHEMA.to_owned(),
            seed,
            source_cursor,
            source_event_ids,
            cases,
            matrix_digest: String::new(),
        };
        matrix.matrix_digest = matrix.digest();
        matrix.validate()?;
        Ok(matrix)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_FAULT_MATRIX_SCHEMA
            || self.seed == 0
            || self.source_cursor == 0
            || self.source_event_ids.is_empty()
            || self.cases.is_empty()
            || self.cases.len() > NOTIFICATION_FAULT_MAX_CASES
        {
            return Err("notification_fault_matrix_invalid".to_owned());
        }
        for case in &self.cases {
            case.validate()?;
            if case.source_cursor != self.source_cursor
                || case.source_event_ids != self.source_event_ids
            {
                return Err("notification_fault_case_source_mismatch".to_owned());
            }
        }
        if self.matrix_digest != self.digest() {
            return Err("notification_fault_matrix_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "seed": self.seed,
            "source_cursor": self.source_cursor,
            "source_event_ids": self.source_event_ids,
            "cases": self.cases,
        }))
    }
}
