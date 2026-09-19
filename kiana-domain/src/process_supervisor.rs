//! Shared process-budget and stop-confirmation contracts.
//!
//! A process exit is not by itself proof that a process tree stopped.  The daemon supervisor
//! fills these values from OS observations; callers must keep the execution fenced unless the
//! report validates as confirmed.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const PROCESS_RESOURCE_BUDGET_SCHEMA: &str = "kiana.process-resource-budget.v1";
pub const STOP_REPORT_SCHEMA: &str = "kiana.stop-report.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopMethod {
    None,
    Term,
    Kill,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessGroupState {
    Empty,
    Present,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessResourceBudget {
    pub schema: String,
    pub max_wall_time_ms: u64,
    pub max_cpu_time_ms: u64,
    pub max_address_space_bytes: u64,
    pub max_processes: u64,
    pub max_open_files: u64,
    pub max_file_bytes: u64,
    pub max_output_bytes: u64,
}

impl Default for ProcessResourceBudget {
    fn default() -> Self {
        Self::for_wall_time_ms(30_000)
    }
}

impl ProcessResourceBudget {
    pub fn for_wall_time_ms(max_wall_time_ms: u64) -> Self {
        Self {
            schema: PROCESS_RESOURCE_BUDGET_SCHEMA.to_owned(),
            max_wall_time_ms: max_wall_time_ms.clamp(1, 3_600_000),
            max_cpu_time_ms: 300_000,
            max_address_space_bytes: 2 * 1024 * 1024 * 1024,
            max_processes: 1_024,
            max_open_files: 256,
            max_file_bytes: 64 * 1024 * 1024,
            max_output_bytes: 1024 * 1024,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROCESS_RESOURCE_BUDGET_SCHEMA
            || self.max_wall_time_ms == 0
            || self.max_cpu_time_ms == 0
            || self.max_address_space_bytes == 0
            || self.max_processes == 0
            || self.max_open_files == 0
            || self.max_file_bytes == 0
            || self.max_output_bytes == 0
            || self.max_wall_time_ms > 3_600_000
        {
            return Err("process_resource_budget_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StopReport {
    pub schema: String,
    pub execution_id: String,
    pub leader_pid: Option<u32>,
    pub process_group_id: Option<u32>,
    pub method: StopMethod,
    pub term_sent: bool,
    pub kill_sent: bool,
    pub leader_reaped: bool,
    pub process_group: ProcessGroupState,
    pub confirmed: bool,
    pub waited_ms: u64,
    pub report_digest: String,
}

impl StopReport {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        execution_id: impl Into<String>,
        leader_pid: Option<u32>,
        process_group_id: Option<u32>,
        method: StopMethod,
        term_sent: bool,
        kill_sent: bool,
        leader_reaped: bool,
        process_group: ProcessGroupState,
        confirmed: bool,
        waited_ms: u64,
    ) -> Result<Self, String> {
        let mut report = Self {
            schema: STOP_REPORT_SCHEMA.to_owned(),
            execution_id: execution_id.into(),
            leader_pid,
            process_group_id,
            method,
            term_sent,
            kill_sent,
            leader_reaped,
            process_group,
            confirmed,
            waited_ms,
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate()?;
        Ok(report)
    }

    pub fn unknown(execution_id: impl Into<String>) -> Self {
        Self::new(
            execution_id,
            None,
            None,
            StopMethod::None,
            false,
            false,
            false,
            ProcessGroupState::Unknown,
            false,
            0,
        )
        .unwrap_or_else(|_| Self {
            schema: STOP_REPORT_SCHEMA.to_owned(),
            execution_id: "invalid".to_owned(),
            leader_pid: None,
            process_group_id: None,
            method: StopMethod::None,
            term_sent: false,
            kill_sent: false,
            leader_reaped: false,
            process_group: ProcessGroupState::Unknown,
            confirmed: false,
            waited_ms: 0,
            report_digest: "sha256:invalid".to_owned(),
        })
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STOP_REPORT_SCHEMA
            || self.execution_id.trim().is_empty()
            || self.execution_id.len() > 256
            || self.leader_pid.is_some_and(|pid| pid == 0)
            || self.process_group_id.is_some_and(|pid| pid == 0)
            || self.waited_ms > 120_000
            || (self.confirmed
                && (!self.leader_reaped || self.process_group != ProcessGroupState::Empty))
            || (self.method == StopMethod::None && (self.term_sent || self.kill_sent))
            || (!self.kill_sent && self.method == StopMethod::Kill)
            || self.report_digest != self.digest()
        {
            return Err("stop_report_invalid".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "execution_id": self.execution_id,
            "leader_pid": self.leader_pid,
            "process_group_id": self.process_group_id,
            "method": self.method,
            "term_sent": self.term_sent,
            "kill_sent": self.kill_sent,
            "leader_reaped": self.leader_reaped,
            "process_group": self.process_group,
            "confirmed": self.confirmed,
            "waited_ms": self.waited_ms,
        }))
    }
}
