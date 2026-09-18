//! Deterministic, isolated evaluation runtime boundary.
//!
//! This module is an adapter for CI/evaluation fixtures only.  It creates an owned temporary
//! workspace and KIANA_HOME, supplies a fixed clock observation and deterministic bytes, and does
//! not start a runner, scheduler, provider or capability loop.  Callers must still route any
//! target through the normal DaemonHost/ControlPlane composition.

use super::DaemonHost;
use async_trait::async_trait;
use kiana_core::ControlPlane;
use kiana_domain::ClockObservation;
use kiana_domain::{
    canonical_journal_bytes, json_digest, redact_text, redact_value, AuthorizedCapabilityRequest,
    CapabilityErrorCode, CapabilityKind, CapabilityResult, CommandReceipt, ModelDelta, ModelOutput,
    ModelRequest, ModelToolCall, ModelUsage, RunId, RuntimeEvent,
};
use kiana_ports::{CapabilityBrokerPort, FixtureStore, ModelClient, PortError};
use kiana_protocol::{RequestEnvelope, ResponseEnvelope};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

pub const EVAL_RUNTIME_SCHEMA: &str = "kiana.eval-runtime.v1";
pub const EVAL_RUNTIME_ENV_KIANA_HOME: &str = "KIANA_HOME";
pub const EVAL_RUNTIME_ENV_HOME: &str = "HOME";
pub const EVAL_TARGET_SPINE_SCHEMA: &str = "kiana.eval-target-spine.v1";

static PROCESS_ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn env_lock() -> &'static Mutex<()> {
    PROCESS_ENV_LOCK.get_or_init(|| Mutex::new(()))
}

fn bounded_label(label: &str) -> bool {
    !label.trim().is_empty()
        && label.len() <= 64
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

/// Owned evaluation sandbox.  The root is created with `create_dir`, never reused or recursively
/// cleaned unless this instance created it successfully.
pub struct EvalRuntimeSandbox {
    root: PathBuf,
    workspace: PathBuf,
    kiana_home: PathBuf,
    clock: ClockObservation,
    random_seed: u64,
}

impl EvalRuntimeSandbox {
    pub fn create(
        label: &str,
        wall_now_unix_ms: u64,
        monotonic_now_ms: u64,
        random_seed: u64,
    ) -> io::Result<Self> {
        if !bounded_label(label) || wall_now_unix_ms == 0 || monotonic_now_ms == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "eval_runtime_spec_invalid",
            ));
        }
        let root = std::env::temp_dir().join(format!(
            "kiana-eval-{label}-{}-{random_seed:016x}",
            std::process::id()
        ));
        fs::create_dir(&root)?;
        let workspace = root.join("workspace");
        let kiana_home = root.join("kiana-home");
        fs::create_dir(&workspace)?;
        fs::create_dir(&kiana_home)?;
        let clock = ClockObservation::observe(
            "eval-fixed",
            u128::from(wall_now_unix_ms),
            u128::from(monotonic_now_ms),
            None,
            1,
        )
        .map_err(io::Error::other)?;
        Ok(Self {
            root,
            workspace,
            kiana_home,
            clock,
            random_seed,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    pub fn kiana_home(&self) -> &Path {
        &self.kiana_home
    }

    pub fn clock(&self) -> &ClockObservation {
        &self.clock
    }

    pub fn random_seed(&self) -> u64 {
        self.random_seed
    }

    /// Explicit environment passed to an eval target.  It never reads the operator's home.
    pub fn environment(&self) -> BTreeMap<String, String> {
        BTreeMap::from([
            (
                EVAL_RUNTIME_ENV_KIANA_HOME.to_owned(),
                self.kiana_home.display().to_string(),
            ),
            (
                EVAL_RUNTIME_ENV_HOME.to_owned(),
                self.root.join("home").display().to_string(),
            ),
        ])
    }

    /// Run a bounded fixture callback with the process environment redirected to this sandbox.
    /// The mutex prevents concurrent CI fixtures from crossing their KIANA_HOME boundaries, and
    /// every original value is restored even when the callback returns an error.
    pub fn with_process_environment<T, F>(&self, callback: F) -> io::Result<T>
    where
        F: FnOnce() -> io::Result<T>,
    {
        let _guard = env_lock()
            .lock()
            .map_err(|_| io::Error::other("eval_runtime_env_lock_poisoned"))?;
        let home = self.root.join("home");
        fs::create_dir_all(&home)?;
        let previous_kiana_home = std::env::var_os(EVAL_RUNTIME_ENV_KIANA_HOME);
        let previous_home = std::env::var_os(EVAL_RUNTIME_ENV_HOME);
        std::env::set_var(EVAL_RUNTIME_ENV_KIANA_HOME, &self.kiana_home);
        std::env::set_var(EVAL_RUNTIME_ENV_HOME, &home);
        let result = callback();
        match previous_kiana_home {
            Some(value) => std::env::set_var(EVAL_RUNTIME_ENV_KIANA_HOME, value),
            None => std::env::remove_var(EVAL_RUNTIME_ENV_KIANA_HOME),
        }
        match previous_home {
            Some(value) => std::env::set_var(EVAL_RUNTIME_ENV_HOME, value),
            None => std::env::remove_var(EVAL_RUNTIME_ENV_HOME),
        }
        result
    }

    /// Deterministic pseudo-random bytes for fixture IDs/nonces.  This is not a security random
    /// source and is intentionally unsuitable for production credentials.
    pub fn deterministic_bytes(&self, label: &str, length: usize) -> io::Result<Vec<u8>> {
        if !bounded_label(label) || length > 64 * 1024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "eval_runtime_seed_request_invalid",
            ));
        }
        let mut state = self.random_seed ^ fnv1a(label.as_bytes());
        let mut bytes = Vec::with_capacity(length);
        while bytes.len() < length {
            state = state.wrapping_add(0x9e37_79b9_7f4a_7c15).rotate_left(17) ^ (state >> 11);
            bytes.extend_from_slice(&state.to_le_bytes());
        }
        bytes.truncate(length);
        Ok(bytes)
    }
}

