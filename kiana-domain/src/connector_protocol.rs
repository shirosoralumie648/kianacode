//! Versioned connector command DTOs and the shared wire normalizer.
//!
//! Connector commands arrive from several surfaces, but they must become one typed intent before
//! entering the ControlPlane.  This module deliberately keeps authority out of the wire shape:
//! actor, role, risk, binding snapshots and endpoints are server-owned values.  A caller can name
//! a binding and operation, but it cannot provide the binding snapshot, lower risk, or select a
//! transport endpoint.  The normalized intent is still only a request; it never calls a Broker.

use crate::{
    AccountBinding, ConnectorBindingSnapshot, ConnectorDefinition, RequestContext, RiskLevel,
    SchemaVersion,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const CONNECTOR_COMMAND_SCHEMA: &str = "kiana.connector-command.v1";
pub const CONNECTOR_COMMAND_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const CONNECTOR_NORMALIZED_INTENT_SCHEMA: &str = "kiana.connector-normalized-intent.v1";
pub const CONNECTOR_PROTOCOL_ERROR_SCHEMA: &str = "kiana.connector-protocol-error.v1";
pub const CONNECTOR_DATA_BOUNDARY_SCHEMA: &str = "kiana.connector-data-boundary.v1";
pub const CONNECTOR_PROTOCOL_MAX_PAYLOAD_BYTES: usize = 64 * 1024;
pub const CONNECTOR_PROTOCOL_MAX_REASON_BYTES: usize = 4 * 1024;
pub const CONNECTOR_PROTOCOL_MAX_IDEMPOTENCY_BYTES: usize = 128;

/// The four public connector commands share one normalized route contract.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorCommand {
    Manage,
    Invoke,
    Health,
    Reconcile,
}

impl ConnectorCommand {
    pub const ALL: [Self; 4] = [Self::Manage, Self::Invoke, Self::Health, Self::Reconcile];

    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Manage => "connector.manage",
            Self::Invoke => "connector.invoke",
            Self::Health => "connector.health",
            Self::Reconcile => "connector.reconcile",
        }
    }

    /// Reconciliation uses the existing management handler and therefore does not create a
    /// second execution path.  The public protocol name remains distinct for typed clients.
    pub const fn control_plane_route(self) -> &'static str {
        match self {
            Self::Reconcile => "connector.manage",
            other => other.wire_name(),
        }
    }

    pub fn from_wire_name(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|command| command.wire_name() == value)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorManageAction {
    List,
    Bind,
    Revoke,
}

impl ConnectorManageAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Bind => "bind",
            Self::Revoke => "revoke",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "list" => Some(Self::List),
            "bind" => Some(Self::Bind),
            "revoke" => Some(Self::Revoke),
            _ => None,
        }
    }
}

/// A typed request carried inside the generic `kiana.protocol.v1` command envelope.
///
/// The optional authority-shaped fields exist only to make downgrade/impersonation attempts
/// explicit and diagnosable.  `validate` rejects them; they are never copied into a normalized
/// intent.  `binding` and `definition` are accepted only for the server-side `manage.bind` input;
/// an invocation must use a server-resolved `binding_snapshot` instead.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorCommandRequest {
    pub schema: String,
    pub version: SchemaVersion,
    pub command: ConnectorCommand,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<ConnectorManageAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_registry_version: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition: Option<ConnectorDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding: Option<AccountBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation_event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_sha256: Option<String>,
    /// Explicitly parsed so a forged value receives a stable authority error instead of being
    /// silently ignored.  These four values are never accepted as authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk: Option<RiskLevel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// A full snapshot is always server-derived.  It is an explicit deny field rather than an
    /// accepted compatibility escape hatch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding_snapshot: Option<ConnectorBindingSnapshot>,
}

