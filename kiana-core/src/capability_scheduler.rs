//! Bounded capability admission, separate from capability execution.
//!
//! The scheduler only decides when an already-authorized request may cross the Broker boundary.
//! It never invokes a handler, starts a process, or creates a second model loop. CellRegistry
//! remains the authority for Cell budgets and durable path locks; this gate supplies the missing
//! process-local FIFO fairness and conservative in-flight footprint isolation.

use kiana_domain::{CapabilityKind, CapabilityRequest, RiskLevel, ScopeDimension};
use kiana_ports::PortError;
use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, PoisonError};
use tokio::sync::{watch, Notify};

const MAX_ACTIVE_ADMISSIONS: usize = 8;
const MAX_PENDING_ADMISSIONS: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FootprintMode {
    Read,
    Write,
    Unknown,
}

#[derive(Clone, Debug)]
struct ResourceFootprint {
    mode: FootprintMode,
    paths: Vec<String>,
}

impl ResourceFootprint {
    fn from_request(request: &CapabilityRequest) -> Self {
        let mode = match request.risk {
            RiskLevel::ReadOnly => FootprintMode::Read,
            RiskLevel::LocalWrite => FootprintMode::Write,
            RiskLevel::ExternalSideEffect | RiskLevel::Critical => FootprintMode::Unknown,
        };
        let mode = if matches!(request.capability, CapabilityKind::Network)
            || matches!(
                request.operation.as_str(),
                "connector.invoke"
                    | "connector.manage"
                    | "connector.health"
                    | "connector.mcp_handshake"
            ) {
            FootprintMode::Unknown
        } else {
            mode
        };
        let paths = match mode {
            FootprintMode::Read => request
                .execution_scope
                .as_ref()
                .and_then(|scope| match &scope.permission_scope.paths {
                    ScopeDimension::Restricted(paths) => Some(paths.as_slice()),
                    ScopeDimension::NotApplicable => None,
                })
                .map(canonical_paths)
                .unwrap_or_else(|| vec!["*".to_owned()]),
            FootprintMode::Write => request
                .execution_scope
                .as_ref()
                .and_then(|scope| {
                    if scope.write_roots.is_empty() {
                        match &scope.permission_scope.paths {
                            ScopeDimension::Restricted(paths) => Some(paths.as_slice()),
                            ScopeDimension::NotApplicable => None,
                        }
                    } else {
                        Some(scope.write_roots.as_slice())
                    }
                })
                .map(canonical_paths)
                .unwrap_or_else(|| vec!["*".to_owned()]),
            // External effects have no trustworthy filesystem footprint. Treating them as
            // exclusive is the conservative rule; a future provider-specific footprint must be
            // explicitly modeled before it can narrow this lock.
            FootprintMode::Unknown => vec!["*".to_owned()],
        };
        Self { mode, paths }
    }

    fn conflicts(&self, other: &Self) -> bool {
        if self.mode == FootprintMode::Read && other.mode == FootprintMode::Read {
            return false;
        }
        if self.mode == FootprintMode::Unknown || other.mode == FootprintMode::Unknown {
            return true;
        }
        self.paths.iter().any(|left| {
            other
                .paths
                .iter()
                .any(|right| kiana_domain::path_locks_conflict(left, right))
        })
    }
}

fn canonical_paths(paths: &[String]) -> Vec<String> {
    let mut normalized = paths
        .iter()
        .map(|path| {
            let path = path.trim();
            if path == "*" {
                "*".to_owned()
            } else {
                kiana_domain::normalize_role_path(path).unwrap_or_else(|| "*".to_owned())
            }
        })
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    if normalized.is_empty() || normalized.iter().any(|path| path == "*") {
        vec!["*".to_owned()]
    } else {
        normalized
    }
}

struct PendingAdmission {
    ticket: u64,
    footprint: ResourceFootprint,
}

struct SchedulerState {
    next_ticket: u64,
    pending: VecDeque<PendingAdmission>,
    active: HashMap<u64, ResourceFootprint>,
    quarantined: Vec<ResourceFootprint>,
}

impl Default for SchedulerState {
    fn default() -> Self {
        Self {
            next_ticket: 1,
            pending: VecDeque::new(),
            active: HashMap::new(),
            quarantined: Vec::new(),
        }
    }
}

#[derive(Default)]
pub(crate) struct CapabilityAdmissionScheduler {
    state: Mutex<SchedulerState>,
    notify: Notify,
}

pub(crate) struct AdmissionLease {
    ticket: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AdmissionOutcome {
    Known,
    Unknown,
}

impl CapabilityAdmissionScheduler {
    pub(crate) async fn acquire(
        &self,
        request: &CapabilityRequest,
        mut cancellation: watch::Receiver<bool>,
    ) -> Result<AdmissionLease, PortError> {
        if *cancellation.borrow() {
            return Err(PortError::Conflict("cancelled:before_dispatch".to_owned()));
        }
        let footprint = ResourceFootprint::from_request(request);
        let ticket = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.pending.len() >= MAX_PENDING_ADMISSIONS {
                return Err(PortError::Conflict(
                    "capability_scheduler_queue_full".to_owned(),
                ));
            }
            let ticket = state.next_ticket;
            state.next_ticket = state.next_ticket.saturating_add(1).max(1);
            state
                .pending
                .push_back(PendingAdmission { ticket, footprint });
            ticket
        };
        self.notify.notify_waiters();

        loop {
            if *cancellation.borrow() {
                self.cancel_pending(ticket);
                return Err(PortError::Conflict("cancelled:before_dispatch".to_owned()));
            }
            let notified = self.notify.notified();
            let admitted = {
                let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(index) = state.pending.iter().position(|item| item.ticket == ticket) {
                    let candidate = state.pending[index].footprint.clone();
                    let earlier_conflict = state
                        .pending
                        .iter()
                        .take(index)
                        .any(|item| item.footprint.conflicts(&candidate));
                    let active_conflict = state
                        .active
                        .values()
                        .any(|active| active.conflicts(&candidate));
                    let quarantine_conflict = state
                        .quarantined
                        .iter()
                        .any(|quarantined| quarantined.conflicts(&candidate));
                    if !earlier_conflict
                        && !active_conflict
                        && !quarantine_conflict
                        && state.active.len() < MAX_ACTIVE_ADMISSIONS
                    {
                        let pending = state.pending.remove(index).expect("candidate was present");
                        state.active.insert(ticket, pending.footprint);
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            };
            if admitted {
                return Ok(AdmissionLease { ticket });
            }
            tokio::select! {
                _ = notified => {}
                changed = cancellation.changed() => {
                    if changed.is_err() || *cancellation.borrow() {
                        self.cancel_pending(ticket);
                        return Err(PortError::Conflict("cancelled:before_dispatch".to_owned()));
                    }
                }
            }
        }
    }

    pub(crate) fn release(&self, lease: AdmissionLease, outcome: AdmissionOutcome) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(footprint) = state.active.remove(&lease.ticket) {
            if outcome == AdmissionOutcome::Unknown {
                // Unknown is not a safe release. Reconciliation/retirement owns the future
                // removal; until then the same footprint cannot be handed to a successor.
                state.quarantined.push(footprint);
            }
        }
        drop(state);
        self.notify.notify_waiters();
    }

    fn cancel_pending(&self, ticket: u64) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.pending.retain(|item| item.ticket != ticket);
        drop(state);
        self.notify.notify_waiters();
    }
}