impl Drop for EvalRuntimeSandbox {
    fn drop(&mut self) {
        // Exact, self-created temp root only; failure is intentionally not turned into a fake
        // evaluation result because cleanup is an infrastructure concern recorded by the caller.
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub const EVAL_INITIAL_STATE_SCHEMA: &str = "kiana.eval-initial-state.v1";
const MAX_INITIAL_STATE_FIXTURE_BYTES: usize = 128 * 1024;

/// A strict, digest-bound collection of evaluation inputs. Values are snapshots only; they are
/// not policy authority, role grants or durable EventLog facts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalInitialStateBundle {
    pub schema: String,
    pub experiment_id: String,
    pub policy_snapshot: Value,
    pub role_assignment: Value,
    pub memory_fixture: Value,
    pub workflow_fixture: Value,
    pub artifact_fixture: Value,
    pub initial_state_digest: String,
}

impl EvalInitialStateBundle {
    pub fn new(
        experiment_id: impl Into<String>,
        policy_snapshot: Value,
        role_assignment: Value,
        memory_fixture: Value,
        workflow_fixture: Value,
        artifact_fixture: Value,
    ) -> Result<Self, String> {
        let mut bundle = Self {
            schema: EVAL_INITIAL_STATE_SCHEMA.to_owned(),
            experiment_id: experiment_id.into(),
            policy_snapshot,
            role_assignment,
            memory_fixture,
            workflow_fixture,
            artifact_fixture,
            initial_state_digest: String::new(),
        };
        bundle.initial_state_digest = bundle.digest();
        bundle.validate()?;
        Ok(bundle)
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "experiment_id": self.experiment_id,
            "policy_snapshot": self.policy_snapshot,
            "role_assignment": self.role_assignment,
            "memory_fixture": self.memory_fixture,
            "workflow_fixture": self.workflow_fixture,
            "artifact_fixture": self.artifact_fixture,
        }))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EVAL_INITIAL_STATE_SCHEMA || !bounded_label(&self.experiment_id) {
            return Err("eval_initial_state_header_invalid".to_owned());
        }
        for (field, value) in [
            ("policy_snapshot", &self.policy_snapshot),
            ("role_assignment", &self.role_assignment),
            ("memory_fixture", &self.memory_fixture),
            ("workflow_fixture", &self.workflow_fixture),
            ("artifact_fixture", &self.artifact_fixture),
        ] {
            let encoded = canonical_journal_bytes(value)
                .map_err(|_| format!("eval_initial_state_{field}_encoding_invalid"))?;
            if !value.is_object() {
                return Err(format!("eval_initial_state_{field}_must_be_object"));
            }
            if encoded.len() > MAX_INITIAL_STATE_FIXTURE_BYTES {
                return Err(format!("eval_initial_state_{field}_too_large"));
            }
            if redact_value(value) != *value {
                return Err(format!("eval_initial_state_{field}_secret_detected"));
            }
        }
        if self.initial_state_digest != self.digest()
            || !self.initial_state_digest.starts_with("sha256:")
        {
            return Err("eval_initial_state_digest_mismatch".to_owned());
        }
        Ok(())
    }

    fn fixture_values(&self) -> [(String, Value); 6] {
        [
            (
                "initial_state".to_owned(),
                json!({
                    "schema": self.schema,
                    "experiment_id": self.experiment_id,
                    "policy_snapshot": self.policy_snapshot,
                    "role_assignment": self.role_assignment,
                    "memory_fixture": self.memory_fixture,
                    "workflow_fixture": self.workflow_fixture,
                    "artifact_fixture": self.artifact_fixture,
                    "initial_state_digest": self.initial_state_digest,
                }),
            ),
            ("policy_snapshot".to_owned(), self.policy_snapshot.clone()),
            ("role_assignment".to_owned(), self.role_assignment.clone()),
            ("memory_fixture".to_owned(), self.memory_fixture.clone()),
            ("workflow_fixture".to_owned(), self.workflow_fixture.clone()),
            ("artifact_fixture".to_owned(), self.artifact_fixture.clone()),
        ]
    }
}