impl ConnectorCommandRequest {
    pub fn new(command: ConnectorCommand) -> Self {
        Self {
            schema: CONNECTOR_COMMAND_SCHEMA.to_owned(),
            version: CONNECTOR_COMMAND_VERSION,
            command,
            action: None,
            binding_id: None,
            operation: None,
            payload: None,
            idempotency_key: None,
            expected_registry_version: None,
            reason: None,
            definition: None,
            binding: None,
            invocation_event_id: None,
            receipt_path: None,
            receipt_sha256: None,
            actor_id: None,
            role_id: None,
            risk: None,
            endpoint: None,
            binding_snapshot: None,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_COMMAND_SCHEMA
            || !self.version.is_compatible_with(&CONNECTOR_COMMAND_VERSION)
        {
            return Err("connector_protocol_schema_invalid".to_owned());
        }
        if self.actor_id.is_some()
            || self.role_id.is_some()
            || self.risk.is_some()
            || self.endpoint.is_some()
            || self.binding_snapshot.is_some()
        {
            return Err("connector_server_owned_override".to_owned());
        }
        if self
            .binding_id
            .as_deref()
            .is_some_and(|value| !valid_identifier(value))
        {
            return Err("connector_binding_id_invalid".to_owned());
        }
        if self
            .operation
            .as_deref()
            .is_some_and(|value| !valid_identifier(value))
        {
            return Err("connector_operation_invalid".to_owned());
        }
        if self.idempotency_key.as_deref().is_some_and(|value| {
            value.trim().is_empty()
                || value.len() > CONNECTOR_PROTOCOL_MAX_IDEMPOTENCY_BYTES
                || value.chars().any(char::is_control)
        }) {
            return Err("connector_idempotency_key_invalid".to_owned());
        }
        if self.reason.as_deref().is_some_and(|value| {
            value.trim().is_empty()
                || value.len() > CONNECTOR_PROTOCOL_MAX_REASON_BYTES
                || value.contains(['\0', '\r', '\n'])
        }) {
            return Err("connector_reason_invalid".to_owned());
        }
        for (field, value) in [
            ("invocation_event_id", self.invocation_event_id.as_deref()),
            ("receipt_path", self.receipt_path.as_deref()),
            ("receipt_sha256", self.receipt_sha256.as_deref()),
        ] {
            if value.is_some_and(|value| {
                value.trim().is_empty() || value.len() > 1024 || value.contains(['\0', '\r', '\n'])
            }) {
                return Err(format!("connector_{field}_invalid"));
            }
        }
        if let Some(payload) = &self.payload {
            let bytes =
                serde_json::to_vec(payload).map_err(|_| "connector_payload_invalid".to_owned())?;
            if bytes.len() > CONNECTOR_PROTOCOL_MAX_PAYLOAD_BYTES {
                return Err("connector_payload_too_large".to_owned());
            }
        }

        match self.command {
            ConnectorCommand::Manage => {
                let action = self.action.ok_or("connector_action_required")?;
                if self.operation.is_some()
                    || self.payload.is_some()
                    || self.invocation_event_id.is_some()
                    || self.receipt_path.is_some()
                    || self.receipt_sha256.is_some()
                {
                    return Err("connector_manage_arguments_invalid".to_owned());
                }
                match action {
                    ConnectorManageAction::List => {
                        if self.binding_id.is_some()
                            || self.definition.is_some()
                            || self.binding.is_some()
                            || self.expected_registry_version.is_some()
                            || self.reason.is_some()
                            || self.idempotency_key.is_some()
                        {
                            return Err("connector_manage_list_mutation_fields".to_owned());
                        }
                    }
                    ConnectorManageAction::Bind => {
                        if self.binding_id.is_some()
                            || self.definition.is_none()
                            || self.binding.is_none()
                            || self.expected_registry_version.is_none()
                            || self.reason.is_none()
                            || self.idempotency_key.is_none()
                        {
                            return Err("connector_bind_fields_required".to_owned());
                        }
                        self.definition
                            .as_ref()
                            .zip(self.binding.as_ref())
                            .ok_or("connector_bind_fields_required")?;
                    }
                    ConnectorManageAction::Revoke => {
                        if self.binding_id.is_none()
                            || self.definition.is_some()
                            || self.binding.is_some()
                            || self.expected_registry_version.is_none()
                            || self.reason.is_none()
                            || self.idempotency_key.is_none()
                        {
                            return Err("connector_revoke_fields_required".to_owned());
                        }
                    }
                }
            }
            ConnectorCommand::Invoke => {
                if self.binding_id.is_none()
                    || self.operation.is_none()
                    || self.payload.is_none()
                    || self.idempotency_key.is_none()
                    || self.action.is_some()
                    || self.definition.is_some()
                    || self.binding.is_some()
                    || self.expected_registry_version.is_some()
                    || self.reason.is_some()
                    || self.invocation_event_id.is_some()
                    || self.receipt_path.is_some()
                    || self.receipt_sha256.is_some()
                {
                    return Err("connector_invoke_fields_invalid".to_owned());
                }
            }
            ConnectorCommand::Health => {
                if self.binding_id.is_none()
                    || self.action.is_some()
                    || self.operation.is_some()
                    || self.payload.is_some()
                    || self.idempotency_key.is_some()
                    || self.expected_registry_version.is_some()
                    || self.reason.is_some()
                    || self.definition.is_some()
                    || self.binding.is_some()
                    || self.invocation_event_id.is_some()
                    || self.receipt_path.is_some()
                    || self.receipt_sha256.is_some()
                {
                    return Err("connector_health_fields_invalid".to_owned());
                }
            }
            ConnectorCommand::Reconcile => {
                if self.invocation_event_id.is_none()
                    || self.receipt_path.is_none()
                    || self.receipt_sha256.is_none()
                    || self.idempotency_key.is_none()
                    || self.expected_registry_version.is_none()
                    || self.reason.is_none()
                    || self.action.is_some()
                    || self.binding_id.is_some()
                    || self.operation.is_some()
                    || self.payload.is_some()
                    || self.definition.is_some()
                    || self.binding.is_some()
                {
                    return Err("connector_reconcile_fields_invalid".to_owned());
                }
            }
        }
        Ok(())
    }

    /// Encode the typed request into the single generic command route.
    pub fn to_arguments(&self) -> Result<Value, String> {
        self.validate()?;
        let mut output = Map::new();
        output.insert("schema".to_owned(), Value::String(self.schema.clone()));
        output.insert(
            "version".to_owned(),
            serde_json::to_value(self.version)
                .map_err(|_| "connector_protocol_version_encode".to_owned())?,
        );
        output.insert(
            "command".to_owned(),
            serde_json::to_value(self.command)
                .map_err(|_| "connector_command_encode".to_owned())?,
        );
        for (key, value) in [
            (
                "binding_id",
                self.binding_id.as_ref().map(|v| Value::String(v.clone())),
            ),
            (
                "operation",
                self.operation.as_ref().map(|v| Value::String(v.clone())),
            ),
            (
                "idempotency_key",
                self.idempotency_key
                    .as_ref()
                    .map(|v| Value::String(v.clone())),
            ),
            (
                "reason",
                self.reason.as_ref().map(|v| Value::String(v.clone())),
            ),
            (
                "invocation_event_id",
                self.invocation_event_id
                    .as_ref()
                    .map(|v| Value::String(v.clone())),
            ),
            (
                "receipt_path",
                self.receipt_path.as_ref().map(|v| Value::String(v.clone())),
            ),
            (
                "receipt_sha256",
                self.receipt_sha256
                    .as_ref()
                    .map(|v| Value::String(v.clone())),
            ),
        ] {
            if let Some(value) = value {
                output.insert(key.to_owned(), value);
            }
        }
        if let Some(action) = self.action {
            output.insert(
                "action".to_owned(),
                Value::String(action.as_str().to_owned()),
            );
        }
        if let Some(payload) = &self.payload {
            output.insert("payload".to_owned(), payload.clone());
        }
        if let Some(version) = self.expected_registry_version {
            output.insert("expected_registry_version".to_owned(), Value::from(version));
        }
        if let Some(definition) = &self.definition {
            output.insert(
                "definition".to_owned(),
                serde_json::to_value(definition)
                    .map_err(|_| "connector_definition_encode".to_owned())?,
            );
        }
        if let Some(binding) = &self.binding {
            output.insert(
                "binding".to_owned(),
                serde_json::to_value(binding).map_err(|_| "connector_binding_encode".to_owned())?,
            );
        }
        Ok(Value::Object(output))
    }
}

/// Server-owned data boundary attached to every normalized connector intent.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorDataBoundary {
    pub schema: String,
    pub project_root: String,
    pub allow_external: bool,
}

