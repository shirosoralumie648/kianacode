//! CAP-33 container/gVisor execution environment descriptor and selection contract.
//!
//! The card asks for an *optional* execution environment, not a shipped one. What that means in
//! source is a descriptor that states exactly which runtime, image, user, mounts, limits and
//! network the environment will have, plus a selection decision that refuses the environment
//! when any of those facts are missing or unverifiable.
//!
//! Three rules from the card are enforced here rather than in prose:
//!
//! 1. `container_never_mounts_host_control_socket_or_credentials`. The mount list is checked
//!    against a deny list of host control and credential locations, and the credential/network
//!    surface is deny-by-default: an environment with network access needs an explicit,
//!    separately-authorized egress declaration, and this descriptor never sets one.
//! 2. `container_runtime_failure_never_falls_back_to_host`. [`ContainerEnvironmentDecision`] has
//!    no field that could name a substitute backend, and its validation refuses a decision that
//!    carries one. A runtime that is missing, mismatched or unverified produces
//!    `container_environment_unavailable`, never a host environment.
//! 3. `container_cancel_confirms_inner_process_stop`. A stop whose inner-process confirmation
//!    was not observed is `Unconfirmed`, and an unconfirmed stop can never complete a
//!    [`ContainerEffectDisposition::Stopped`] claim.
//!
//! gVisor is modelled as a runtime *name* in the descriptor, exactly as the existing
//! `kiana-daemon::container_environment` adapter does. Selecting `runsc` is a configuration
//! choice, not a claim that gVisor confinement is in force; [`ContainerEnvironmentDecision`]
//! requires a runtime receipt before a gVisor environment may be reported as usable.
//!
//! This is a source contract. It inspects no runtime, runs no container, reads no manifest,
//! starts no process and calls no port. The daemon adapter remains the only thing that talks to
//! a runtime; this module decides whether it may be asked to.

use crate::{json_digest, redact_text, PlatformTarget, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const CONTAINER_ENVIRONMENT_SCHEMA: &str = "kiana.container-environment-descriptor.v1";
pub const CONTAINER_ENVIRONMENT_DECISION_SCHEMA: &str = "kiana.container-environment-decision.v1";
pub const CONTAINER_ENVIRONMENT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const CONTAINER_WORKSPACE_MOUNT: &str = "/workspace";
pub const MAX_CONTAINER_MOUNTS: usize = 16;
pub const MAX_CONTAINER_LIMITS: usize = 16;

/// Runtimes this contract knows how to name. A name outside this list is refused rather than
/// passed to a shell, so a typo in configuration cannot become an arbitrary executable.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerRuntimeKind {
    /// Standard OCI isolation.
    RunC,
    /// gVisor's `runsc`. Configuration only; confinement is not asserted by naming it.
    RunSc,
}

impl ContainerRuntimeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RunC => "runc",
            Self::RunSc => "runsc",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "runc" => Some(Self::RunC),
            "runsc" => Some(Self::RunSc),
            _ => None,
        }
    }

    /// Whether this runtime provides an additional syscall-interception layer beyond standard
    /// container isolation. It is a property of the runtime, not a claim about what is enforced.
    pub const fn has_extra_isolation(self) -> bool {
        matches!(self, Self::RunSc)
    }
}

/// One host path bound into the container. The workspace is the only mount the product path
/// creates; anything else has to be declared and is checked against the host deny list.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerMount {
    pub host_path: String,
    pub container_path: String,
    pub read_only: bool,
}

impl ContainerMount {
    pub fn new(
        host_path: impl Into<String>,
        container_path: impl Into<String>,
        read_only: bool,
    ) -> Result<Self, String> {
        let mount = Self {
            host_path: host_path.into(),
            container_path: container_path.into(),
            read_only,
        };
        mount.validate()?;
        Ok(mount)
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.host_path.starts_with('/')
            || self.host_path.contains(['\0', '\n', '\r'])
            || !self.container_path.starts_with('/')
            || self.container_path.contains(['\0', '\n', '\r'])
            || self.container_path.split('/').any(|part| part == "..")
        {
            return Err("container_environment_mount_invalid".to_owned());
        }
        if is_forbidden_host_mount(&self.host_path) {
            return Err("container_environment_host_control_mount_denied".to_owned());
        }
        Ok(())
    }
}

