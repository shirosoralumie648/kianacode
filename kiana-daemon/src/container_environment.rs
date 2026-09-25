//! Controlled OCI/gVisor execution environment.
//!
//! This adapter is deliberately narrower than a general container client. It accepts a
//! server-owned immutable image and workspace, creates one labelled non-root container, and
//! refuses to execute or dispose an object whose owner/scope/plan labels do not match. The
//! adapter never exposes a runtime socket, host credentials, or a host fallback to callers.
//!
//! The generic EnvironmentPort remains the lifecycle seam. A future shell/MCP adapter can use
//! ContainerEnvironmentAdapter::execute_argv after the control plane has selected this backend;
//! this module itself does not create a second model or capability loop.

use async_trait::async_trait;
use kiana_ports::{
    EnvironmentEffectReceipt, EnvironmentPlan, EnvironmentPort, EnvironmentProbe, EnvironmentScope,
    PortError, PreparedEnvironment,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::process::Command;

pub const CONTAINER_BACKEND: &str = "oci-container";
pub const DEFAULT_CONTAINER_RUNTIME: &str = "docker";
pub const DEFAULT_CONTAINER_USER: &str = "65532:65532";
pub const DEFAULT_CONTAINER_NETWORK: &str = "none";
pub const DEFAULT_CONTAINER_RUNTIME_NAME: &str = "runc";
pub const CONTAINER_STOP_SIGNAL: &str = "SIGTERM";
const RUNTIME_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_RUNTIME_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_EXEC_ARGS: usize = 128;
const MAX_EXEC_ARG_BYTES: usize = 16 * 1024;
const MAX_ENV_ENTRIES: usize = 64;

const LABEL_OWNER: &str = "io.kiana.owner";
const LABEL_SCOPE: &str = "io.kiana.scope";
const LABEL_PLAN: &str = "io.kiana.plan";
const LABEL_ROOT: &str = "io.kiana.root";
const LABEL_BACKEND: &str = "io.kiana.backend";

/// Immutable, server-selected settings for one container environment.
///
/// The image must be pinned by digest. The workspace is the only host path ever mounted by the
/// generated create command; runtime sockets, host home directories and credential paths are not
/// accepted as a workspace. Environment variables are an explicit allowlist rather than a copy
/// of the daemon process environment.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerLaunchConfig {
    pub runtime: PathBuf,
    pub image: String,
    pub workspace: PathBuf,
    pub user: String,
    pub network: String,
    pub memory_bytes: u64,
    pub cpu_millis: u32,
    pub max_processes: u32,
    pub runtime_name: String,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
}

impl ContainerLaunchConfig {
    pub fn new(image: impl Into<String>, workspace: impl Into<PathBuf>) -> Self {
        Self {
            runtime: PathBuf::from(
                std::env::var("KIANA_CONTAINER_RUNTIME")
                    .unwrap_or_else(|_| DEFAULT_CONTAINER_RUNTIME.to_owned()),
            ),
            image: image.into(),
            workspace: workspace.into(),
            user: DEFAULT_CONTAINER_USER.to_owned(),
            network: DEFAULT_CONTAINER_NETWORK.to_owned(),
            memory_bytes: 512 * 1024 * 1024,
            cpu_millis: 1_000,
            max_processes: 256,
            runtime_name: std::env::var("KIANA_CONTAINER_RUNTIME_NAME")
                .unwrap_or_else(|_| DEFAULT_CONTAINER_RUNTIME_NAME.to_owned()),
            environment: BTreeMap::new(),
        }
    }

    pub fn validate(&self) -> Result<(), PortError> {
        self.validate_fields()?;
        let workspace = canonical_workspace(&self.workspace)?;
        if is_forbidden_host_mount(&workspace) {
            return Err(PortError::Failed(
                "container_host_control_path_denied".to_owned(),
            ));
        }
        Ok(())
    }

