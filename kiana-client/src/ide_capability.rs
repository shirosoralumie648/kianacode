//! IDE/editor/terminal capability boundary.
//!
//! The editor host is a display/input peer. This adapter validates bounded references and turns
//! every mutation into the existing `UiActionV1` intent; it never reads files, spawns a terminal,
//! consumes a permit, or calls a Broker.

use kiana_domain::{json_digest, RequestId};
use kiana_protocol::{
    UiActionV1, UiHostCapability, UiHostCapabilityDisposition, UI_ACTION_SCHEMA,
    UI_HOST_CAPABILITY_SCHEMA,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

pub const IDE_CAPABILITY_SCHEMA: &str = "kiana.ide-capability.v1";
pub const IDE_PERMIT_SCHEMA: &str = "kiana.ide-permit.v1";
pub const IDE_MAX_PATH_BYTES: usize = 4_096;
pub const IDE_MAX_OUTPUT_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdeOperation {
    OpenFile,
    ReadFile,
    PatchPreview,
    ApplyPatch,
    TerminalOutput,
    CancelTerminal,
}

impl IdeOperation {
    fn wire(self) -> &'static str {
        match self {
            Self::OpenFile => "editor.open_file",
            Self::ReadFile => "editor.read_file",
            Self::PatchPreview => "editor.patch_preview",
            Self::ApplyPatch => "editor.apply",
            Self::TerminalOutput => "terminal.output",
            Self::CancelTerminal => "terminal.cancel",
        }
    }

    fn is_mutation(self) -> bool {
        matches!(self, Self::ApplyPatch | Self::CancelTerminal)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdeCapabilityRequest {
    pub schema: String,
    pub operation: IdeOperation,
    pub workspace_digest: String,
    pub relative_path: String,
    pub path_digest: String,
    pub expected_revision: u64,
    pub idempotency_key: String,
    #[serde(default)]
    pub artifact_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdeCapabilityPermit {
    pub schema: String,
    pub permit_id: String,
    pub operation: IdeOperation,
    pub workspace_digest: String,
    pub path_digest: String,
    pub expected_revision: u64,
    pub owner_digest: String,
    pub permit_digest: String,
    pub expires_at_unix_ms: u64,
    pub effect_allowed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IdeIntent {
    Query(IdeCapabilityRequest),
    Action(UiActionV1),
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum IdeCapabilityError {
    #[error("ide_schema_invalid")]
    SchemaInvalid,
    #[error("ide_path_invalid")]
    PathInvalid,
    #[error("ide_digest_invalid:{0}")]
    DigestInvalid(&'static str),
    #[error("ide_scope_mismatch")]
    ScopeMismatch,
    #[error("ide_revision_mismatch")]
    RevisionMismatch,
    #[error("ide_permit_invalid:{0}")]
    PermitInvalid(&'static str),
    #[error("ide_permit_expired")]
    PermitExpired,
    #[error("ide_direct_effect_forbidden")]
    DirectEffectForbidden,
    #[error("ide_output_bound_exceeded")]
    OutputBoundExceeded,
    #[error("ide_action_invalid:{0}")]
    ActionInvalid(String),
}

pub struct IdeCapabilityAdapter {
    scope_digest: String,
    owner_digest: String,
}

impl IdeCapabilityAdapter {
    pub fn new(
        scope_digest: impl Into<String>,
        owner_digest: impl Into<String>,
    ) -> Result<Self, IdeCapabilityError> {
        let scope_digest = digest(scope_digest.into(), "scope")?;
        let owner_digest = digest(owner_digest.into(), "owner")?;
        Ok(Self {
            scope_digest,
            owner_digest,
        })
    }

    pub fn capability_matrix(&self) -> Result<Vec<UiHostCapability>, IdeCapabilityError> {
        let mut rows = Vec::new();
        for (capability_id, actions) in [
            (
                "ide.editor",
                vec![
                    "editor.open_file",
                    "editor.read_file",
                    "editor.patch_preview",
                    "editor.apply",
                ],
            ),
            ("ide.terminal", vec!["terminal.output", "terminal.cancel"]),
        ] {
            let row = UiHostCapability {
                schema: UI_HOST_CAPABILITY_SCHEMA.to_owned(),
                capability_id: capability_id.to_owned(),
                actions: actions.into_iter().map(str::to_owned).collect(),
                scope_digest: self.scope_digest.clone(),
                disposition: UiHostCapabilityDisposition::Advertised,
                direct_effect: false,
                delegated_to_kiana: true,
                reason: None,
            };
            row.validate()
                .map_err(|_| IdeCapabilityError::SchemaInvalid)?;
            rows.push(row);
        }
        Ok(rows)
    }

    pub fn query(&self, request: IdeCapabilityRequest) -> Result<IdeIntent, IdeCapabilityError> {
        self.validate_request(&request)?;
        if request.operation.is_mutation() {
            return Err(IdeCapabilityError::PermitInvalid("query_mutation"));
        }
        Ok(IdeIntent::Query(request))
    }

    pub fn apply(
        &self,
        request: IdeCapabilityRequest,
        permit: IdeCapabilityPermit,
        now_unix_ms: u64,
    ) -> Result<IdeIntent, IdeCapabilityError> {
        self.validate_request(&request)?;
        if request.operation != IdeOperation::ApplyPatch {
            return Err(IdeCapabilityError::PermitInvalid("apply_operation"));
        }
        self.validate_permit(&request, &permit, now_unix_ms)?;
        Ok(IdeIntent::Action(self.action(
            &request,
            json!({"kind":"editor.apply","path":request.relative_path,"path_digest":request.path_digest,"expected_revision":request.expected_revision}),
        )?))
    }

    pub fn cancel_terminal(
        &self,
        request: IdeCapabilityRequest,
        permit: IdeCapabilityPermit,
        now_unix_ms: u64,
    ) -> Result<IdeIntent, IdeCapabilityError> {
        self.validate_request(&request)?;
        if request.operation != IdeOperation::CancelTerminal {
            return Err(IdeCapabilityError::PermitInvalid("cancel_operation"));
        }
        self.validate_permit(&request, &permit, now_unix_ms)?;
        Ok(IdeIntent::Action(self.action(
            &request,
            json!({"kind":"terminal.cancel","path":request.relative_path,"expected_revision":request.expected_revision}),
        )?))
    }

    pub fn terminal_output(
        &self,
        request: IdeCapabilityRequest,
        output_bytes: usize,
    ) -> Result<IdeIntent, IdeCapabilityError> {
        self.validate_request(&request)?;
        if request.operation != IdeOperation::TerminalOutput {
            return Err(IdeCapabilityError::PermitInvalid("terminal_operation"));
        }
        if output_bytes > IDE_MAX_OUTPUT_BYTES {
            return Err(IdeCapabilityError::OutputBoundExceeded);
        }
        Ok(IdeIntent::Query(request))
    }

    fn validate_request(&self, request: &IdeCapabilityRequest) -> Result<(), IdeCapabilityError> {
        if request.schema != IDE_CAPABILITY_SCHEMA || request.expected_revision == 0 {
            return Err(IdeCapabilityError::SchemaInvalid);
        }
        relative_path(&request.relative_path)?;
        digest(request.workspace_digest.clone(), "workspace")?;
        digest(request.path_digest.clone(), "path")?;
        if request.workspace_digest != self.scope_digest {
            return Err(IdeCapabilityError::ScopeMismatch);
        }
        if request.idempotency_key.trim().is_empty() || request.idempotency_key.len() > 256 {
            return Err(IdeCapabilityError::SchemaInvalid);
        }
        Ok(())
    }

    fn validate_permit(
        &self,
        request: &IdeCapabilityRequest,
        permit: &IdeCapabilityPermit,
        now_unix_ms: u64,
    ) -> Result<(), IdeCapabilityError> {
        if permit.schema != IDE_PERMIT_SCHEMA || permit.permit_id.trim().is_empty() {
            return Err(IdeCapabilityError::PermitInvalid("schema"));
        }
        if permit.operation != request.operation
            || permit.workspace_digest != request.workspace_digest
            || permit.path_digest != request.path_digest
            || permit.expected_revision != request.expected_revision
        {
            return Err(IdeCapabilityError::PermitInvalid("binding"));
        }
        if permit.owner_digest != self.owner_digest {
            return Err(IdeCapabilityError::PermitInvalid("owner"));
        }
        digest(permit.permit_digest.clone(), "permit")?;
        if permit.expires_at_unix_ms <= now_unix_ms {
            return Err(IdeCapabilityError::PermitExpired);
        }
        if !permit.effect_allowed {
            return Err(IdeCapabilityError::DirectEffectForbidden);
        }
        Ok(())
    }

    fn action(
        &self,
        request: &IdeCapabilityRequest,
        payload: Value,
    ) -> Result<UiActionV1, IdeCapabilityError> {
        let action = UiActionV1 {
            schema: UI_ACTION_SCHEMA.to_owned(),
            command_id: RequestId::new(),
            idempotency_key: request.idempotency_key.clone(),
            target_id: request.relative_path.clone(),
            expected_epoch: request.workspace_digest.clone(),
            expected_cursor: request.expected_revision,
            expected_revision: Some(request.expected_revision),
            payload: payload.clone(),
            payload_digest: json_digest(&payload),
            submitted_by: self.owner_digest.clone(),
            deadline_unix_ms: None,
        };
        action
            .validate()
            .map_err(IdeCapabilityError::ActionInvalid)?;
        Ok(action)
    }
}

fn relative_path(value: &str) -> Result<(), IdeCapabilityError> {
    if value.trim().is_empty()
        || value.len() > IDE_MAX_PATH_BYTES
        || value.contains('\0')
        || value.starts_with('/')
        || value.starts_with('\\')
        || value
            .split(['/', '\\'])
            .any(|part| part == "." || part == ".." || part.is_empty())
    {
        return Err(IdeCapabilityError::PathInvalid);
    }
    Ok(())
}

fn digest(value: String, field: &'static str) -> Result<String, IdeCapabilityError> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(IdeCapabilityError::DigestInvalid(field));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(IdeCapabilityError::DigestInvalid(field));
    }
    Ok(value)
}