/// In-memory controlled fixture store for one experiment scope. It exposes only opaque fixture
/// names and digest-bound bytes; it never opens paths or mutates canonical project state.
pub struct EvalInitialStateStore {
    scope_digest: String,
    fixtures: BTreeMap<String, Vec<u8>>,
}

impl EvalInitialStateStore {
    pub fn from_bundle(bundle: &EvalInitialStateBundle) -> Result<Self, String> {
        bundle.validate()?;
        let mut fixtures = BTreeMap::new();
        for (name, value) in bundle.fixture_values() {
            let bytes = canonical_journal_bytes(&value)?;
            fixtures.insert(name, bytes);
        }
        Ok(Self {
            scope_digest: bundle.initial_state_digest.clone(),
            fixtures,
        })
    }

    pub fn scope_digest(&self) -> &str {
        &self.scope_digest
    }

    pub fn fixture_names(&self) -> Vec<String> {
        self.fixtures.keys().cloned().collect()
    }
}

#[async_trait]
impl FixtureStore for EvalInitialStateStore {
    async fn read_fixture(
        &self,
        fixture_ref: &str,
        scope_digest: &str,
    ) -> Result<Vec<u8>, PortError> {
        if fixture_ref.trim().is_empty()
            || fixture_ref.len() > 64
            || fixture_ref.contains(['/', '\\', '\0'])
        {
            return Err(PortError::Failed("eval_fixture_ref_invalid".to_owned()));
        }
        if scope_digest != self.scope_digest {
            return Err(PortError::Failed("eval_fixture_scope_mismatch".to_owned()));
        }
        self.fixtures
            .get(fixture_ref)
            .cloned()
            .ok_or_else(|| PortError::Unavailable("eval_fixture_not_found".to_owned()))
    }
}