/// Resource ceilings for one environment. These enter the descriptor so a limit is a
/// versioned fact, not an unrecorded default.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerResourceLimits {
    pub memory_bytes: u64,
    pub cpu_millis: u32,
    pub max_processes: u32,
    pub max_output_bytes: u64,
    pub disk_bytes: u64,
}

impl ContainerResourceLimits {
    pub fn new(
        memory_bytes: u64,
        cpu_millis: u32,
        max_processes: u32,
        max_output_bytes: u64,
        disk_bytes: u64,
    ) -> Result<Self, String> {
        let limits = Self {
            memory_bytes,
            cpu_millis,
            max_processes,
            max_output_bytes,
            disk_bytes,
        };
        limits.validate()?;
        Ok(limits)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.memory_bytes == 0
            || self.memory_bytes > 8 * 1024 * 1024 * 1024
            || self.cpu_millis == 0
            || self.cpu_millis > 16_000
            || self.max_processes == 0
            || self.max_processes > 4_096
            || self.max_output_bytes == 0
            || self.max_output_bytes > 64 * 1024 * 1024
            || self.disk_bytes == 0
            || self.disk_bytes > 64 * 1024 * 1024 * 1024
        {
            return Err("container_environment_limits_invalid".to_owned());
        }
        Ok(())
    }
}

/// The immutable, server-selected description of one optional container environment.
///
/// Every field here is part of what the control plane must show before selecting the backend,
/// so image, runtime, user, mounts, network and limits are all reviewable rather than implicit
/// in an argv a user cannot see.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerEnvironmentDescriptor {
    pub schema: String,
    pub version: SchemaVersion,
    pub owner_id: String,
    pub scope_digest: String,
    /// `repository@sha256:<64 hex>`. A tag or a bare name is refused: a mutable reference is
    /// not an immutable environment.
    pub image: String,
    pub runtime: ContainerRuntimeKind,
    /// Non-root uid:gid. A root container is refused at construction, not downgraded.
    pub user: String,
    pub mounts: Vec<ContainerMount>,
    pub limits: ContainerResourceLimits,
    /// Always `deny`. The container backend has no egress adapter in this checkout, and the
    /// field exists so that "network is deny" is a value in the record rather than an omission.
    pub network_policy: String,
    pub read_only_root: bool,
    pub environment_id: String,
    pub descriptor_digest: String,
}

