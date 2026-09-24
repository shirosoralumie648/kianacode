//! Domain contracts for a stdio MCP connector handshake.
//!
//! MCP advertises untrusted metadata.  The tool list therefore describes what a server
//! claims to expose, but it never creates a Kiana scope or an invocation grant.  Scope
//! and operation binding are derived from the already authorised connector binding and are
//! represented by digests in this contract.

use crate::{is_sha256_hex, json_digest, valid_extension_identifier, validate_schema_contract};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const MCP_STDIO_TRANSPORT: &str = "stdio";
pub const MCP_CAPABILITY_HANDSHAKE_SCHEMA: &str = "kiana.mcp-capability-handshake.v1";
pub const MCP_TOOL_ADVERTISEMENT_SCHEMA: &str = "kiana.mcp-tool-advertisement.v1";
pub const MCP_TOOL_CAPABILITY_SCHEMA: &str = "kiana.mcp-tool-capability.v1";
pub const MCP_MAX_HANDSHAKE_TOOLS: usize = 256;
pub const MCP_MAX_PROTOCOL_VERSION: usize = 64;
pub const MCP_MAX_SESSION_REF: usize = 256;

/// A child process is owned by one invocation.  Terminal states are retained in the handshake
/// evidence so a timeout or disconnect cannot be projected as a successful capability.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpSessionState {
    Starting,
    Ready,
    TimedOut,
    Disconnected,
    Closed,
}

impl McpSessionState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Ready => "ready",
            Self::TimedOut => "timed_out",
            Self::Disconnected => "disconnected",
            Self::Closed => "closed",
        }
    }

    pub const fn terminal(self) -> bool {
        matches!(self, Self::TimedOut | Self::Disconnected | Self::Closed)
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Starting, Self::Ready)
                | (Self::Starting, Self::TimedOut)
                | (Self::Starting, Self::Disconnected)
                | (Self::Starting, Self::Closed)
                | (Self::Ready, Self::TimedOut)
                | (Self::Ready, Self::Disconnected)
                | (Self::Ready, Self::Closed)
                | (Self::TimedOut, Self::Closed)
                | (Self::Disconnected, Self::Closed)
                | (Self::Closed, Self::Closed)
        )
    }
}

/// A server advertisement is input to negotiation only.  `server_requested_scopes` is retained
/// to make the trust boundary explicit and is rejected when non-empty: a server schema cannot
/// turn into a Kiana grant.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpToolAdvertisement {
    pub schema: String,
    pub name: String,
    pub input_schema: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub conditional: bool,
    #[serde(default)]
    pub server_requested_scopes: BTreeSet<String>,
}

impl McpToolAdvertisement {
    pub fn new(name: impl Into<String>, input_schema: Value) -> Result<Self, String> {
        let advertisement = Self {
            schema: MCP_TOOL_ADVERTISEMENT_SCHEMA.to_owned(),
            name: name.into(),
            input_schema,
            output_schema: None,
            description: None,
            conditional: false,
            server_requested_scopes: BTreeSet::new(),
        };
        advertisement.validate()?;
        Ok(advertisement)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MCP_TOOL_ADVERTISEMENT_SCHEMA
            || !valid_extension_identifier(&self.name)
            || self.name.len() > 256
            || self
                .description
                .as_deref()
                .is_some_and(|description| description.len() > 8 * 1024)
            || self
                .server_requested_scopes
                .iter()
                .any(|scope| !valid_extension_identifier(scope))
        {
            return Err("mcp_tool_advertisement_invalid".to_owned());
        }
        if self.input_schema["type"] != "object" {
            return Err("mcp_tool_schema_object_required".to_owned());
        }
        validate_schema_contract(&self.input_schema)
            .map_err(|_| "mcp_tool_schema_unsupported".to_owned())?;
        if let Some(output_schema) = &self.output_schema {
            if output_schema["type"] != "object" {
                return Err("mcp_output_schema_object_required".to_owned());
            }
            validate_schema_contract(output_schema)
                .map_err(|_| "mcp_output_schema_unsupported".to_owned())?;
        }
        if !self.server_requested_scopes.is_empty() {
            return Err("mcp_server_scope_not_authority".to_owned());
        }
        Ok(())
    }