pub const EVAL_EVIDENCE_CAPTURE_SCHEMA: &str = "kiana.eval-evidence-capture.v1";
const MAX_CAPTURE_EVENTS: usize = 4_096;
const MAX_CAPTURE_REFS: usize = 512;
const MAX_BOUNDARY_PROCESSES: usize = 2_048;
const MAX_BOUNDARY_FILES: usize = 8_192;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvalFileChangeKind {
    Added,
    Modified,
    Deleted,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalProcessObservation {
    pub pid: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_pid: Option<u32>,
    pub command_digest: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalFileDiff {
    pub path: String,
    pub kind: EvalFileChangeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_digest: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalBoundaryEvidence {
    pub schema: String,
    pub process_tree: Vec<EvalProcessObservation>,
    pub file_diff: Vec<EvalFileDiff>,
    pub network_syscalls: u64,
    pub secret_patterns: Vec<String>,
    pub evidence_digest: String,
}

impl EvalBoundaryEvidence {
    pub fn new(
        process_tree: Vec<EvalProcessObservation>,
        file_diff: Vec<EvalFileDiff>,
        network_syscalls: u64,
        secret_patterns: Vec<String>,
    ) -> Result<Self, String> {
        let mut evidence = Self {
            schema: EVAL_EVIDENCE_CAPTURE_SCHEMA.to_owned(),
            process_tree,
            file_diff,
            network_syscalls,
            secret_patterns,
            evidence_digest: String::new(),
        };
        evidence.evidence_digest = evidence.digest();
        evidence.validate()?;
        Ok(evidence)
    }

    fn valid_digest(value: &str) -> bool {
        value.len() == 71
            && value.starts_with("sha256:")
            && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EVAL_EVIDENCE_CAPTURE_SCHEMA
            || self.process_tree.len() > MAX_BOUNDARY_PROCESSES
            || self.file_diff.len() > MAX_BOUNDARY_FILES
            || self.secret_patterns.len() > MAX_CAPTURE_REFS
            || !Self::valid_digest(&self.evidence_digest)
            || self.evidence_digest != self.digest()
        {
            return Err("eval_boundary_evidence_header_invalid".to_owned());
        }
        for process in &self.process_tree {
            if process.pid == 0
                || process.parent_pid == Some(0)
                || !Self::valid_digest(&process.command_digest)
            {
                return Err("eval_boundary_process_invalid".to_owned());
            }
        }
        for change in &self.file_diff {
            if change.path.trim().is_empty()
                || change.path.len() > 4_096
                || change.path.starts_with('/')
                || change.path.contains(['\\', '\0'])
                || change
                    .path
                    .split('/')
                    .any(|part| part.is_empty() || part == "." || part == "..")
                || matches!(change.kind, EvalFileChangeKind::Added) && change.after_digest.is_none()
                || matches!(change.kind, EvalFileChangeKind::Deleted)
                    && change.before_digest.is_none()
                || change
                    .before_digest
                    .as_deref()
                    .is_some_and(|digest| !Self::valid_digest(digest))
                || change
                    .after_digest
                    .as_deref()
                    .is_some_and(|digest| !Self::valid_digest(digest))
            {
                return Err("eval_boundary_file_diff_invalid".to_owned());
            }
        }
        for pattern in &self.secret_patterns {
            if !matches!(
                pattern.as_str(),
                "api_key" | "authorization" | "password" | "private_key" | "token"
            ) {
                return Err("eval_boundary_secret_pattern_unknown".to_owned());
            }
        }
        if self
            .secret_patterns
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            return Err("eval_boundary_secret_pattern_order".to_owned());
        }
        Ok(())
    }

    pub fn is_safe(&self) -> bool {
        self.network_syscalls == 0 && self.secret_patterns.is_empty()
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "process_tree": self.process_tree,
            "file_diff": self.file_diff,
            "network_syscalls": self.network_syscalls,
            "secret_patterns": self.secret_patterns,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvalCaptureStatus {
    Open,
    Flushed,
    InfraUnknown,
    SafetyViolation,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalCaptureReceipt {
    pub schema: String,
    pub run_id: RunId,
    pub status: EvalCaptureStatus,
    pub event_count: usize,
    pub invocation_ref_count: usize,
    pub artifact_ref_count: usize,
    pub receipt_ref_count: usize,
    pub source_cursor: u64,
    pub capture_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_evidence_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_code: Option<String>,
}

impl EvalCaptureReceipt {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EVAL_EVIDENCE_CAPTURE_SCHEMA
            || self.run_id.as_uuid().is_nil()
            || self.capture_digest.len() != 71
            || !self.capture_digest.starts_with("sha256:")
            || (self.status == EvalCaptureStatus::Flushed && self.failure_code.is_some())
            || (self.status == EvalCaptureStatus::InfraUnknown
                && self.failure_code.as_deref() != Some("infra_flush_unknown"))
            || (self.status == EvalCaptureStatus::SafetyViolation
                && self.failure_code.as_deref() != Some("safety_violation"))
            || self
                .safety_evidence_digest
                .as_deref()
                .is_some_and(|digest| !EvalBoundaryEvidence::valid_digest(digest))
        {
            return Err("eval_capture_receipt_invalid".to_owned());
        }
        if self.event_count > MAX_CAPTURE_EVENTS
            || self.invocation_ref_count > MAX_CAPTURE_REFS
            || self.artifact_ref_count > MAX_CAPTURE_REFS
            || self.receipt_ref_count > MAX_CAPTURE_REFS
        {
            return Err("eval_capture_receipt_limit".to_owned());
        }
        Ok(())
    }
}

/// Bounded evidence collector for one evaluation target. It stores committed event values and
/// opaque references only; it never becomes a second EventLog or receipt authority.
pub struct EvalEvidenceCapture {
    run_id: RunId,
    events: Vec<RuntimeEvent>,
    invocation_refs: Vec<String>,
    artifact_refs: Vec<String>,
    receipt_refs: Vec<String>,
    command_receipt: Option<CommandReceipt>,
    safety_evidence: Option<EvalBoundaryEvidence>,
    status: EvalCaptureStatus,
    failure_code: Option<String>,
}

impl EvalEvidenceCapture {
    pub fn new(run_id: RunId) -> Result<Self, String> {
        if run_id.as_uuid().is_nil() {
            return Err("eval_capture_run_id_invalid".to_owned());
        }
        Ok(Self {
            run_id,
            events: Vec::new(),
            invocation_refs: Vec::new(),
            artifact_refs: Vec::new(),
            receipt_refs: Vec::new(),
            command_receipt: None,
            safety_evidence: None,
            status: EvalCaptureStatus::Open,
            failure_code: None,
        })
    }

    fn ensure_open(&self) -> Result<(), String> {
        if self.status != EvalCaptureStatus::Open {
            Err("eval_capture_not_open".to_owned())
        } else {
            Ok(())
        }
    }

    fn check_reference(reference: &str, field: &str) -> Result<(), String> {
        if reference.trim().is_empty()
            || reference.len() > 4_096
            || reference.contains('\0')
            || redact_text(reference) != reference
        {
            return Err(format!("eval_capture_{field}_invalid"));
        }
        Ok(())
    }

    fn insert_reference(
        references: &mut Vec<String>,
        reference: impl Into<String>,
        field: &str,
    ) -> Result<(), String> {
        let reference = reference.into();
        Self::check_reference(&reference, field)?;
        if references.len() >= MAX_CAPTURE_REFS {
            return Err(format!("eval_capture_{field}_limit"));
        }
        if !references.contains(&reference) {
            references.push(reference);
        }
        Ok(())
    }

    pub fn record_event(&mut self, event: RuntimeEvent) -> Result<(), String> {
        self.ensure_open()?;
        if self.events.len() >= MAX_CAPTURE_EVENTS
            || self
                .events
                .iter()
                .any(|prior| prior.event_id == event.event_id)
            || event.sequence == 0
            || event.kind.trim().is_empty()
            || redact_value(&event.data) != event.data
        {
            return Err("eval_capture_event_invalid".to_owned());
        }
        if canonical_journal_bytes(&event)
            .map_err(|_| "eval_capture_event_encode_invalid".to_owned())?
            .len()
            > 256 * 1024
        {
            return Err("eval_capture_event_too_large".to_owned());
        }
        for reference in &event.artifact_refs {
            Self::insert_reference(&mut self.artifact_refs, reference.clone(), "artifact_ref")?;
        }
        self.events.push(event);
        Ok(())
    }

    pub fn record_invocation_ref(&mut self, reference: impl Into<String>) -> Result<(), String> {
        self.ensure_open()?;
        Self::insert_reference(&mut self.invocation_refs, reference, "invocation_ref")
    }

    pub fn record_artifact_ref(&mut self, reference: impl Into<String>) -> Result<(), String> {
        self.ensure_open()?;
        Self::insert_reference(&mut self.artifact_refs, reference, "artifact_ref")
    }

    pub fn record_receipt_ref(&mut self, reference: impl Into<String>) -> Result<(), String> {
        self.ensure_open()?;
        Self::insert_reference(&mut self.receipt_refs, reference, "receipt_ref")
    }

    pub fn record_command_receipt(&mut self, receipt: CommandReceipt) -> Result<(), String> {
        self.ensure_open()?;
        if receipt.first_cursor == 0
            || receipt.cursor < receipt.first_cursor
            || receipt.event_ids.is_empty()
            || receipt.command_digest.len() != 64
            || !receipt
                .command_digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || canonical_journal_bytes(&receipt)
                .map_err(|_| "eval_capture_receipt_encode_invalid".to_owned())?
                .len()
                > 128 * 1024
        {
            return Err("eval_capture_command_receipt_invalid".to_owned());
        }
        self.command_receipt = Some(receipt);
        Ok(())
    }

    pub fn attach_safety_evidence(&mut self, evidence: EvalBoundaryEvidence) -> Result<(), String> {
        self.ensure_open()?;
        evidence.validate()?;
        self.safety_evidence = Some(evidence);
        Ok(())
    }

    pub fn finish(&mut self, flush: Result<(), String>) -> EvalCaptureReceipt {
        self.status = if flush.is_err() {
            self.failure_code = Some("infra_flush_unknown".to_owned());
            EvalCaptureStatus::InfraUnknown
        } else if self
            .safety_evidence
            .as_ref()
            .is_some_and(|evidence| !evidence.is_safe())
        {
            self.failure_code = Some("safety_violation".to_owned());
            EvalCaptureStatus::SafetyViolation
        } else {
            EvalCaptureStatus::Flushed
        };
        let receipt = EvalCaptureReceipt {
            schema: EVAL_EVIDENCE_CAPTURE_SCHEMA.to_owned(),
            run_id: self.run_id,
            status: self.status,
            event_count: self.events.len(),
            invocation_ref_count: self.invocation_refs.len(),
            artifact_ref_count: self.artifact_refs.len(),
            receipt_ref_count: self.receipt_refs.len()
                + usize::from(self.command_receipt.is_some()),
            source_cursor: self
                .events
                .iter()
                .map(|event| event.sequence)
                .max()
                .unwrap_or_default(),
            capture_digest: self.digest(),
            safety_evidence_digest: self
                .safety_evidence
                .as_ref()
                .map(|evidence| evidence.evidence_digest.clone()),
            failure_code: self.failure_code.clone(),
        };
        debug_assert!(receipt.validate().is_ok());
        receipt
    }

    pub fn status(&self) -> EvalCaptureStatus {
        self.status
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": EVAL_EVIDENCE_CAPTURE_SCHEMA,
            "run_id": self.run_id,
            "events": self.events,
            "invocation_refs": self.invocation_refs,
            "artifact_refs": self.artifact_refs,
            "receipt_refs": self.receipt_refs,
            "command_receipt": self.command_receipt,
            "safety_evidence": self.safety_evidence,
            "status": self.status,
            "failure_code": self.failure_code,
        }))
    }
}

pub const EVAL_FAULT_PLAN_SCHEMA: &str = "kiana.eval-fault-plan.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvalFaultKind {
    ApprovalDenied,
    ApprovalExpired,
    CancelRace,
    CrashAfterEffect,
    Restart,
    StaleLease,
    ResultUnknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvalFaultDisposition {
    Denied,
    CancelledNotStarted,
    UnknownReconcile,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalFaultPlan {
    pub schema: String,
    pub kind: EvalFaultKind,
    pub effect_started: bool,
    pub stop_confirmed: bool,
    pub lease_epoch: u64,
    pub observed_lease_epoch: u64,
    pub restart_generation: u64,
    pub plan_digest: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalFaultEvidence {
    pub schema: String,
    pub plan_digest: String,
    pub kind: EvalFaultKind,
    pub disposition: EvalFaultDisposition,
    pub effect_started: bool,
    pub effect_known: bool,
    pub stop_confirmed: bool,
    pub requires_reconciliation: bool,
    pub terminal_code: String,
    pub evidence_digest: String,
}

impl EvalFaultPlan {
    pub fn new(kind: EvalFaultKind) -> Self {
        let mut plan = Self {
            schema: EVAL_FAULT_PLAN_SCHEMA.to_owned(),
            kind,
            effect_started: false,
            stop_confirmed: true,
            lease_epoch: 1,
            observed_lease_epoch: 1,
            restart_generation: 1,
            plan_digest: String::new(),
        };
        plan.plan_digest = plan.digest();
        plan
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "kind": self.kind,
            "effect_started": self.effect_started,
            "stop_confirmed": self.stop_confirmed,
            "lease_epoch": self.lease_epoch,
            "observed_lease_epoch": self.observed_lease_epoch,
            "restart_generation": self.restart_generation,
        }))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EVAL_FAULT_PLAN_SCHEMA
            || self.lease_epoch == 0
            || self.observed_lease_epoch == 0
            || self.restart_generation == 0
            || self.plan_digest != self.digest()
            || !self.plan_digest.starts_with("sha256:")
        {
            return Err("eval_fault_plan_header_invalid".to_owned());
        }
        match self.kind {
            EvalFaultKind::ApprovalDenied | EvalFaultKind::ApprovalExpired => {
                if self.effect_started {
                    return Err("eval_fault_approval_effect_started".to_owned());
                }
            }
            EvalFaultKind::CrashAfterEffect => {
                if !self.effect_started {
                    return Err("eval_fault_crash_effect_missing".to_owned());
                }
            }
            EvalFaultKind::StaleLease => {
                if self.lease_epoch == self.observed_lease_epoch || self.effect_started {
                    return Err("eval_fault_lease_not_stale".to_owned());
                }
            }
            EvalFaultKind::CancelRace | EvalFaultKind::Restart | EvalFaultKind::ResultUnknown => {}
        }
        Ok(())
    }

    pub fn apply(&self) -> Result<EvalFaultEvidence, String> {
        self.validate()?;
        let (disposition, effect_known, requires_reconciliation, terminal_code) = match self.kind {
            EvalFaultKind::ApprovalDenied => {
                (EvalFaultDisposition::Denied, true, false, "approval_denied")
            }
            EvalFaultKind::ApprovalExpired => (
                EvalFaultDisposition::Denied,
                true,
                false,
                "approval_expired",
            ),
            EvalFaultKind::StaleLease => (EvalFaultDisposition::Denied, true, false, "stale_lease"),
            EvalFaultKind::CancelRace if !self.effect_started && self.stop_confirmed => (
                EvalFaultDisposition::CancelledNotStarted,
                true,
                false,
                "cancelled_not_started",
            ),
            EvalFaultKind::CancelRace => (
                EvalFaultDisposition::UnknownReconcile,
                false,
                true,
                "cancel_race_unknown",
            ),
            EvalFaultKind::CrashAfterEffect => (
                EvalFaultDisposition::UnknownReconcile,
                false,
                true,
                "crash_after_effect_unknown",
            ),
            EvalFaultKind::Restart => (
                EvalFaultDisposition::UnknownReconcile,
                false,
                true,
                "restart_reconcile_required",
            ),
            EvalFaultKind::ResultUnknown => (
                EvalFaultDisposition::UnknownReconcile,
                false,
                true,
                "result_unknown",
            ),
        };
        let mut evidence = EvalFaultEvidence {
            schema: EVAL_FAULT_PLAN_SCHEMA.to_owned(),
            plan_digest: self.plan_digest.clone(),
            kind: self.kind,
            disposition,
            effect_started: self.effect_started,
            effect_known,
            stop_confirmed: self.stop_confirmed,
            requires_reconciliation,
            terminal_code: terminal_code.to_owned(),
            evidence_digest: String::new(),
        };
        evidence.evidence_digest = json_digest(&evidence_without_digest(&evidence));
        Ok(evidence)
    }
}