    fn validate_fields(&self) -> Result<(), PortError> {
        if self.runtime.as_os_str().is_empty()
            || self.runtime.to_string_lossy().contains('\0')
            || self.image.len() > 512
            || self.image.contains('\0')
            || !is_digest_pinned_image(&self.image)
        {
            return Err(PortError::Failed(
                "container_image_or_runtime_invalid".to_owned(),
            ));
        }
        if self.network != DEFAULT_CONTAINER_NETWORK {
            return Err(PortError::Failed(
                "container_network_must_be_none".to_owned(),
            ));
        }
        if !valid_non_root_user(&self.user)
            || self.memory_bytes == 0
            || self.memory_bytes > 8 * 1024 * 1024 * 1024
            || self.cpu_millis == 0
            || self.cpu_millis > 16_000
            || self.max_processes == 0
            || self.max_processes > 4_096
            || !matches!(self.runtime_name.as_str(), "runc" | "runsc")
        {
            return Err(PortError::Failed(
                "container_security_profile_invalid".to_owned(),
            ));
        }
        if self.environment.len() > MAX_ENV_ENTRIES {
            return Err(PortError::Failed(
                "container_environment_too_large".to_owned(),
            ));
        }
        for (name, value) in &self.environment {
            if !is_safe_environment_name(name)
                || name.len() > 128
                || value.len() > 4_096
                || value.contains('\0')
            {
                return Err(PortError::Failed(
                    "container_environment_entry_denied".to_owned(),
                ));
            }
        }
        Ok(())
    }

    fn canonical_workspace(&self) -> Result<PathBuf, PortError> {
        self.validate_fields()?;
        canonical_workspace(&self.workspace)
    }
}

/// Parsed command supplied to the container exec boundary. The operation is data, not a shell
/// string; shell syntax is not accepted by this adapter.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerExecOperation {
    pub argv: Vec<String>,
    #[serde(default = "default_container_cwd")]
    pub cwd: String,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
    #[serde(default = "default_exec_timeout_ms")]
    pub timeout_ms: u64,
}

/// Probe semantics are explicit because startup, readiness and liveness answer different
/// questions.  None of them grants a permit or promotes a rollout; the ControlPlane remains the
/// authority for those decisions.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerProbeKind {
    Startup,
    Readiness,
    Liveness,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerProbeRequest {
    pub kind: ContainerProbeKind,
    #[serde(default)]
    pub operation: Option<ContainerExecOperation>,
}

impl ContainerProbeRequest {
    pub fn startup() -> Self {
        Self {
            kind: ContainerProbeKind::Startup,
            operation: None,
        }
    }

    pub fn readiness(operation: ContainerExecOperation) -> Self {
        Self {
            kind: ContainerProbeKind::Readiness,
            operation: Some(operation),
        }
    }

    pub fn liveness(operation: ContainerExecOperation) -> Self {
        Self {
            kind: ContainerProbeKind::Liveness,
            operation: Some(operation),
        }
    }