impl ContainerEnvironmentDescriptor {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        owner_id: impl Into<String>,
        scope_digest: impl Into<String>,
        image: impl Into<String>,
        runtime: ContainerRuntimeKind,
        user: impl Into<String>,
        mounts: Vec<ContainerMount>,
        limits: ContainerResourceLimits,
        environment_id: impl Into<String>,
    ) -> Result<Self, String> {
        let mut descriptor = Self {
            schema: CONTAINER_ENVIRONMENT_SCHEMA.to_owned(),
            version: CONTAINER_ENVIRONMENT_VERSION,
            owner_id: owner_id.into(),
            scope_digest: scope_digest.into(),
            image: image.into(),
            runtime,
            user: user.into(),
            mounts,
            limits,
            network_policy: "deny".to_owned(),
            read_only_root: true,
            environment_id: environment_id.into(),
            descriptor_digest: String::new(),
        };
        descriptor.descriptor_digest = descriptor.digest();
        descriptor.validate()?;
        Ok(descriptor)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONTAINER_ENVIRONMENT_SCHEMA
            || !self
                .version
                .is_compatible_with(&CONTAINER_ENVIRONMENT_VERSION)
            || self.owner_id.trim().is_empty()
            || self.owner_id.len() > 256
            || self.owner_id.contains(['\0', '\n', '\r', '/'])
            || self.scope_digest.trim().is_empty()
            || self.scope_digest.len() > 256
            || self.environment_id.trim().is_empty()
            || self.environment_id.len() > 256
            || self.environment_id.contains(['\0', '\n', '\r', '/'])
            || !is_digest_pinned_image(&self.image)
            || !is_non_root_user(&self.user)
            || self.network_policy != "deny"
            || !self.read_only_root
            || self.mounts.is_empty()
            || self.mounts.len() > MAX_CONTAINER_MOUNTS
        {
            return Err("container_environment_descriptor_invalid".to_owned());
        }
        if redact_text(&self.owner_id) != self.owner_id
            || redact_text(&self.scope_digest) != self.scope_digest
            || redact_text(&self.image) != self.image
        {
            return Err("container_environment_descriptor_secret_material".to_owned());
        }
        self.limits.validate()?;
        let mut container_paths = BTreeSet::new();
        let mut writable = 0usize;
        for mount in &self.mounts {
            mount.validate()?;
            if !container_paths.insert(mount.container_path.clone()) {
                return Err("container_environment_mount_path_duplicate".to_owned());
            }
            if !mount.read_only {
                writable += 1;
            }
        }
        // The isolated workspace is the only writable surface. A second writable mount is how a
        // host directory would become writable from inside the container, so it is refused
        // rather than bounded.
        if writable > 1
            || !self
                .mounts
                .iter()
                .any(|mount| mount.container_path == CONTAINER_WORKSPACE_MOUNT && !mount.read_only)
        {
            return Err("container_environment_workspace_mount_required".to_owned());
        }
        if self.descriptor_digest != self.digest() {
            return Err("container_environment_descriptor_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "owner_id": self.owner_id,
            "scope_digest": self.scope_digest,
            "image": self.image,
            "runtime": self.runtime,
            "user": self.user,
            "mounts": self.mounts,
            "limits": self.limits,
            "network_policy": self.network_policy,
            "read_only_root": self.read_only_root,
            "environment_id": self.environment_id,
        }))
    }
}

/// What the server observed about the runtime before selecting the environment.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerRuntimeObservation {
    /// The runtime version string the probe returned. A runtime that cannot report a version is
    /// not a runtime this contract will select.
    pub runtime_version: String,
    pub available: bool,
    /// The runtime the probe actually found, when it differs from the requested one. A mismatch
    /// is a refusal: running a `runc` container while the descriptor says `runsc` would make the
    /// descriptor a lie.
    pub observed_runtime: Option<ContainerRuntimeKind>,
    /// Whether the image reference resolved in the runtime's own store. A descriptor naming an
    /// image the runtime has never seen cannot be prepared offline, so it is refused here rather
    /// than pulled at execution time.
    pub image_present: bool,
    /// Receipts for the enforcement dimensions the descriptor promises. A dimension that is
    /// promised but not observed is a refusal, not an assumption.
    pub observed_dimensions: BTreeSet<String>,
}

impl ContainerRuntimeObservation {
    pub fn new(
        runtime_version: impl Into<String>,
        available: bool,
        observed_runtime: Option<ContainerRuntimeKind>,
        image_present: bool,
        observed_dimensions: BTreeSet<String>,
    ) -> Result<Self, String> {
        let observation = Self {
            runtime_version: runtime_version.into(),
            available,
            observed_runtime,
            image_present,
            observed_dimensions,
        };
        observation.validate()?;
        Ok(observation)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.runtime_version.trim().is_empty()
            || self.runtime_version.len() > 256
            || self.runtime_version.contains(['\0', '\n', '\r'])
            || self.observed_dimensions.len() > MAX_CONTAINER_LIMITS
            || self
                .observed_dimensions
                .iter()
                .any(|dimension| dimension.trim().is_empty() || dimension.len() > 128)
        {
            return Err("container_runtime_observation_invalid".to_owned());
        }
        Ok(())
    }
}