    pub fn input_schema_digest(&self) -> String {
        json_digest(&self.input_schema)
    }

    pub fn output_schema_digest(&self) -> Option<String> {
        self.output_schema.as_ref().map(json_digest)
    }
}

/// The operation contract is server-owned.  It deliberately carries the required scope only
/// while deriving the digest; the handshake never serializes the scope as a server grant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct McpDeclaredTool {
    pub tool_name: String,
    pub operation_id: String,
    pub required_scope: String,
    pub input_schema_digest: String,
    pub output_schema_digest: Option<String>,
    pub conditional: bool,
}

impl McpDeclaredTool {
    pub fn validate(&self) -> Result<(), String> {
        if !valid_extension_identifier(&self.tool_name)
            || self.tool_name.len() > 256
            || !valid_extension_identifier(&self.operation_id)
            || !valid_extension_identifier(&self.required_scope)
            || !valid_digest(&self.input_schema_digest)
            || self
                .output_schema_digest
                .as_deref()
                .is_some_and(|digest| !valid_digest(digest))
        {
            return Err("mcp_declared_tool_invalid".to_owned());
        }
        Ok(())
    }
}

/// A capability is derived from a Kiana operation and binding snapshot.  The only scope-related
/// material crossing the handshake boundary is a digest of the server-owned intersection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpToolCapability {
    pub schema: String,
    pub tool_name: String,
    pub operation_id: String,
    pub input_schema_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema_digest: Option<String>,
    pub binding_scope_digest: String,
    pub available: bool,
    pub conditional: bool,
}

impl McpToolCapability {
    fn from_declared(declared: &McpDeclaredTool, binding_scopes: &BTreeSet<String>) -> Self {
        let binding_scope_digest = json_digest(&json!({
            "source": "kiana.account_binding",
            "required_scope": declared.required_scope,
            "binding_scopes": binding_scopes,
        }));
        Self {
            schema: MCP_TOOL_CAPABILITY_SCHEMA.to_owned(),
            tool_name: declared.tool_name.clone(),
            operation_id: declared.operation_id.clone(),
            input_schema_digest: declared.input_schema_digest.clone(),
            output_schema_digest: declared.output_schema_digest.clone(),
            binding_scope_digest,
            available: binding_scopes.contains(&declared.required_scope),
            conditional: declared.conditional || !binding_scopes.contains(&declared.required_scope),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MCP_TOOL_CAPABILITY_SCHEMA
            || !valid_extension_identifier(&self.tool_name)
            || !valid_extension_identifier(&self.operation_id)
            || !valid_digest(&self.input_schema_digest)
            || !valid_digest(&self.binding_scope_digest)
            || self
                .output_schema_digest
                .as_deref()
                .is_some_and(|digest| !valid_digest(digest))
            || !self.available && !self.conditional
        {
            return Err("mcp_tool_capability_invalid".to_owned());
        }
        Ok(())
    }
}

/// Bounded, secret-free evidence from one stdio initialization and tools/list exchange.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpCapabilityHandshake {
    pub schema: String,
    pub transport: String,
    pub protocol_version: String,
    pub server_info_digest: String,
    pub catalog_digest: String,
    pub session_digest: String,
    pub state: McpSessionState,
    pub tools: Vec<McpToolCapability>,
    pub handshake_digest: String,
}

