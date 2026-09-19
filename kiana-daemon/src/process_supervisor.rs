//! One daemon-owned process tree supervisor for shell and long-running process adapters.
//!
//! The supervisor owns process-group setup, bounded TERM/KILL/reap, stop confirmation and the
//! per-process resource limits that can actually be enforced with rlimits.  It deliberately
//! reports RSS/cgroup and aggregate-pids as not hard-enforced until a backend supplies those
//! guarantees; an RLIMIT_AS or a leader exit is never mislabeled as an aggregate proof.

use kiana_domain::{ProcessGroupState, ProcessResourceBudget, StopMethod, StopReport};
use kiana_ports::PortError;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::process::{Child, Command};

const TERM_GRACE: Duration = Duration::from_millis(100);
const KILL_GRACE: Duration = Duration::from_millis(500);

pub(crate) struct ProcessSupervisor;

impl ProcessSupervisor {
    pub(crate) fn prepare_command(
        command: &mut Command,
        budget: &ProcessResourceBudget,
    ) -> Result<(), PortError> {
        budget
            .validate()
            .map_err(|error| PortError::Failed(error.to_owned()))?;
        command.kill_on_drop(true);
        #[cfg(unix)]
        {
            command.process_group(0);
            let budget = budget.clone();
            unsafe {
                command.pre_exec(move || {
                    for (resource, value) in [
                        (libc::RLIMIT_CORE, 0u64),
                        (libc::RLIMIT_CPU, budget.max_cpu_time_ms.div_ceil(1_000)),
                        (libc::RLIMIT_NOFILE, budget.max_open_files),
                        (libc::RLIMIT_FSIZE, budget.max_file_bytes),
                        (libc::RLIMIT_AS, budget.max_address_space_bytes),
                        (libc::RLIMIT_NPROC, budget.max_processes),
                    ] {
                        let limit = libc::rlimit {
                            rlim_cur: value as libc::rlim_t,
                            rlim_max: value as libc::rlim_t,
                        };
                        if libc::setrlimit(resource, &limit) != 0 {
                            return Err(std::io::Error::last_os_error());
                        }
                    }
                    Ok(())
                });
            }
        }
        Ok(())
    }

    pub(crate) fn resource_receipt(budget: &ProcessResourceBudget) -> Value {
        json!({
            "schema": budget.schema,
            "max_wall_time_ms": budget.max_wall_time_ms,
            "max_cpu_time_ms": budget.max_cpu_time_ms,
            "max_address_space_bytes": budget.max_address_space_bytes,
            "max_processes": budget.max_processes,
            "max_open_files": budget.max_open_files,
            "max_file_bytes": budget.max_file_bytes,
            "max_output_bytes": budget.max_output_bytes,
            "enforcement": {
                "wall_time": "supervisor_deadline",
                "cpu_time": "per_process_rlimit_cpu",
                "address_space": "per_process_rlimit_as",
                "rss": "observed_only",
                "pids": "per_process_rlimit_nproc",
                "aggregate_memory": "not_hard_enforced",
                "aggregate_pids": "not_hard_enforced",
                "file_and_tmp_quota": "rlimit_fsize_plus_workspace_policy"
            }
        })
    }

    pub(crate) fn guard(pid: Option<u32>) -> ProcessGuard {
        ProcessGuard { pid, active: true }
    }

    pub(crate) fn process_group_exists(pid: u32) -> bool {
        observe_process_group(pid) == ProcessGroupState::Present
    }

    pub(crate) async fn stop(
        execution_id: &str,
        child: &mut Child,
        process_group_id: Option<u32>,
    ) -> StopReport {
        let started = tokio::time::Instant::now();
        let mut leader_reaped = matches!(child.try_wait(), Ok(Some(_)));
        let mut term_sent = false;
        let mut kill_sent = false;

        let mut group_state = process_group_id
            .map(observe_process_group)
            .unwrap_or(ProcessGroupState::Unknown);
        if !leader_reaped || group_state != ProcessGroupState::Empty {
            if let Some(group) = process_group_id {
                if signal_process_group(group, libc::SIGTERM) {
                    term_sent = true;
                }
            } else if child.start_kill().is_ok() {
                kill_sent = true;
            }
        }

        if !leader_reaped {
            leader_reaped = wait_leader(child, TERM_GRACE).await;
        }
        group_state = process_group_id
            .map(observe_process_group)
            .unwrap_or(ProcessGroupState::Unknown);
        if group_state != ProcessGroupState::Empty {
            if let Some(group) = process_group_id {
                if signal_process_group(group, libc::SIGKILL) {
                    kill_sent = true;
                }
            } else if child.start_kill().is_ok() {
                kill_sent = true;
            }
        }
        if !leader_reaped {
            leader_reaped = wait_leader(child, KILL_GRACE).await;
        }
        group_state = process_group_id
            .map(observe_process_group)
            .unwrap_or(ProcessGroupState::Unknown);
        let confirmed =
            leader_reaped && process_group_id.is_some() && group_state == ProcessGroupState::Empty;
        let method = if kill_sent {
            StopMethod::Kill
        } else if term_sent {
            StopMethod::Term
        } else {
            StopMethod::None
        };
        StopReport::new(
            execution_id,
            child.id(),
            process_group_id,
            method,
            term_sent,
            kill_sent,
            leader_reaped,
            group_state,
            confirmed,
            started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
        )
        .unwrap_or_else(|_| StopReport::unknown(execution_id))
    }
}

pub(crate) struct ProcessGuard {
    pid: Option<u32>,
    active: bool,
}

impl ProcessGuard {
    pub(crate) fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for ProcessGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        #[cfg(unix)]
        if let Some(pid) = self.pid {
            let _ = signal_process_group(pid, libc::SIGKILL);
        }
    }
}

async fn wait_leader(child: &mut Child, timeout: Duration) -> bool {
    matches!(tokio::time::timeout(timeout, child.wait()).await, Ok(Ok(_)))
}

#[cfg(unix)]
fn signal_process_group(pid: u32, signal: libc::c_int) -> bool {
    let Ok(pgid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    unsafe { libc::kill(-pgid, signal) == 0 }
}

#[cfg(unix)]
fn observe_process_group(pid: u32) -> ProcessGroupState {
    let Ok(pgid) = libc::pid_t::try_from(pid) else {
        return ProcessGroupState::Unknown;
    };
    let result = unsafe { libc::kill(-pgid, 0) };
    if result == 0 {
        return ProcessGroupState::Present;
    }
    match std::io::Error::last_os_error().raw_os_error() {
        Some(libc::ESRCH) => ProcessGroupState::Empty,
        _ => ProcessGroupState::Unknown,
    }
}

#[cfg(not(unix))]
fn observe_process_group(_: u32) -> ProcessGroupState {
    ProcessGroupState::Unknown
}

#[cfg(not(unix))]
fn signal_process_group(_: u32, _: i32) -> bool {
    false
}