/// The enforcement dimensions a container environment must demonstrate before it may be used.
pub const CONTAINER_REQUIRED_DIMENSIONS: [&str; 7] = [
    "immutable_image",
    "non_root_user",
    "network_none",
    "resource_limits",
    "owner_scope_labels",
    "workspace_isolation",
    "no_host_control_socket",
];

/// The additional dimension a gVisor environment must demonstrate. It is separate from the
/// base set because naming `runsc` is configuration, and only a runtime observation can say
/// the gVisor layer is actually in force.
pub const CONTAINER_GVISOR_DIMENSION: &str = "gvisor_runtime";

/// The outcome of asking for the container backend.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerEnvironmentSelection {
    /// The environment may be used.
    Selected,
    /// The runtime is not available on this host, or does not match the descriptor.
    Unavailable,
    /// The runtime is available but a required enforcement dimension was not observed.
    NotEnforced,
    /// The descriptor itself is unusable.
    Rejected,
}

impl ContainerEnvironmentSelection {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Selected => "selected",
            Self::Unavailable => "unavailable",
            Self::NotEnforced => "not_enforced",
            Self::Rejected => "rejected",
        }
    }

    pub const fn is_usable(self) -> bool {
        matches!(self, Self::Selected)
    }
}

/// The ordered decision for one container environment selection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerEnvironmentDecision {
    pub schema: String,
    pub version: SchemaVersion,
    pub environment_id: String,
    pub selection: ContainerEnvironmentSelection,
    /// Always true. This type has no field that could name a host substitute, and this makes the
    /// claim checkable: `validate` refuses a decision whose value says otherwise.
    pub no_host_fallback: bool,
    /// The runtime that would be used. Empty when the environment was refused, because there is
    /// nothing to fall back to and nothing to name.
    pub effective_runtime: String,
    /// The enforcement dimensions the server would accept as in force for this environment.
    pub enforced_dimensions: BTreeSet<String>,
    pub reason: String,
    pub remediation: String,
    pub descriptor_digest: String,
    pub decision_digest: String,
}

impl ContainerEnvironmentDecision {
    pub fn evaluate(
        descriptor: &ContainerEnvironmentDescriptor,
        observation: &ContainerRuntimeObservation,
    ) -> Result<Self, String> {
        let (selection, enforced, reason, remediation) = derive(descriptor, observation);
        let mut decision = Self {
            schema: CONTAINER_ENVIRONMENT_DECISION_SCHEMA.to_owned(),
            version: CONTAINER_ENVIRONMENT_VERSION,
            environment_id: descriptor.environment_id.clone(),
            selection,
            no_host_fallback: true,
            effective_runtime: if selection.is_usable() {
                descriptor.runtime.as_str().to_owned()
            } else {
                String::new()
            },
            enforced_dimensions: enforced,
            reason: reason.to_owned(),
            remediation: remediation.to_owned(),
            descriptor_digest: descriptor.descriptor_digest.clone(),
            decision_digest: String::new(),
        };
        decision.decision_digest = decision.digest();
        decision.validate_against(descriptor, observation)?;
        Ok(decision)
    }