fn evidence_without_digest(evidence: &EvalFaultEvidence) -> Value {
    json!({
        "schema": evidence.schema,
        "plan_digest": evidence.plan_digest,
        "kind": evidence.kind,
        "disposition": evidence.disposition,
        "effect_started": evidence.effect_started,
        "effect_known": evidence.effect_known,
        "stop_confirmed": evidence.stop_confirmed,
        "requires_reconciliation": evidence.requires_reconciliation,
        "terminal_code": evidence.terminal_code,
    })
}

/// Evaluation target wrapper that delegates every protocol request to the existing DaemonHost.
/// It owns no runner loop, policy decision or capability dispatch path of its own.
pub struct EvalTarget {
    host: DaemonHost,
    sandbox: EvalRuntimeSandbox,
}

impl EvalTarget {
    pub fn new(host: DaemonHost, sandbox: EvalRuntimeSandbox) -> Self {
        Self { host, sandbox }
    }

    pub fn from_control_plane(core: Arc<ControlPlane>, sandbox: EvalRuntimeSandbox) -> Self {
        Self::new(DaemonHost::new(core), sandbox)
    }

    pub fn host(&self) -> &DaemonHost {
        &self.host
    }

    pub fn sandbox(&self) -> &EvalRuntimeSandbox {
        &self.sandbox
    }