impl ConnectorDataBoundary {
    fn for_context(context: &RequestContext) -> Result<Self, ConnectorProtocolError> {
        if context.project_root.trim().is_empty() || context.project_root.len() > 4096 {
            return Err(ConnectorProtocolError::invalid("connector_project_invalid"));
        }
        Ok(Self {
            schema: CONNECTOR_DATA_BOUNDARY_SCHEMA.to_owned(),
            project_root: context.project_root.clone(),
            // INT-14 only admits the existing local_fixture/stdio source path.
            allow_external: false,
        })
    }
}

/// The canonical, server-shaped intent shared by CLI/Web/Workbench/MCP adapters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorNormalizedIntent {
    pub schema: String,
    pub version: SchemaVersion,
    pub command: ConnectorCommand,
    pub route: String,
    pub arguments: Value,
    pub actor_id: String,
    pub role_id: String,
    pub department_id: String,
    pub data_boundary: ConnectorDataBoundary,
}

impl ConnectorNormalizedIntent {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_NORMALIZED_INTENT_SCHEMA
            || !self.version.is_compatible_with(&CONNECTOR_COMMAND_VERSION)
            || self.route != self.command.control_plane_route()
            || self.actor_id.trim().is_empty()
            || self.role_id.trim().is_empty()
            || self.department_id.trim().is_empty()
            || self.data_boundary.schema != CONNECTOR_DATA_BOUNDARY_SCHEMA
            || self.data_boundary.project_root.trim().is_empty()
            || self.data_boundary.allow_external
            || !self.arguments.is_object()
        {
            return Err("connector_normalized_intent_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorProtocolErrorCode {
    InvalidRequest,
    ProjectUntrusted,
    AuthenticationRequired,
    AuthorityOverride,
    BindingRequired,
    OperationRequired,
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorProtocolError {
    pub schema: String,
    pub code: ConnectorProtocolErrorCode,
    pub reason: String,
    pub broker_calls: u32,
}

impl ConnectorProtocolError {
    pub fn invalid(reason: impl Into<String>) -> Self {
        Self {
            schema: CONNECTOR_PROTOCOL_ERROR_SCHEMA.to_owned(),
            code: ConnectorProtocolErrorCode::InvalidRequest,
            reason: reason.into(),
            broker_calls: 0,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_PROTOCOL_ERROR_SCHEMA
            || self.reason.trim().is_empty()
            || self.reason.len() > 256
            || self.broker_calls != 0
        {
            return Err("connector_protocol_error_invalid".to_owned());
        }
        Ok(())
    }
}

/// Normalize every connector wire request before ControlPlane admission.  It only validates and
/// stamps server context; binding lookup, policy, approval and Broker dispatch remain in Core.
pub fn normalize_connector_intent(
    wire_name: &str,
    arguments: Value,
    context: &RequestContext,
) -> Result<ConnectorNormalizedIntent, ConnectorProtocolError> {
    let command = ConnectorCommand::from_wire_name(wire_name)
        .ok_or_else(|| ConnectorProtocolError::invalid("connector_command_invalid"))?;
    if !context.project_trusted {
        return Err(ConnectorProtocolError {
            schema: CONNECTOR_PROTOCOL_ERROR_SCHEMA.to_owned(),
            code: ConnectorProtocolErrorCode::ProjectUntrusted,
            reason: "project_untrusted".to_owned(),
            broker_calls: 0,
        });
    }
    if context.cell_id.is_some() {
        return Err(ConnectorProtocolError::invalid(
            "connector_operator_required",
        ));
    }
    let actor_id = context
        .actor_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ConnectorProtocolError {
            schema: CONNECTOR_PROTOCOL_ERROR_SCHEMA.to_owned(),
            code: ConnectorProtocolErrorCode::AuthenticationRequired,
            reason: "connector_authentication_required".to_owned(),
            broker_calls: 0,
        })?
        .to_owned();
    if context.role_id.trim().is_empty() || context.department_id.trim().is_empty() {
        return Err(ConnectorProtocolError::invalid(
            "connector_role_context_required",
        ));
    }
    let mut arguments = arguments;
    let object = arguments
        .as_object_mut()
        .ok_or_else(|| ConnectorProtocolError::invalid("connector_arguments_invalid"))?;
    // Generic CommandRequest values are converted through the same typed DTO at every surface.
    // The envelope route and the nested discriminator must agree; neither is caller authority.
    let expected_command = serde_json::to_value(command)
        .map_err(|_| ConnectorProtocolError::invalid("connector_command_encode"))?;
    if object.get("command") != Some(&expected_command) {
        return Err(ConnectorProtocolError::invalid(
            "connector_command_mismatch",
        ));
    }
    let request: ConnectorCommandRequest = serde_json::from_value(arguments)
        .map_err(|_| ConnectorProtocolError::invalid("connector_protocol_decode_failed"))?;
    request
        .validate()
        .map_err(|reason| ConnectorProtocolError {
            schema: CONNECTOR_PROTOCOL_ERROR_SCHEMA.to_owned(),
            code: if reason == "connector_server_owned_override" {
                ConnectorProtocolErrorCode::AuthorityOverride
            } else {
                ConnectorProtocolErrorCode::InvalidRequest
            },
            reason,
            broker_calls: 0,
        })?;
    if request.command != command {
        return Err(ConnectorProtocolError::invalid(
            "connector_command_mismatch",
        ));
    }
    let data_boundary = ConnectorDataBoundary::for_context(context)?;
    let intent = ConnectorNormalizedIntent {
        schema: CONNECTOR_NORMALIZED_INTENT_SCHEMA.to_owned(),
        version: CONNECTOR_COMMAND_VERSION,
        command,
        route: command.control_plane_route().to_owned(),
        arguments: request
            .to_arguments()
            .map_err(ConnectorProtocolError::invalid)?,
        actor_id,
        role_id: context.role_id.clone(),
        department_id: context.department_id.clone(),
        data_boundary,
    };
    intent.validate().map_err(ConnectorProtocolError::invalid)?;
    Ok(intent)
}

fn valid_identifier(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}