    pub fn validate_against(
        &self,
        descriptor: &ContainerEnvironmentDescriptor,
        observation: &ContainerRuntimeObservation,
    ) -> Result<(), String> {
        descriptor.validate()?;
        observation.validate()?;
        let (selection, enforced, reason, remediation) = derive(descriptor, observation);
        if self.schema != CONTAINER_ENVIRONMENT_DECISION_SCHEMA
            || !self
                .version
                .is_compatible_with(&CONTAINER_ENVIRONMENT_VERSION)
            || self.environment_id != descriptor.environment_id
            || self.selection != selection
            || self.enforced_dimensions != enforced
            || self.reason != reason
            || self.remediation != remediation
            || self.descriptor_digest != descriptor.descriptor_digest
        {
            return Err("container_environment_decision_binding_invalid".to_owned());
        }
        if !self.no_host_fallback {
            return Err("container_environment_host_fallback_forbidden".to_owned());
        }
        if !selection.is_usable() && !self.effective_runtime.is_empty() {
            return Err("container_environment_substitute_runtime_named".to_owned());
        }
        if selection.is_usable() && self.effective_runtime != descriptor.runtime.as_str() {
            return Err("container_environment_effective_runtime_mismatch".to_owned());
        }
        if self.decision_digest != self.digest() {
            return Err("container_environment_decision_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn usable(&self) -> bool {
        self.selection.is_usable()
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "environment_id": self.environment_id,
            "selection": self.selection,
            "no_host_fallback": self.no_host_fallback,
            "effective_runtime": self.effective_runtime,
            "enforced_dimensions": self.enforced_dimensions,
            "reason": self.reason,
            "remediation": self.remediation,
            "descriptor_digest": self.descriptor_digest,
        }))
    }
}

/// Decide in a fixed order: descriptor first, then runtime availability, then runtime identity,
/// then the image, then the enforcement dimensions. The order puts "the thing you asked for is
/// not even well-formed" ahead of "the runtime is missing", so a misconfiguration is never
/// reported as a missing dependency.
fn derive(
    descriptor: &ContainerEnvironmentDescriptor,
    observation: &ContainerRuntimeObservation,
) -> (
    ContainerEnvironmentSelection,
    BTreeSet<String>,
    &'static str,
    &'static str,
) {
    let none = BTreeSet::new();
    if descriptor.validate().is_err() || observation.validate().is_err() {
        return (
            ContainerEnvironmentSelection::Rejected,
            none,
            "container_environment_descriptor_invalid",
            "repair the descriptor; an invalid descriptor is never treated as a usable environment",
        );
    }
    if !observation.available {
        return (
            ContainerEnvironmentSelection::Unavailable,
            none,
            "container_environment_unavailable",
            "install the runtime or select a different backend; the host is not used instead",
        );
    }
    if observation.observed_runtime != Some(descriptor.runtime) {
        return (
            ContainerEnvironmentSelection::Unavailable,
            none,
            "container_environment_runtime_mismatch",
            "the observed runtime does not match the descriptor; the descriptor is not rewritten",
        );
    }
    if !observation.image_present {
        return (
            ContainerEnvironmentSelection::Unavailable,
            none,
            "container_environment_image_unavailable",
            "prefetch the pinned image; fetching at execution time needs its own authorization",
        );
    }
    let mut required = CONTAINER_REQUIRED_DIMENSIONS
        .iter()
        .map(|dimension| (*dimension).to_owned())
        .collect::<BTreeSet<_>>();
    if descriptor.runtime.has_extra_isolation() {
        required.insert(CONTAINER_GVISOR_DIMENSION.to_owned());
    }
    let missing = required
        .difference(&observation.observed_dimensions)
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return (
            ContainerEnvironmentSelection::NotEnforced,
            none,
            "container_environment_required_dimension_unobserved",
            "re-probe the runtime; an unenforced dimension is never assumed present",
        );
    }
    (
        ContainerEnvironmentSelection::Selected,
        required,
        "container_environment_accepted",
        "",
    )
}

/// What happened to the inner process when a stop was requested.
///
/// `Unconfirmed` is the only honest answer when the runtime did not report the inner process as
/// gone. It is a distinct value from `Stopped` precisely so a caller cannot read a stop attempt
/// as a stop.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerStopConfirmation {
    /// The runtime reported the container stopped and the labelled object still matched.
    Stopped,
    /// A stop was attempted and the runtime did not confirm the inner process is gone.
    Unconfirmed,
    /// The runtime could not be asked at all.
    NotObserved,
}

impl ContainerStopConfirmation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Unconfirmed => "unconfirmed",
            Self::NotObserved => "not_observed",
        }
    }

    pub const fn is_confirmed(self) -> bool {
        matches!(self, Self::Stopped)
    }
}

