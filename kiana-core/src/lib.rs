//! The single command and capability control plane for Kiana.

mod approvals;
mod artifacts;
mod capabilities;
mod cell_registry;
mod collaboration;
mod commands;
mod context_query;
mod events;
mod lifecycle;
mod receipts;
mod redaction;
mod sessions;

use cell_registry::MemoryCellRegistry;
use kiana_domain::{
    builder_lock_paths, path_locks_conflict, ApprovalDecision, ApprovalId,
    AuthorizedCapabilityRequest, BudgetLease, CapabilityGrant, CapabilityGrantId, CapabilityKind,
    CapabilityRequest, CapabilityResult, CellId, CellLifecycle, CellSpec, ClosingReceipt,
    CommandIntent, CoreResponse, DecisionRecord, ExecutionStatus, GateDecision, MergeReceipt,
    PendingInvocation, PermissionProfile, PolicyDecision, RequestContext, RequestId, ReviewPacket,
    RiskLevel, RoleSpec, RunId, RuntimeEvent, SpawnPlan, SpawnPlanId, SpawnPlanStatus,
    SupervisionLease, SupervisionLeaseId, Symposium, SymposiumClaim, WorkFingerprint, WorkPacket,
    CAPABILITY_GRANT_SCHEMA, CELL_SCHEMA, DEPARTMENT_EXECUTING, DEPARTMENT_PLANNING,
    MEMORY_SEARCH_SCHEMA, REVIEW_PACKET_PATH, REVIEW_RESULT_SCHEMA, ROLE_BUILDER, ROLE_CLOSER,
    ROLE_REVIEWER, SPAWN_PLAN_SCHEMA, SUPERVISION_LEASE_SCHEMA, SYMPOSIUM_RESULT_SCHEMA,
    WORK_PACKET_PATH,
};
use kiana_gates::GateEngine;
use kiana_policy::{capability_risk_violation, PolicyEngine};
use kiana_ports::{
    AllowAllPreToolHooks, ApprovalStorePort, CapabilityBrokerPort, CapabilityLease,
    CapabilityOutcome, EventStorePort, PortError, PreToolHookDecision, PreToolHookPort, RunnerPort,
    SpawnReservationRequest,
};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
#[cfg(target_os = "linux")]
use std::ffi::{CString, OsString};
use std::fs::{self, File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
#[cfg(target_os = "linux")]
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
#[cfg(target_os = "linux")]
use std::os::unix::ffi::OsStrExt;
#[cfg(target_os = "linux")]
use std::os::unix::fs::MetadataExt;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::watch;

struct PathLockLease {
    _file: File,
}

struct BuilderPathLockGuard<'a> {
    control_plane: &'a ControlPlane,
    project_root: String,
    session_id: String,
}

impl Drop for BuilderPathLockGuard<'_> {
    fn drop(&mut self) {
        self.control_plane
            .release_builder_path_locks(&self.project_root, &self.session_id);
    }
}

pub const LEGACY_EDGES_REMAINING: usize = 9;
pub const HARNESS_ID: &str = "kiana-harness";
pub const RUN_RESULT_SCHEMA: &str = "kiana.run-result.v1";
pub const COMPACT_SCHEMA: &str = "kiana.compact.v1";
const CONTEXT_QUERY_COMMAND: &str = "context.query.v1";
const CONTEXT_REPO_MAP_OPERATION: &str = "context.repo_map";
const CONTEXT_INDEX_OPERATION: &str = "context.index.read";
const CONTEXT_INDEX_CACHE_OPERATION: &str = "context.index.cache.write";
const CONTEXT_ARTIFACTS_OPERATION: &str = "context.artifacts.read";
const CONTEXT_ARTIFACTS_CACHE_OPERATION: &str = "context.artifacts.cache.write";
const CONTEXT_ARTIFACT_STORE_OPERATION: &str = "context.artifact_store.read";
const CONTEXT_ARTIFACT_STORE_CACHE_OPERATION: &str = "context.artifact_store.cache.write";
const CONTEXT_ARTIFACT_INGEST_OPERATION: &str = "context.artifact_ingest.write";
const CONTEXT_ARTIFACT_GRAPH_OPERATION: &str = "context.artifact_graph.read";
const CONTEXT_ARTIFACT_READINESS_OPERATION: &str = "context.artifact_readiness.read";
const CONTEXT_SEARCH_OPERATION: &str = "context.search";
const CONTEXT_VECTOR_SEARCH_OPERATION: &str = "context.vector_search";
const CONTEXT_PACK_OPERATION: &str = "context.pack";
const MAX_CONTEXT_LIMIT: u64 = 1_000;
const MAX_CONTEXT_BYTES_PER_FILE: u64 = 16 * 1024 * 1024;
const MAX_CONTEXT_SNIPPET_LINES: u64 = 1_000;

#[derive(Clone, Debug)]
struct SessionBinding {
    run_id: RunId,
    actor_id: Option<String>,
    project_root: String,
    role_id: String,
    department_id: String,
}

#[derive(Clone, Copy)]
struct PersistedApprovalCursor {
    run_id: RunId,
    event_request_id: RequestId,
    event_sequence: u64,
    continuation_recorded: bool,
}

pub struct ControlPlane {
    policy: Arc<dyn PolicyEngine>,
    gates: Arc<dyn GateEngine>,
    events: Arc<dyn EventStorePort>,
    capabilities: Arc<dyn CapabilityBrokerPort>,
    approvals: Arc<dyn ApprovalStorePort>,
    runner: Arc<dyn RunnerPort>,
    pre_tool_hooks: Arc<dyn PreToolHookPort>,
    cell_registry: Arc<dyn kiana_ports::CellRegistryPort>,
    sessions: Mutex<HashMap<String, SessionBinding>>,
    pending_invocations: Mutex<HashMap<ApprovalId, PendingInvocation>>,
    cancellations: Mutex<HashMap<RunId, watch::Sender<bool>>>,
    path_locks: Mutex<HashMap<String, String>>,
    durable_path_locks: Mutex<HashMap<String, Vec<PathLockLease>>>,
}

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error(transparent)]
    Domain(#[from] kiana_domain::DomainError),
    #[error(transparent)]
    Port(#[from] PortError),
}