    fn validate(&self) -> Result<(), PortError> {
        match (self.kind, self.operation.as_ref()) {
            (ContainerProbeKind::Startup, None)
            | (ContainerProbeKind::Readiness | ContainerProbeKind::Liveness, Some(_)) => Ok(()),
            (ContainerProbeKind::Startup, Some(_)) => Err(PortError::Failed(
                "container_startup_probe_must_be_inspect_only".to_owned(),
            )),
            (ContainerProbeKind::Readiness | ContainerProbeKind::Liveness, None) => Err(
                PortError::Failed("container_runtime_probe_operation_missing".to_owned()),
            ),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContainerProbeOutcome {
    pub kind: ContainerProbeKind,
    pub passed: bool,
    pub observation_known: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContainerExecOutcome {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub stop_confirmed: bool,
}

#[derive(Clone, Debug)]
struct PlannedContainer {
    owner_id: String,
    scope_digest: String,
    plan_digest: String,
    config: ContainerLaunchConfig,
    name: String,
}

#[derive(Clone, Default)]
struct ContainerState {
    plans: HashMap<String, PlannedContainer>,
}

/// OCI runtime adapter. State is process-local until the EventLog-backed environment inventory is
/// wired in; restart recovery is deliberately reported as unavailable rather than guessed from a
/// stale container name.
#[derive(Clone)]
pub struct ContainerEnvironmentAdapter {
    config: ContainerLaunchConfig,
    state: Arc<Mutex<ContainerState>>,
}

impl ContainerEnvironmentAdapter {
    pub fn new(config: ContainerLaunchConfig) -> Result<Self, PortError> {
        config.validate()?;
        Ok(Self {
            config,
            state: Arc::new(Mutex::new(ContainerState::default())),
        })
    }

    pub fn config(&self) -> &ContainerLaunchConfig {
        &self.config
    }

    /// Build the exact create argv without executing it. This is the source/CI conformance seam
    /// for proving that a container never receives the host control socket or ambient credentials.
    pub fn create_args(
        config: &ContainerLaunchConfig,
        owner_id: &str,
        scope_digest: &str,
        plan_digest: &str,
        name: &str,
    ) -> Result<Vec<String>, PortError> {
        config.validate()?;
        validate_identity(owner_id, "container_owner_invalid")?;
        validate_identity(scope_digest, "container_scope_invalid")?;
        validate_identity(plan_digest, "container_plan_invalid")?;
        validate_identity(name, "container_name_invalid")?;
        let workspace = config.canonical_workspace()?;
        let root_identity = volume_root_identity(config, owner_id, scope_digest)?;
        let mut args = vec!["create".to_owned()];
        if config.runtime_name != DEFAULT_CONTAINER_RUNTIME_NAME {
            args.extend(["--runtime".to_owned(), config.runtime_name.clone()]);
        }
        for (key, value) in [
            (LABEL_OWNER, owner_id),
            (LABEL_SCOPE, scope_digest),
            (LABEL_PLAN, plan_digest),
            (LABEL_ROOT, root_identity.as_str()),
            (LABEL_BACKEND, CONTAINER_BACKEND),
        ] {
            args.extend(["--label".to_owned(), format!("{key}={value}")]);
        }
        args.extend([
            "--name".to_owned(),
            name.to_owned(),
            "--read-only".to_owned(),
            "--stop-signal".to_owned(),
            CONTAINER_STOP_SIGNAL.to_owned(),
            "--network".to_owned(),
            DEFAULT_CONTAINER_NETWORK.to_owned(),
            "--cap-drop".to_owned(),
            "ALL".to_owned(),
            "--security-opt".to_owned(),
            "no-new-privileges=true".to_owned(),
            "--pids-limit".to_owned(),
            config.max_processes.to_string(),
            "--memory".to_owned(),
            config.memory_bytes.to_string(),
            "--cpus".to_owned(),
            format!("{:.3}", f64::from(config.cpu_millis) / 1_000.0),
            "--user".to_owned(),
            config.user.clone(),
            "--workdir".to_owned(),
            "/workspace".to_owned(),
            "--mount".to_owned(),
            format!("type=bind,src={},dst=/workspace,rw", workspace.display()),
            "--tmpfs".to_owned(),
            "/tmp:rw,noexec,nosuid,nodev".to_owned(),
        ]);
        for (key, value) in &config.environment {
            args.extend(["--env".to_owned(), format!("{key}={value}")]);
        }
        args.extend([
            config.image.clone(),
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            "while :; do sleep 3600; done".to_owned(),
        ]);
        Ok(args)
    }

    pub async fn execute_argv(
        &self,
        prepared: &PreparedEnvironment,
        operation: ContainerExecOperation,
    ) -> Result<ContainerExecOutcome, PortError> {
        let planned = self.planned(prepared)?;
        validate_exec_operation(&operation, &planned.config.environment)?;
        let mut command = self.runtime_command();
        command.arg("exec");
        command.arg("--workdir").arg(&operation.cwd);
        for (key, value) in &operation.environment {
            command.arg("--env").arg(format!("{key}={value}"));
        }
        command.arg(&planned.name);
        command.args(&operation.argv);
        let result = tokio::time::timeout(
            Duration::from_millis(operation.timeout_ms),
            command.output(),
        )
        .await;
        match result {
            Ok(Ok(output)) => {
                bounded_output(&output.stdout)?;
                bounded_output(&output.stderr)?;
                Ok(ContainerExecOutcome {
                    exit_code: output.status.code().unwrap_or(-1),
                    stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                    timed_out: false,
                    stop_confirmed: true,
                })
            }
            Ok(Err(error)) => Err(PortError::Failed(format!("container_exec_failed:{error}"))),
            Err(_) => {
                let stop_confirmed = self.stop_and_confirm(&planned).await?;
                if !stop_confirmed {
                    return Err(PortError::Failed(
                        "result_unknown:container_cancel_unconfirmed".to_owned(),
                    ));
                }
                Ok(ContainerExecOutcome {
                    exit_code: 124,
                    stdout: String::new(),
                    stderr: String::new(),
                    timed_out: true,
                    stop_confirmed,
                })
            }
        }
    }

    /// Run an explicit lifecycle observation after checking the container's labelled identity.
    /// Startup is inspect-only; readiness and liveness use caller-supplied argv constrained by
    /// the same workspace and environment allowlist as normal execution.
    pub async fn lifecycle_probe(
        &self,
        prepared: &PreparedEnvironment,
        request: ContainerProbeRequest,
    ) -> Result<ContainerProbeOutcome, PortError> {
        request.validate()?;
        let planned = self.planned(prepared)?;
        let inspected = self.inspect(&planned).await?;
        if !inspected.identity_matches(&planned) {
            return Err(PortError::Failed(
                "result_unknown:container_probe_identity_mismatch".to_owned(),
            ));
        }
        if request.kind == ContainerProbeKind::Startup {
            let running = inspected.running().ok_or_else(|| {
                PortError::Failed("result_unknown:container_startup_state_unknown".to_owned())
            })?;
            return Ok(ContainerProbeOutcome {
                kind: request.kind,
                passed: running,
                observation_known: true,
                reason: (!running).then(|| "container_startup_not_running".to_owned()),
            });
        }
        let running = inspected.running().ok_or_else(|| {
            PortError::Failed("result_unknown:container_probe_state_unknown".to_owned())
        })?;
        if !running {
            return Ok(ContainerProbeOutcome {
                kind: request.kind,
                passed: false,
                observation_known: true,
                reason: Some("container_probe_not_running".to_owned()),
            });
        }
        let operation = request.operation.ok_or_else(|| {
            PortError::Failed("container_runtime_probe_operation_missing".to_owned())
        })?;
        let outcome = self.execute_argv(prepared, operation).await?;
        Ok(ContainerProbeOutcome {
            kind: request.kind,
            passed: !outcome.timed_out && outcome.exit_code == 0,
            observation_known: !outcome.timed_out,
            reason: if outcome.timed_out {
                Some("result_unknown:container_probe_timeout".to_owned())
            } else {
                (outcome.exit_code != 0).then(|| format!("probe_exit_code:{}", outcome.exit_code))
            },
        })
    }

    fn runtime_command(&self) -> Command {
        let mut command = Command::new(&self.config.runtime);
        command.env_clear();
        command
            .env("PATH", "/usr/local/bin:/usr/bin:/bin")
            .env("HOME", "/nonexistent")
            .env("LANG", "C");
        command
    }

    fn planned(&self, prepared: &PreparedEnvironment) -> Result<PlannedContainer, PortError> {
        let environment_id = prepared
            .environment_id
            .as_deref()
            .ok_or_else(|| PortError::Failed("container_environment_id_missing".to_owned()))?;
        let state = self
            .state
            .lock()
            .map_err(|_| PortError::Failed("container_state_poisoned".to_owned()))?;
        let planned = state
            .plans
            .get(&prepared.plan_digest)
            .cloned()
            .ok_or_else(|| {
                PortError::Unavailable("container_environment_not_recoverable".to_owned())
            })?;
        if planned.name != environment_id
            || planned.plan_digest != prepared.plan_digest
            || planned.config.runtime_name.is_empty()
        {
            return Err(PortError::Conflict(
                "container_environment_identity_mismatch".to_owned(),
            ));
        }
        Ok(planned)
    }

    async fn stop_and_confirm(&self, planned: &PlannedContainer) -> Result<bool, PortError> {
        let mut stop = self.runtime_command();
        stop.arg("stop")
            .arg("--signal")
            .arg(CONTAINER_STOP_SIGNAL)
            .arg("--time")
            .arg("1")
            .arg(&planned.name);
        let output = bounded_runtime_output(stop).await?;
        if !output.status.success() {
            return Ok(false);
        }
        self.confirm_identity_and_stopped(planned).await
    }

    async fn confirm_identity_and_stopped(
        &self,
        planned: &PlannedContainer,
    ) -> Result<bool, PortError> {
        let inspected = self.inspect(planned).await?;
        Ok(inspected.identity_matches(planned) && inspected.stopped())
    }

    async fn inspect(&self, planned: &PlannedContainer) -> Result<ContainerInspection, PortError> {
        let mut command = self.runtime_command();
        command.arg("inspect").arg(&planned.name);
        let output = bounded_runtime_output(command).await?;
        if !output.status.success() {
            return Err(PortError::Failed(
                "result_unknown:container_inspect_failed".to_owned(),
            ));
        }
        let value: Value = serde_json::from_slice(&output.stdout).map_err(|_| {
            PortError::Failed("result_unknown:container_inspect_invalid".to_owned())
        })?;
        Ok(ContainerInspection { value })
    }
}

#[async_trait]
impl EnvironmentPort for ContainerEnvironmentAdapter {
    async fn probe(&self, _scope: &EnvironmentScope) -> Result<EnvironmentProbe, PortError> {
        self.config.validate()?;
        let mut command = self.runtime_command();
        command.arg("--version");
        let output = bounded_runtime_output(command).await.map_err(|error| {
            PortError::Unavailable(format!("container_runtime_unavailable:{error}"))
        })?;
        if !output.status.success() {
            return Err(PortError::Unavailable(
                "container_runtime_probe_failed".to_owned(),
            ));
        }
        let version = String::from_utf8_lossy(&output.stdout)
            .trim()
            .chars()
            .take(256)
            .collect::<String>();
        let capabilities = BTreeMap::from([
            ("oci_container".to_owned(), true),
            ("immutable_image".to_owned(), true),
            ("non_root_user".to_owned(), true),
            ("network_none".to_owned(), true),
            ("resource_limits".to_owned(), true),
            ("owner_scope_labels".to_owned(), true),
            ("volume_root_identity".to_owned(), true),
            ("no_host_control_socket".to_owned(), true),
            (
                "gvisor_runtime".to_owned(),
                self.config.runtime_name == "runsc",
            ),
        ]);
        let enforced = capabilities
            .keys()
            .map(|key| (key.clone(), false))
            .collect::<BTreeMap<_, _>>();
        Ok(EnvironmentProbe {
            backend: CONTAINER_BACKEND.to_owned(),
            backend_version: version,
            capabilities,
            enforced,
            failure_reason: None,
        })
    }

    async fn plan(
        &self,
        scope: &EnvironmentScope,
        probe: &EnvironmentProbe,
    ) -> Result<EnvironmentPlan, PortError> {
        self.config.validate()?;
        validate_scope(scope)?;
        if probe.backend != CONTAINER_BACKEND
            || probe.failure_reason.is_some()
            || !required_capabilities_supported(probe, self.config.runtime_name == "runsc")
        {
            return Err(PortError::Unavailable(
                "container_required_dimension_unavailable".to_owned(),
            ));
        }
        let workspace = self.config.canonical_workspace()?;
        let project_root = Path::new(&scope.project_root)
            .canonicalize()
            .map_err(|_| PortError::Failed("container_project_root_invalid".to_owned()))?;
        if !workspace.starts_with(&project_root) {
            return Err(PortError::Failed(
                "container_workspace_outside_scope".to_owned(),
            ));
        }
        let plan_input = serde_json::json!({
            "backend": CONTAINER_BACKEND,
            "backend_version": &probe.backend_version,
            "owner_id": &scope.owner_id,
            "scope_digest": &scope.scope_digest,
            "image": &self.config.image,
            "workspace": &workspace,
            "user": &self.config.user,
            "network": &self.config.network,
            "memory_bytes": self.config.memory_bytes,
            "cpu_millis": self.config.cpu_millis,
            "max_processes": self.config.max_processes,
            "runtime_name": &self.config.runtime_name,
            "environment": &self.config.environment,
        });
        let plan_digest = kiana_domain::json_digest(&plan_input);
        let name = format!("kiana-{}", &plan_digest[7..19]);
        let planned = PlannedContainer {
            owner_id: scope.owner_id.clone(),
            scope_digest: scope.scope_digest.clone(),
            plan_digest: plan_digest.clone(),
            config: self.config.clone(),
            name,
        };
        self.state
            .lock()
            .map_err(|_| PortError::Failed("container_state_poisoned".to_owned()))?
            .plans
            .insert(plan_digest.clone(), planned);
        Ok(EnvironmentPlan {
            owner_id: scope.owner_id.clone(),
            scope_digest: scope.scope_digest.clone(),
            backend: CONTAINER_BACKEND.to_owned(),
            backend_version: probe.backend_version.clone(),
            plan_digest,
            required_dimensions: vec![
                "immutable_image".to_owned(),
                "non_root_user".to_owned(),
                "network_none".to_owned(),
                "resource_limits".to_owned(),
                "owner_scope_labels".to_owned(),
                "volume_root_identity".to_owned(),
                "no_host_control_socket".to_owned(),
            ],
        })
    }

    async fn prepare(&self, plan: &EnvironmentPlan) -> Result<PreparedEnvironment, PortError> {
        let planned = {
            let state = self
                .state
                .lock()
                .map_err(|_| PortError::Failed("container_state_poisoned".to_owned()))?;
            state
                .plans
                .get(&plan.plan_digest)
                .cloned()
                .ok_or_else(|| PortError::Unavailable("container_plan_not_found".to_owned()))?
        };
        if plan.backend != CONTAINER_BACKEND
            || plan.owner_id != planned.owner_id
            || plan.scope_digest != planned.scope_digest
        {
            return Err(PortError::Conflict(
                "container_plan_identity_mismatch".to_owned(),
            ));
        }
        let args = Self::create_args(
            &planned.config,
            &planned.owner_id,
            &planned.scope_digest,
            &planned.plan_digest,
            &planned.name,
        )?;
        let mut create = self.runtime_command();
        create.args(args);
        let output = bounded_runtime_output(create)
            .await
            .map_err(|error| PortError::Unavailable(format!("container_create_failed:{error}")))?;
        if !output.status.success() {
            return Err(PortError::Failed(
                "container_runtime_failure_no_host_fallback".to_owned(),
            ));
        }
        let id = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if id.is_empty() || id.len() > 256 || id.chars().any(char::is_whitespace) {
            return Err(PortError::Failed(
                "container_runtime_identity_invalid".to_owned(),
            ));
        }
        let mut start = self.runtime_command();
        start.arg("start").arg(&planned.name);
        let start_output = bounded_runtime_output(start).await?;
        if !start_output.status.success() {
            let _ = self.remove_owned(&planned).await;
            return Err(PortError::Failed(
                "container_start_failed_no_host_fallback".to_owned(),
            ));
        }
        let inspected = self.inspect(&planned).await?;
        if !inspected.identity_matches(&planned) {
            let _ = self.remove_owned(&planned).await;
            return Err(PortError::Failed(
                "result_unknown:container_label_identity_mismatch".to_owned(),
            ));
        }
        let mut enforced = vec![
            "immutable_image".to_owned(),
            "non_root_user".to_owned(),
            "network_none".to_owned(),
            "resource_limits".to_owned(),
            "owner_scope_labels".to_owned(),
            "volume_root_identity".to_owned(),
            "no_host_control_socket".to_owned(),
        ];
        if planned.config.runtime_name == "runsc" {
            enforced.push("gvisor_runtime".to_owned());
        }
        Ok(PreparedEnvironment {
            plan_digest: plan.plan_digest.clone(),
            backend: CONTAINER_BACKEND.to_owned(),
            backend_version: plan.backend_version.clone(),
            enforced_dimensions: enforced,
            environment_id: Some(planned.name),
        })
    }

    async fn execute(
        &self,
        prepared: &PreparedEnvironment,
        operation: &str,
    ) -> Result<EnvironmentEffectReceipt, PortError> {
        let operation: ContainerExecOperation = serde_json::from_str(operation)
            .map_err(|_| PortError::Failed("container_exec_operation_invalid".to_owned()))?;
        let outcome = self.execute_argv(prepared, operation).await?;
        Ok(EnvironmentEffectReceipt {
            plan_digest: prepared.plan_digest.clone(),
            phase: "execute".to_owned(),
            effect_known: !outcome.timed_out,
            stop_confirmed: outcome.stop_confirmed,
            reason: (outcome.exit_code != 0).then(|| format!("exit_code:{}", outcome.exit_code)),
        })
    }

    async fn quiesce(
        &self,
        prepared: &PreparedEnvironment,
    ) -> Result<EnvironmentEffectReceipt, PortError> {
        let planned = self.planned(prepared)?;
        let stop_confirmed = self.stop_and_confirm(&planned).await?;
        if !stop_confirmed {
            return Err(PortError::Failed(
                "result_unknown:container_quiesce_unconfirmed".to_owned(),
            ));
        }
        Ok(EnvironmentEffectReceipt {
            plan_digest: prepared.plan_digest.clone(),
            phase: "quiesce".to_owned(),
            effect_known: true,
            stop_confirmed: true,
            reason: None,
        })
    }

    async fn dispose(
        &self,
        prepared: PreparedEnvironment,
    ) -> Result<EnvironmentEffectReceipt, PortError> {
        let planned = self.planned(&prepared)?;
        let inspected = self.inspect(&planned).await?;
        if !inspected.identity_matches(&planned) {
            return Err(PortError::Conflict(
                "container_dispose_owner_mismatch".to_owned(),
            ));
        }
        self.remove_owned(&planned).await?;
        self.state
            .lock()
            .map_err(|_| PortError::Failed("container_state_poisoned".to_owned()))?
            .plans
            .remove(&prepared.plan_digest);
        Ok(EnvironmentEffectReceipt {
            plan_digest: prepared.plan_digest,
            phase: "dispose".to_owned(),
            effect_known: true,
            stop_confirmed: true,
            reason: None,
        })
    }
}

impl ContainerEnvironmentAdapter {
    async fn remove_owned(&self, planned: &PlannedContainer) -> Result<(), PortError> {
        let inspected = self.inspect(planned).await?;
        if !inspected.identity_matches(planned) {
            return Err(PortError::Conflict(
                "container_cleanup_owner_mismatch".to_owned(),
            ));
        }
        let mut remove = self.runtime_command();
        remove.arg("rm").arg("--force").arg(&planned.name);
        let output = bounded_runtime_output(remove).await?;
        if !output.status.success() {
            return Err(PortError::Failed(
                "result_unknown:container_cleanup_failed".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct ContainerInspection {
    value: Value,
}

impl ContainerInspection {
    fn labels(&self) -> Option<&serde_json::Map<String, Value>> {
        self.value
            .as_array()?
            .first()?
            .get("Config")?
            .get("Labels")?
            .as_object()
    }

    fn identity_matches(&self, planned: &PlannedContainer) -> bool {
        let Some(labels) = self.labels() else {
            return false;
        };
        let Some(root_identity) =
            volume_root_identity(&planned.config, &planned.owner_id, &planned.scope_digest).ok()
        else {
            return false;
        };
        labels.get(LABEL_OWNER).and_then(Value::as_str) == Some(planned.owner_id.as_str())
            && labels.get(LABEL_SCOPE).and_then(Value::as_str)
                == Some(planned.scope_digest.as_str())
            && labels.get(LABEL_PLAN).and_then(Value::as_str) == Some(planned.plan_digest.as_str())
            && labels.get(LABEL_ROOT).and_then(Value::as_str) == Some(root_identity.as_str())
            && labels.get(LABEL_BACKEND).and_then(Value::as_str) == Some(CONTAINER_BACKEND)
    }

    fn running(&self) -> Option<bool> {
        self.value
            .as_array()
            .and_then(|items| items.first())
            .and_then(|item| item.get("State"))
            .and_then(|state| state.get("Running"))
            .and_then(Value::as_bool)
    }

    fn stopped(&self) -> bool {
        self.running() == Some(false)
    }
}

async fn bounded_runtime_output(mut command: Command) -> Result<std::process::Output, PortError> {
    let output = tokio::time::timeout(RUNTIME_TIMEOUT, command.output())
        .await
        .map_err(|_| PortError::Failed("container_runtime_timeout".to_owned()))?
        .map_err(|error| PortError::Unavailable(format!("container_runtime_io:{error}")))?;
    bounded_output(&output.stdout)?;
    bounded_output(&output.stderr)?;
    Ok(output)
}

fn bounded_output(bytes: &[u8]) -> Result<(), PortError> {
    if bytes.len() > MAX_RUNTIME_OUTPUT_BYTES {
        Err(PortError::Failed(
            "container_runtime_output_limit".to_owned(),
        ))
    } else {
        Ok(())
    }
}

fn required_capabilities_supported(probe: &EnvironmentProbe, require_gvisor: bool) -> bool {
    let required = [
        "oci_container",
        "immutable_image",
        "non_root_user",
        "network_none",
        "resource_limits",
        "owner_scope_labels",
        "volume_root_identity",
        "no_host_control_socket",
    ];
    required
        .iter()
        .all(|name| probe.capabilities.get(*name) == Some(&true))
        && (!require_gvisor || probe.capabilities.get("gvisor_runtime") == Some(&true))
}

fn validate_scope(scope: &EnvironmentScope) -> Result<(), PortError> {
    for (value, reason) in [
        (&scope.owner_id, "container_owner_invalid"),
        (&scope.scope_digest, "container_scope_invalid"),
        (&scope.sandbox, "container_sandbox_invalid"),
    ] {
        validate_identity(value, reason)?;
    }
    if scope.project_root.trim().is_empty()
        || scope.project_root.len() > 4_096
        || scope.project_root.contains('\0')
        || !Path::new(&scope.project_root).is_absolute()
    {
        return Err(PortError::Failed(
            "container_project_root_invalid".to_owned(),
        ));
    }
    if scope.path_allow.len() > 256
        || scope.path_allow.iter().any(|path| {
            path.trim().is_empty() || path.contains('\0') || Path::new(path).is_absolute()
        })
    {
        return Err(PortError::Failed("container_path_allow_invalid".to_owned()));
    }
    Ok(())
}

fn validate_identity(value: &str, reason: &str) -> Result<(), PortError> {
    if value.trim().is_empty() || value.len() > 512 || value.contains('\0') || value.contains('/') {
        return Err(PortError::Failed(reason.to_owned()));
    }
    Ok(())
}

fn is_digest_pinned_image(image: &str) -> bool {
    let Some((repository, digest)) = image.rsplit_once("@sha256:") else {
        return false;
    };
    !repository.trim().is_empty()
        && digest.len() == 64
        && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_non_root_user(user: &str) -> bool {
    let Some((uid, gid)) = user.split_once(':') else {
        return false;
    };
    uid.parse::<u32>().is_ok_and(|value| value > 0)
        && gid.parse::<u32>().is_ok_and(|value| value > 0)
}

fn is_safe_environment_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        && !upper.contains("KEY")
        && !upper.contains("SECRET")
        && !upper.contains("TOKEN")
        && !matches!(
            upper.as_str(),
            "DOCKER_HOST" | "DOCKER_TLS_VERIFY" | "DOCKER_CERT_PATH" | "SSH_AUTH_SOCK"
        )
}

fn canonical_workspace(path: &Path) -> Result<PathBuf, PortError> {
    let canonical = path
        .canonicalize()
        .map_err(|_| PortError::Failed("container_workspace_invalid".to_owned()))?;
    if !canonical.is_dir() {
        return Err(PortError::Failed("container_workspace_invalid".to_owned()));
    }
    Ok(canonical)
}

fn is_forbidden_host_mount(path: &Path) -> bool {
    let normalized = path.to_string_lossy();
    normalized == "/"
        || normalized == "/root"
        || normalized == "/home"
        || normalized == "/run"
        || normalized == "/var/run"
        || normalized.starts_with("/run/")
        || normalized.starts_with("/var/run/")
        || normalized.ends_with("/.docker")
        || normalized.ends_with("/.ssh")
}

fn default_container_cwd() -> String {
    "/workspace".to_owned()
}

fn default_exec_timeout_ms() -> u64 {
    30_000
}

fn validate_exec_operation(
    operation: &ContainerExecOperation,
    allowed_environment: &BTreeMap<String, String>,
) -> Result<(), PortError> {
    if operation.argv.is_empty()
        || operation.argv.len() > MAX_EXEC_ARGS
        || operation.timeout_ms == 0
        || operation.timeout_ms > 60_000
        || (!operation.cwd.eq("/workspace") && !operation.cwd.starts_with("/workspace/"))
        || operation.cwd.split('/').any(|part| part == "..")
        || operation.cwd.contains('\0')
        || operation.environment.len() > MAX_ENV_ENTRIES
    {
        return Err(PortError::Failed(
            "container_exec_operation_invalid".to_owned(),
        ));
    }
    for arg in &operation.argv {
        if arg.is_empty() || arg.len() > MAX_EXEC_ARG_BYTES || arg.contains('\0') {
            return Err(PortError::Failed(
                "container_exec_argument_invalid".to_owned(),
            ));
        }
    }
    for (name, value) in &operation.environment {
        if !is_safe_environment_name(name)
            || !allowed_environment.contains_key(name)
            || value.len() > 4_096
            || value.contains('\0')
        {
            return Err(PortError::Failed(
                "container_exec_environment_denied".to_owned(),
            ));
        }
    }
    Ok(())
}

fn volume_root_identity(
    config: &ContainerLaunchConfig,
    owner_id: &str,
    scope_digest: &str,
) -> Result<String, PortError> {
    let workspace = config.canonical_workspace()?;
    Ok(kiana_domain::json_digest(&json!({
        "owner_id": owner_id,
        "scope_digest": scope_digest,
        "workspace": workspace,
        "container_path": "/workspace",
        "read_only_root": true,
    })))
}