/// What a cancellation or quiesce may be recorded as.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerEffectDisposition {
    /// The stop was confirmed against the labelled object.
    Stopped,
    /// The stop could not be confirmed. The effect is unknown, and the caller must not proceed
    /// as though the environment were released.
    Unknown,
}

impl ContainerEffectDisposition {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Unknown => "unknown",
        }
    }
}

/// Record the result of a stop. A stop that was not confirmed can only produce `Unknown`.
pub fn stop_disposition(confirmation: ContainerStopConfirmation) -> ContainerEffectDisposition {
    if confirmation.is_confirmed() {
        ContainerEffectDisposition::Stopped
    } else {
        ContainerEffectDisposition::Unknown
    }
}

/// The stable reason a stop could not be confirmed. Returned as a string so the caller records
/// the same code the daemon adapter uses for `result_unknown:` receipts.
pub fn unconfirmed_stop_reason(confirmation: ContainerStopConfirmation) -> Option<&'static str> {
    match confirmation {
        ContainerStopConfirmation::Stopped => None,
        ContainerStopConfirmation::Unconfirmed => {
            Some("result_unknown:container_cancel_unconfirmed")
        }
        ContainerStopConfirmation::NotObserved => {
            Some("result_unknown:container_stop_not_observed")
        }
    }
}

/// Whether the container backend is a valid target for this host.
///
/// The container backend is not implemented on every host. This is the same refusal rule the
/// macOS and Windows backends use, expressed through the shared platform vocabulary so a caller
/// does not have to learn a second way of saying "not on this platform".
pub fn container_target_supported(target: PlatformTarget) -> bool {
    matches!(
        target,
        PlatformTarget::Linux | PlatformTarget::Macos | PlatformTarget::Windows
    )
}

/// The facts a caller shows alongside a container receipt, so image, runtime and limits are
/// part of the record rather than an implementation detail.
pub fn container_receipt_facts(
    descriptor: &ContainerEnvironmentDescriptor,
) -> Result<BTreeMap<String, String>, String> {
    descriptor.validate()?;
    Ok(BTreeMap::from([
        ("image".to_owned(), descriptor.image.clone()),
        ("runtime".to_owned(), descriptor.runtime.as_str().to_owned()),
        ("user".to_owned(), descriptor.user.clone()),
        (
            "network_policy".to_owned(),
            descriptor.network_policy.clone(),
        ),
        (
            "limits".to_owned(),
            json_digest(&json!({
                "memory_bytes": descriptor.limits.memory_bytes,
                "cpu_millis": descriptor.limits.cpu_millis,
                "max_processes": descriptor.limits.max_processes,
                "max_output_bytes": descriptor.limits.max_output_bytes,
                "disk_bytes": descriptor.limits.disk_bytes,
            })),
        ),
    ]))
}

fn is_digest_pinned_image(image: &str) -> bool {
    let Some((repository, digest)) = image.rsplit_once("@sha256:") else {
        return false;
    };
    !repository.trim().is_empty()
        && !repository.contains(char::is_whitespace)
        && digest.len() == 64
        && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_non_root_user(user: &str) -> bool {
    let Some((uid, gid)) = user.split_once(':') else {
        return false;
    };
    uid.parse::<u32>().is_ok_and(|value| value > 0)
        && gid.parse::<u32>().is_ok_and(|value| value > 0)
}

/// Host locations a container must never see. This mirrors the adapter's own check in
/// `kiana-daemon::container_environment::is_forbidden_host_mount`; the two are asserted to agree
/// by the CAP-33 source guard, so a change to one without the other fails CI rather than
/// silently widening what a container can read.
fn is_forbidden_host_mount(host_path: &str) -> bool {
    let normalized = host_path.trim_end_matches('/');
    let normalized = if normalized.is_empty() {
        "/"
    } else {
        normalized
    };
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
