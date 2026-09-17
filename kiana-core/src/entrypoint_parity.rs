//! Entry-point-neutral command normalization and parity matrix.
//!
//! CLI, TTY, Web, Workbench, Desktop, scheduler, swarm and connector adapters can label a
//! request, but they cannot choose a different authority path. The normalized command contains
//! only digests and a fixed DaemonHost->ControlPlane route; deny/approval/unknown status is
//! projected from the same decision and handler calls remain zero on deny.

use kiana_domain::{
    json_digest, CommandIntent, EntryPointKind, RequestContext, RequestId, SchemaVersion,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub const ENTRYPOINT_COMMAND_SCHEMA: &str = "kiana.entrypoint-command.v1";
pub const ENTRYPOINT_PARITY_MATRIX_SCHEMA: &str = "kiana.entrypoint-parity-matrix.v1";
pub const ENTRYPOINT_ROUTE: &str = "daemonhost.controlplane";
pub const ENTRYPOINT_PARITY_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_PARITY_COMMANDS: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntrypointDecision {
    Denied,
    AwaitingApproval,
    Allowed,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntrypointCommand {
    pub schema: String,
    pub version: SchemaVersion,
    pub entrypoint: EntryPointKind,
    pub request_id: RequestId,
    pub command_name: String,
    pub arguments_digest: String,
    pub context_digest: String,
    pub command_digest: String,
    pub route: String,
    pub decision: EntrypointDecision,
}

impl EntrypointCommand {
    pub fn new(
        entrypoint: EntryPointKind,
        context: &RequestContext,
        intent: &CommandIntent,
    ) -> Result<Self, String> {
        if intent.name.trim().is_empty() || intent.name.len() > 256 || intent.name.contains('\0') {
            return Err("entrypoint_command_name_invalid".to_owned());
        }
        let context_digest = context_digest(context);
        let arguments_digest = json_digest(&intent.arguments);
        let command_digest = json_digest(&json!({
            "request_id": context.request_id,
            "command_name": intent.name,
            "arguments_digest": arguments_digest,
            "context_digest": context_digest,
            "route": ENTRYPOINT_ROUTE,
        }));
        let command = Self {
            schema: ENTRYPOINT_COMMAND_SCHEMA.to_owned(),
            version: ENTRYPOINT_PARITY_VERSION,
            entrypoint,
            request_id: context.request_id,
            command_name: intent.name.clone(),
            arguments_digest,
            context_digest,
            command_digest,
            route: ENTRYPOINT_ROUTE.to_owned(),
            decision: EntrypointDecision::Unknown,
        };
        command.validate()?;
        Ok(command)
    }

    pub fn with_decision(mut self, decision: EntrypointDecision) -> Result<Self, String> {
        self.decision = decision;
        self.validate()?;
        Ok(self)
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let command: Self = serde_json::from_value(value.clone())
            .map_err(|_| "entrypoint_command_decode_failed".to_owned())?;
        command.validate()?;
        Ok(command)
    }

    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "entrypoint_command_encode_failed".to_owned())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ENTRYPOINT_COMMAND_SCHEMA
            || !self.version.is_compatible_with(&ENTRYPOINT_PARITY_VERSION)
            || self.request_id.as_uuid().is_nil()
            || self.command_name.trim().is_empty()
            || self.command_name.len() > 256
            || self.route != ENTRYPOINT_ROUTE
        {
            return Err("entrypoint_command_header_invalid".to_owned());
        }
        for (digest, field) in [
            (
                &self.arguments_digest,
                "entrypoint_command_arguments_digest",
            ),
            (&self.context_digest, "entrypoint_command_context_digest"),
            (&self.command_digest, "entrypoint_command_digest"),
        ] {
            validate_digest(digest, field)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntrypointParityMatrix {
    pub schema: String,
    pub version: SchemaVersion,
    pub request_id: RequestId,
    pub command_digest: String,
    pub commands: Vec<EntrypointCommand>,
    pub handler_calls: u32,
    pub matrix_digest: String,
}

impl EntrypointParityMatrix {
    pub fn new(mut commands: Vec<EntrypointCommand>, handler_calls: u32) -> Result<Self, String> {
        commands.sort_by_key(|command| format!("{:?}", command.entrypoint));
        let first = commands
            .first()
            .ok_or_else(|| "entrypoint_parity_commands_required".to_owned())?;
        let mut matrix = Self {
            schema: ENTRYPOINT_PARITY_MATRIX_SCHEMA.to_owned(),
            version: ENTRYPOINT_PARITY_VERSION,
            request_id: first.request_id,
            command_digest: first.command_digest.clone(),
            commands,
            handler_calls,
            matrix_digest: String::new(),
        };
        matrix.matrix_digest = matrix.digest();
        matrix.validate()?;
        Ok(matrix)
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let matrix: Self = serde_json::from_value(value.clone())
            .map_err(|_| "entrypoint_parity_matrix_decode_failed".to_owned())?;
        matrix.validate()?;
        Ok(matrix)
    }

    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "entrypoint_parity_matrix_encode_failed".to_owned())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ENTRYPOINT_PARITY_MATRIX_SCHEMA
            || !self.version.is_compatible_with(&ENTRYPOINT_PARITY_VERSION)
            || self.request_id.as_uuid().is_nil()
            || self.commands.is_empty()
            || self.commands.len() > MAX_PARITY_COMMANDS
        {
            return Err("entrypoint_parity_matrix_header_invalid".to_owned());
        }
        let mut entrypoints = BTreeSet::new();
        for command in &self.commands {
            command.validate()?;
            if command.request_id != self.request_id
                || command.command_digest != self.command_digest
            {
                return Err("entrypoint_parity_command_mismatch".to_owned());
            }
            if !entrypoints.insert(format!("{:?}", command.entrypoint)) {
                return Err("entrypoint_parity_duplicate_entrypoint".to_owned());
            }
        }
        if self
            .commands
            .iter()
            .any(|command| matches!(command.decision, EntrypointDecision::Denied))
            && self.handler_calls != 0
        {
            return Err("entrypoint_parity_denied_handler_effect".to_owned());
        }
        validate_digest(&self.command_digest, "entrypoint_parity_command_digest")?;
        validate_digest(&self.matrix_digest, "entrypoint_parity_matrix_digest")?;
        if self.matrix_digest != self.digest() {
            return Err("entrypoint_parity_matrix_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "request_id": self.request_id,
            "command_digest": self.command_digest,
            "commands": self.commands,
            "handler_calls": self.handler_calls,
        }))
    }
}

fn context_digest(context: &RequestContext) -> String {
    json_digest(&json!({
        "request_id": context.request_id,
        "session_id": context.session_id,
        "actor_id": context.actor_id,
        "project_root": context.project_root,
        "project_trusted": context.project_trusted,
        "permission_profile": context.permission_profile,
        "role_id": context.role_id,
        "department_id": context.department_id,
        "work_packet_id": context.work_packet_id,
        "cell_id": context.cell_id,
        "path_allow": context.path_allow,
    }))
}

fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
