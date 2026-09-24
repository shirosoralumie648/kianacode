//! Versioned command contracts for the extension management surface.
//!
//! The command is a wire-level intent only.  It does not load a package, execute a hook, or
//! grant a capability.  Read-only commands may be issued from any authenticated UI surface;
//! registry mutations must carry the operator, reason, idempotency key and expected registry
//! version so the ControlPlane can perform the final authorization and CAS check.

use crate::SchemaVersion;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const EXTENSION_COMMAND_SCHEMA: &str = "kiana.extension-command.v1";
pub const EXTENSION_COMMAND_RECEIPT_SCHEMA: &str = "kiana.extension-command-receipt.v1";
pub const EXTENSION_COMMAND_ERROR_SCHEMA: &str = "kiana.extension-command-error.v1";
pub const EXTENSION_COMMAND_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

const MAX_REASON_BYTES: usize = 4 * 1024;
const MAX_QUERY_BYTES: usize = 256;
const MAX_IDEMPOTENCY_BYTES: usize = 128;

/// Commands exposed by CLI, Workbench, Web and Desktop.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionCommand {
    List,
    Inspect,
    Install,
    Enable,
    Disable,
    Revoke,
    Rollback,
}

impl ExtensionCommand {
    pub const ALL: [Self; 7] = [
        Self::List,
        Self::Inspect,
        Self::Install,
        Self::Enable,
        Self::Disable,
        Self::Revoke,
        Self::Rollback,
    ];

    pub const fn action(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Inspect => "inspect",
            Self::Install => "install",
            Self::Enable => "enable",
            Self::Disable => "disable",
            Self::Revoke => "revoke",
            Self::Rollback => "rollback",
        }
    }

    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::List => "extension.list",
            Self::Inspect => "extension.inspect",
            Self::Install => "extension.install",
            Self::Enable => "extension.enable",
            Self::Disable => "extension.disable",
            Self::Revoke => "extension.revoke",
            Self::Rollback => "extension.rollback",
        }
    }

    pub const fn is_read_only(self) -> bool {
        matches!(self, Self::List | Self::Inspect)
    }

    pub const fn requires_package(self) -> bool {
        matches!(self, Self::Install | Self::Rollback)
    }

    pub fn from_wire_name(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|command| command.wire_name() == value)
    }
}

/// A typed command carried inside the generic protocol command envelope.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionCommandRequest {
    pub schema: String,
    pub version: SchemaVersion,
    pub command: ExtensionCommand,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extension_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_registry_version: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_results: Option<u16>,
}

