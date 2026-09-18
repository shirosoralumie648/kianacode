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
    canonical_journal_bytes, json_digest, redact_value, AuthorizedCapabilityRequest,
    CapabilityErrorCode, CapabilityKind, CapabilityResult, ModelDelta, ModelOutput, ModelRequest,
    ModelToolCall, ModelUsage,
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