    pub async fn handle(&self, request: RequestEnvelope) -> ResponseEnvelope {
        self.host.handle(request).await
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x1000_0000_01b3)
    })
}

/// Deterministic provider cassette scenarios used by EQ-10+.  They are local values only; no
/// endpoint, credential or network field exists in this adapter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum FakeProviderScenario {
    Complete {
        text: String,
    },
    Stream {
        chunks: Vec<String>,
    },
    ToolCall {
        id: String,
        name: String,
        arguments: Value,
    },
    Malformed {
        reason: String,
    },
    Error {
        code: String,
    },
}

impl FakeProviderScenario {
    pub fn validate(&self) -> Result<(), String> {
        fn bounded(value: &str, max: usize) -> bool {
            !value.trim().is_empty() && value.len() <= max && !value.contains(['\0', '\r', '\n'])
        }
        match self {
            Self::Complete { text } => {
                if text.len() > 64 * 1024 {
                    return Err("fake_provider_text_too_large".to_owned());
                }
            }
            Self::Stream { chunks } => {
                if chunks.is_empty()
                    || chunks.len() > 256
                    || chunks.iter().any(|chunk| chunk.len() > 16 * 1024)
                {
                    return Err("fake_provider_stream_invalid".to_owned());
                }
            }
            Self::ToolCall {
                id,
                name,
                arguments,
            } => {
                if !bounded(id, 128)
                    || !bounded(name, 128)
                    || !arguments.is_object()
                    || serde_json::to_vec(arguments).map_or(true, |bytes| bytes.len() > 64 * 1024)
                {
                    return Err("fake_provider_tool_call_invalid".to_owned());
                }
            }
            Self::Malformed { reason } => {
                if !bounded(reason, 256) {
                    return Err("fake_provider_malformed_invalid".to_owned());
                }
            }
            Self::Error { code } => {
                if !bounded(code, 128) {
                    return Err("fake_provider_error_invalid".to_owned());
                }
            }
        }
        Ok(())
    }
}