impl ExtensionCommandRequest {
    pub fn new(command: ExtensionCommand) -> Self {
        Self {
            schema: EXTENSION_COMMAND_SCHEMA.to_owned(),
            version: EXTENSION_COMMAND_VERSION,
            command,
            extension_id: None,
            package_path: None,
            package_sha256: None,
            expected_registry_version: None,
            actor_id: None,
            reason: None,
            idempotency_key: None,
            query: None,
            max_results: None,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_COMMAND_SCHEMA
            || !self.version.is_compatible_with(&EXTENSION_COMMAND_VERSION)
        {
            return Err("extension_command_schema_invalid".to_owned());
        }
        if self
            .extension_id
            .as_deref()
            .is_some_and(|value| !crate::valid_extension_identifier(value))
        {
            return Err("extension_command_extension_id_invalid".to_owned());
        }
        if self
            .package_path
            .as_deref()
            .is_some_and(|value| !crate::valid_extension_path(value))
        {
            return Err("extension_command_package_path_invalid".to_owned());
        }
        if self
            .package_sha256
            .as_deref()
            .is_some_and(|value| !crate::is_sha256_hex(value))
        {
            return Err("extension_command_package_hash_invalid".to_owned());
        }
        if self.actor_id.as_deref().is_some_and(|value| {
            value.trim().is_empty() || value.len() > 256 || value.contains('\0')
        }) {
            return Err("extension_command_actor_invalid".to_owned());
        }
        if self.reason.as_deref().is_some_and(|value| {
            value.trim().is_empty() || value.len() > MAX_REASON_BYTES || value.contains('\0')
        }) {
            return Err("extension_command_reason_invalid".to_owned());
        }
        if self.idempotency_key.as_deref().is_some_and(|value| {
            value.trim().is_empty()
                || value.len() > MAX_IDEMPOTENCY_BYTES
                || value.chars().any(char::is_control)
        }) {
            return Err("extension_command_idempotency_key_invalid".to_owned());
        }
        if self
            .query
            .as_deref()
            .is_some_and(|value| value.len() > MAX_QUERY_BYTES || value.contains('\0'))
        {
            return Err("extension_command_query_invalid".to_owned());
        }
        if self
            .max_results
            .is_some_and(|value| value == 0 || value > 512)
        {
            return Err("extension_command_max_results_invalid".to_owned());
        }

        if self.command.is_read_only() {
            if self.expected_registry_version.is_some()
                || self.actor_id.is_some()
                || self.reason.is_some()
                || self.idempotency_key.is_some()
            {
                return Err("extension_command_read_only_mutation_fields".to_owned());
            }
            if matches!(self.command, ExtensionCommand::Inspect) && self.extension_id.is_none() {
                return Err("extension_command_extension_id_required".to_owned());
            }
        } else {
            if self.extension_id.is_none()
                || self.expected_registry_version.is_none()
                || self.actor_id.is_none()
                || self.reason.is_none()
                || self.idempotency_key.is_none()
            {
                return Err("extension_command_mutation_fields_required".to_owned());
            }
            if self.command.requires_package()
                && (self.package_path.is_none() || self.package_sha256.is_none())
            {
                return Err("extension_command_package_required".to_owned());
            }
            if self.expected_registry_version == Some(0) {
                return Err("extension_command_expected_registry_version_invalid".to_owned());
            }
        }
        Ok(())
    }

    /// Encode the typed request as arguments for the shared `extension.manage` ControlPlane
    /// route.  The route remains the sole authorization and execution spine.
    pub fn to_arguments(&self) -> Result<Value, String> {
        self.validate()?;
        let mut arguments = Map::new();
        arguments.insert("schema".to_owned(), Value::String(self.schema.clone()));
        arguments.insert(
            "version".to_owned(),
            serde_json::to_value(self.version)
                .map_err(|_| "extension_command_version_encode".to_owned())?,
        );
        arguments.insert(
            "action".to_owned(),
            Value::String(self.command.action().to_owned()),
        );
        for (name, value) in [
            ("extension_id", self.extension_id.as_ref()),
            ("package_path", self.package_path.as_ref()),
            ("package_sha256", self.package_sha256.as_ref()),
            ("actor_id", self.actor_id.as_ref()),
            ("reason", self.reason.as_ref()),
            ("idempotency_key", self.idempotency_key.as_ref()),
            ("query", self.query.as_ref()),
        ] {
            if let Some(value) = value {
                arguments.insert(name.to_owned(), Value::String(value.clone()));
            }
        }
        if let Some(value) = self.expected_registry_version {
            arguments.insert("expected_registry_version".to_owned(), value.into());
        }
        if let Some(value) = self.max_results {
            arguments.insert("max_results".to_owned(), u64::from(value).into());
        }
        Ok(Value::Object(arguments))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionCommandErrorCode {
    InvalidRequest,
    PermissionDenied,
    ApprovalRequired,
    Conflict,
    StaleSnapshot,
    Unsupported,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionCommandError {
    pub schema: String,
    pub code: ExtensionCommandErrorCode,
    pub message: String,
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_registry_version: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_registry_version: Option<u64>,
}

impl ExtensionCommandError {
    pub fn new(
        code: ExtensionCommandErrorCode,
        message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            schema: EXTENSION_COMMAND_ERROR_SCHEMA.to_owned(),
            code,
            message: message.into(),
            retryable,
            expected_registry_version: None,
            current_registry_version: None,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_COMMAND_ERROR_SCHEMA
            || self.message.trim().is_empty()
            || self.message.len() > 2_048
        {
            return Err("extension_command_error_invalid".to_owned());
        }
        Ok(())
    }
}

/// Receipt projection returned for a mutation. It is evidence of the committed registry event,
/// not permission for a UI to execute a Hook or resolve a package locally.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionCommandReceipt {
    pub schema: String,
    pub command: ExtensionCommand,
    pub extension_id: String,
    pub actor_id: String,
    pub reason: String,
    pub idempotency_key: String,
    pub expected_registry_version: u64,
    pub registry_version: u64,
    pub event_id: String,
    pub replayed: bool,
}

impl ExtensionCommandReceipt {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_COMMAND_RECEIPT_SCHEMA
            || !crate::valid_extension_identifier(&self.extension_id)
            || self.actor_id.trim().is_empty()
            || self.reason.trim().is_empty()
            || self.idempotency_key.trim().is_empty()
            || self.expected_registry_version >= self.registry_version
            || self.event_id.trim().is_empty()
        {
            return Err("extension_command_receipt_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionCommandResponse {
    pub schema: String,
    pub command: ExtensionCommand,
    pub registry_version: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<ExtensionCommandReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ExtensionCommandError>,
}

impl ExtensionCommandResponse {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_COMMAND_SCHEMA || self.registry_version == 0 {
            return Err("extension_command_response_invalid".to_owned());
        }
        if let Some(receipt) = &self.receipt {
            receipt.validate()?;
        }
        if let Some(error) = &self.error {
            error.validate()?;
        }
        Ok(())
    }
}