impl McpCapabilityHandshake {
    pub fn negotiate(
        protocol_version: impl Into<String>,
        server_info: &Value,
        session_ref: &str,
        advertisements: Vec<McpToolAdvertisement>,
        declared: Vec<McpDeclaredTool>,
        binding_scopes: &BTreeSet<String>,
    ) -> Result<Self, String> {
        let protocol_version = protocol_version.into();
        if protocol_version.trim().is_empty()
            || protocol_version.len() > MCP_MAX_PROTOCOL_VERSION
            || protocol_version.chars().any(char::is_control)
            || !matches!(
                protocol_version.as_str(),
                "2024-11-05" | "2025-03-26" | "2025-06-18"
            )
            || !server_info.is_object()
            || session_ref.trim().is_empty()
            || session_ref.len() > MCP_MAX_SESSION_REF
            || session_ref.chars().any(char::is_control)
            || advertisements.len() > MCP_MAX_HANDSHAKE_TOOLS
        {
            return Err("mcp_capability_handshake_invalid".to_owned());
        }
        let mut declared_by_name = BTreeMap::new();
        for contract in declared {
            contract.validate()?;
            if declared_by_name
                .insert(contract.tool_name.clone(), contract)
                .is_some()
            {
                return Err("mcp_tool_declaration_ambiguous".to_owned());
            }
        }
        let mut seen = BTreeSet::new();
        let mut tools = Vec::with_capacity(advertisements.len());
        for advertisement in advertisements {
            advertisement.validate()?;
            if !seen.insert(advertisement.name.clone()) {
                return Err("mcp_tool_ambiguous".to_owned());
            }
            let Some(contract) = declared_by_name.get(&advertisement.name) else {
                // Discovery may see a newly advertised tool. It is retained as conditional
                // metadata, but cannot be invoked until a later server-owned operation mapping
                // supplies a schema digest and scope intersection.
                let tool_name = advertisement.name.clone();
                let input_schema_digest = advertisement.input_schema_digest();
                let output_schema_digest = advertisement.output_schema_digest();
                tools.push(McpToolCapability {
                    schema: MCP_TOOL_CAPABILITY_SCHEMA.to_owned(),
                    tool_name,
                    operation_id: "mcp.unbound".to_owned(),
                    input_schema_digest,
                    output_schema_digest,
                    binding_scope_digest: json_digest(&json!({
                        "source": "kiana.unbound_tool",
                        "binding_scopes": binding_scopes,
                    })),
                    available: false,
                    conditional: true,
                });
                continue;
            };
            if contract.input_schema_digest != advertisement.input_schema_digest()
                || contract.output_schema_digest != advertisement.output_schema_digest()
            {
                return Err("mcp_tool_schema_changed".to_owned());
            }
            tools.push(McpToolCapability::from_declared(contract, binding_scopes));
        }
        tools.sort_by(|left, right| left.tool_name.cmp(&right.tool_name));
        let mut handshake = Self {
            schema: MCP_CAPABILITY_HANDSHAKE_SCHEMA.to_owned(),
            transport: MCP_STDIO_TRANSPORT.to_owned(),
            protocol_version,
            server_info_digest: json_digest(server_info),
            catalog_digest: json_digest(&json!({ "tools": &tools })),
            session_digest: json_digest(&json!({ "session_ref": session_ref })),
            state: McpSessionState::Ready,
            tools,
            handshake_digest: String::new(),
        };
        handshake.handshake_digest = handshake.digest();
        handshake.validate()?;
        Ok(handshake)
    }

    pub fn transition(&mut self, next: McpSessionState) -> Result<(), String> {
        if !self.state.can_transition_to(next) {
            return Err("mcp_session_transition_invalid".to_owned());
        }
        self.state = next;
        self.handshake_digest = self.digest();
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MCP_CAPABILITY_HANDSHAKE_SCHEMA
            || self.transport != MCP_STDIO_TRANSPORT
            || !matches!(
                self.protocol_version.as_str(),
                "2024-11-05" | "2025-03-26" | "2025-06-18"
            )
            || !valid_digest(&self.server_info_digest)
            || !valid_digest(&self.catalog_digest)
            || !valid_digest(&self.session_digest)
            || !valid_digest(&self.handshake_digest)
            || self.tools.len() > MCP_MAX_HANDSHAKE_TOOLS
            || self.handshake_digest != self.digest()
        {
            return Err("mcp_capability_handshake_invalid".to_owned());
        }
        let mut names = BTreeSet::new();
        for tool in &self.tools {
            tool.validate()?;
            if !names.insert(tool.tool_name.clone()) {
                return Err("mcp_tool_ambiguous".to_owned());
            }
        }
        if self.catalog_digest != json_digest(&json!({ "tools": &self.tools })) {
            return Err("mcp_tool_catalog_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "transport": self.transport,
            "protocol_version": self.protocol_version,
            "server_info_digest": self.server_info_digest,
            "catalog_digest": self.catalog_digest,
            "session_digest": self.session_digest,
            "state": self.state,
            "tools": &self.tools,
        }))
    }
}

fn valid_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(is_sha256_hex)
}