/// A provider-port fake that is deterministic and explicitly offline.  It records call count so
/// CI can assert replay/stream fixtures do not make hidden retries or network calls.
pub struct FakeProviderAdapter {
    scenario: FakeProviderScenario,
    calls: AtomicUsize,
}

impl FakeProviderAdapter {
    pub fn new(scenario: FakeProviderScenario) -> Result<Self, String> {
        scenario.validate()?;
        Ok(Self {
            scenario,
            calls: AtomicUsize::new(0),
        })
    }

    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn output(&self) -> Result<ModelOutput, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match &self.scenario {
            FakeProviderScenario::Complete { text } => Ok(ModelOutput {
                text: text.clone(),
                stop_reason: Some("end_turn".to_owned()),
                usage: Some(ModelUsage {
                    input_tokens: 1,
                    output_tokens: text.len() as u64,
                }),
                ..ModelOutput::default()
            }),
            FakeProviderScenario::Stream { chunks } => Ok(ModelOutput {
                text: chunks.concat(),
                stop_reason: Some("end_turn".to_owned()),
                ..ModelOutput::default()
            }),
            FakeProviderScenario::ToolCall {
                id,
                name,
                arguments,
            } => Ok(ModelOutput {
                tool_calls: vec![ModelToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    arguments: arguments.clone(),
                }],
                stop_reason: Some("tool_use".to_owned()),
                ..ModelOutput::default()
            }),
            FakeProviderScenario::Malformed { reason } => {
                Err(format!("fake_provider_malformed:{reason}"))
            }
            FakeProviderScenario::Error { code } => Err(format!("fake_provider_error:{code}")),
        }
    }
}

#[async_trait]
impl ModelClient for FakeProviderAdapter {
    async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, String> {
        self.output()
    }

    async fn complete_streaming(
        &self,
        _request: ModelRequest,
        on_delta: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
    ) -> Result<ModelOutput, String> {
        let output = self.output()?;
        if let FakeProviderScenario::Stream { chunks } = &self.scenario {
            for chunk in chunks {
                on_delta(ModelDelta::Text {
                    text: chunk.clone(),
                })?;
            }
        }
        if let FakeProviderScenario::ToolCall {
            id,
            name,
            arguments,
        } = &self.scenario
        {
            on_delta(ModelDelta::ToolArguments {
                index: 0,
                id: id.clone(),
                name: name.clone(),
                partial_json: serde_json::to_string(arguments)
                    .map_err(|_| "fake_provider_tool_arguments_encode".to_owned())?,
            })?;
        }
        Ok(output)
    }
}

pub const EVAL_DENY_BROKER_SCHEMA: &str = "kiana.eval-deny-broker.v1";

/// Evaluation-only broker boundary. It accepts the same authorized request shape as the
/// production broker but never registers or invokes an executor; every capability is a known,
/// not-executed permission denial with a stable effect category.
pub struct DenyByDefaultEvalBroker {
    denied_calls: AtomicUsize,
}

impl Default for DenyByDefaultEvalBroker {
    fn default() -> Self {
        Self::new()
    }
}

impl DenyByDefaultEvalBroker {
    pub fn new() -> Self {
        Self {
            denied_calls: AtomicUsize::new(0),
        }
    }

    pub fn denied_calls(&self) -> usize {
        self.denied_calls.load(Ordering::SeqCst)
    }

    fn effect_category(request: &AuthorizedCapabilityRequest) -> &'static str {
        let operation = request.request.operation.as_str();
        let normalized = operation.to_ascii_lowercase();
        if normalized.starts_with("mcp.") {
            "mcp"
        } else if normalized.contains("payment")
            || normalized.contains("charge")
            || normalized.contains("billing")
        {
            "payment"
        } else if normalized.contains("publish")
            || normalized.contains("release")
            || normalized.contains("deploy")
        {
            "publish"
        } else {
            match &request.request.capability {
                CapabilityKind::Network => "network",
                CapabilityKind::Secret => "secret",
                CapabilityKind::Computer => "desktop",
                _ => "unknown",
            }
        }
    }

    fn denied_result(&self, request: &AuthorizedCapabilityRequest) -> CapabilityResult {
        self.denied_calls.fetch_add(1, Ordering::SeqCst);
        let category = Self::effect_category(request);
        let mut result = CapabilityResult::failure_with_code(
            request.request.request_id,
            CapabilityErrorCode::PermissionDenied,
            Some(&format!("eval_effect_denied:{category}")),
        );
        result.output["not_executed"] = serde_json::json!(true);
        result.output["effect_started"] = serde_json::json!(false);
        result.output["effect_known"] = serde_json::json!(true);
        result.output["stop_confirmed"] = serde_json::json!(true);
        result
    }
}

#[async_trait]
impl CapabilityBrokerPort for DenyByDefaultEvalBroker {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        Ok(self.denied_result(&request))
    }

    async fn execute_cancellable(
        &self,
        request: AuthorizedCapabilityRequest,
        _cancellation: tokio::sync::watch::Receiver<bool>,
    ) -> Result<CapabilityResult, PortError> {
        // A deny-only adapter has no in-flight effect to stop; cancellation cannot turn a deny
        // into an Unknown result or accidentally delegate to a real executor.
        Ok(self.denied_result(&request))
    }
}
