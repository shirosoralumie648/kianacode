//! Stdio-only MCP connector adapter.
//!
//! This adapter is reached after a connector capability permit.  It reuses the confined
//! per-invocation MCP process owned by the daemon and returns only the typed handshake.  HTTP,
//! SSE, WebSocket and remote configuration are rejected before a child or network client starts.

use crate::mcp_stdio::ConfinedMcpClient;
use async_trait::async_trait;
use kiana_domain::{
    McpCapabilityHandshake, McpDeclaredTool, McpSessionState, McpToolAdvertisement,
    MCP_STDIO_TRANSPORT,
};
use kiana_ports::{
    ConnectorAdapter, ConnectorAdapterCapabilities, ConnectorAdapterCapability,
    McpCapabilityHandshakeRequest, PortError,
};
use kiana_services::mcp::{McpServerConfig, TransportType};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use tokio::sync::watch;

pub(crate) struct StdioMcpConnectorAdapter {
    config: McpServerConfig,
    scope: Value,
    declared: Vec<McpDeclaredTool>,
}

impl StdioMcpConnectorAdapter {
    pub(crate) fn new(
        config: McpServerConfig,
        scope: Value,
        declared: Vec<McpDeclaredTool>,
    ) -> Result<Self, PortError> {
        if config.transport != TransportType::Stdio
            || config.url.is_some()
            || config
                .headers
                .as_ref()
                .is_some_and(|headers| !headers.is_empty())
        {
            return Err(failed("mcp_http_unsupported"));
        }
        if config.name.trim().is_empty() {
            return Err(failed("mcp_server_name_invalid"));
        }
        Ok(Self {
            config,
            scope,
            declared,
        })
    }

    async fn run_handshake(
        &self,
        request: &McpCapabilityHandshakeRequest,
    ) -> Result<McpCapabilityHandshake, PortError> {
        if request.server != self.config.name {
            return Err(failed("mcp_server_binding_mismatch"));
        }
        if request.binding.definition.adapter != "stdio_mcp" {
            return Err(failed("mcp_connector_transport_unsupported"));
        }
        let (_sender, cancellation) = watch::channel(false);
        let mut client = ConfinedMcpClient::connect(&self.config, &self.scope, cancellation)
            .await
            .map_err(|error| map_handshake_error(error, "mcp_startup_failed"))?;
        // Once the child exists every outcome must reclaim it: the negotiation result is only
        // reported after a confirmed stop, so a refused advertisement, a schema drift or a broken
        // catalogue cannot leave a live session behind.
        let negotiated: Result<McpCapabilityHandshake, PortError> = async {
            let tools = client.list_tools().await?;
            let server_info = client.server_info.clone();
            let protocol_version = server_info["protocol_version"]
                .as_str()
                .ok_or_else(|| failed("mcp_protocol_version_required"))?
                .to_owned();
            let advertisements = tools
                .iter()
                .map(advertisement_from_tool)
                .collect::<Result<Vec<_>, _>>()?;
            let binding_scopes = request.binding.binding.read_scopes.clone();
            McpCapabilityHandshake::negotiate(
                protocol_version,
                &server_info,
                &request.session_ref,
                advertisements,
                self.declared.clone(),
                &binding_scopes,
            )
            .map_err(failed)
        }
        .await;
        let mut handshake = match (negotiated, client.stop().await) {
            (_, Err(stop_error)) => {
                return Err(map_handshake_error(
                    stop_error,
                    "result_unknown:mcp_stop_unconfirmed",
                ))
            }
            (Err(error), Ok(_)) => return Err(map_handshake_error(error, "mcp_tools_list_failed")),
            (Ok(handshake), Ok(_)) => handshake,
        };
        // The child is gone, so the session is terminal.  Record that fact and reclaim every
        // capability the closed session can no longer promote instead of publishing an
        // invocable-looking catalogue.
        handshake
            .transition(McpSessionState::Closed)
            .map_err(failed)?;
        handshake.retract_unavailable_tools().map_err(failed)?;
        Ok(handshake)
    }
}

#[async_trait]
impl ConnectorAdapter for StdioMcpConnectorAdapter {
    fn capabilities(&self) -> ConnectorAdapterCapabilities {
        ConnectorAdapterCapabilities::from_iter([ConnectorAdapterCapability::CapabilityHandshake])
    }

    async fn capability_handshake(
        &self,
        request: McpCapabilityHandshakeRequest,
    ) -> Result<McpCapabilityHandshake, PortError> {
        self.run_handshake(&request).await
    }
}

fn advertisement_from_tool(tool: &Value) -> Result<McpToolAdvertisement, PortError> {
    let name = tool["name"]
        .as_str()
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| failed("mcp_tool_name_invalid"))?;
    let mut requested_scopes = BTreeSet::new();
    if let Some(scopes) = tool.get("requiredScopes") {
        let values = scopes
            .as_array()
            .ok_or_else(|| failed("mcp_server_scope_not_authority"))?;
        for scope in values {
            requested_scopes.insert(
                scope
                    .as_str()
                    .filter(|scope| !scope.trim().is_empty())
                    .ok_or_else(|| failed("mcp_server_scope_not_authority"))?
                    .to_owned(),
            );
        }
    }
    let advertisement = McpToolAdvertisement {
        schema: kiana_domain::MCP_TOOL_ADVERTISEMENT_SCHEMA.to_owned(),
        name: name.to_owned(),
        input_schema: tool["inputSchema"].clone(),
        output_schema: tool.get("outputSchema").cloned(),
        description: tool
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_owned),
        conditional: tool["execution"]["taskSupport"] == "optional",
        server_requested_scopes: requested_scopes,
    };
    advertisement.validate().map_err(failed)?;
    Ok(advertisement)
}

/// Bound every transport failure to a bounded code.  Timeout, disconnect and an unconfirmed stop
/// are the three split outcomes the handshake vocabulary needs; every other failure keeps its own
/// stable code so spawn, executable-pin, protocol and advertisement failures stay tellable apart
/// in the audited evidence.
fn map_handshake_error(error: PortError, fallback: &str) -> PortError {
    let text = error.to_string();
    if text.contains("mcp_response_timeout") || text.contains("mcp_write_timeout") {
        return failed("mcp_startup_timeout");
    }
    if text.contains("mcp_stream_closed") {
        return failed("mcp_disconnected");
    }
    if text.contains("mcp_stop_unconfirmed") {
        return failed("result_unknown:mcp_stop_unconfirmed");
    }
    failed(stable_code(&text).unwrap_or_else(|| fallback.to_owned()))
}

/// Keep only codes that are bounded tokens.  Provider text, paths or any other free-form failure
/// description never reach the handshake evidence; the caller's split is used instead.
fn stable_code(text: &str) -> Option<String> {
    let code = text
        .strip_prefix("port_failed:")
        .or_else(|| text.strip_prefix("port_conflict:"))
        .or_else(|| text.strip_prefix("port_unavailable:"))
        .unwrap_or(text)
        .trim();
    let bounded = !code.is_empty()
        && code.len() <= 128
        && code
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b':' | b'.' | b'-'));
    bounded.then(|| code.to_owned())
}

fn failed(reason: impl Into<String>) -> PortError {
    PortError::Failed(reason.into())
}

#[cfg(test)]
mod tests {
    //! INT-10 daemon behaviour fixtures for the stdio MCP connector adapter.
    //!
    //! Every fixture drives the production `StdioMcpConnectorAdapter` against a real, bwrap
    //! confined stdio child (the same `ConfinedMcpClient` the `McpHandler` path uses) and a
    //! stdlib-only Python server.  The host observes the child through `/proc/*/cmdline` marker
    //! arguments instead of trusting the child's own report, so "reclaimed" means "no descendant
    //! survives".  A missing `bwrap`/`python3` fails the fixture instead of degrading to an
    //! unsandboxed call; the INT-10 lane installs and verifies both.
    use super::*;
    use kiana_domain::{
        AccountBinding, ConnectorBindingSnapshot, ConnectorDefinition, ConnectorEffect,
        ConnectorOperation,
    };
    use std::collections::{BTreeMap, HashMap};
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    #[cfg(target_os = "linux")]
    const INT10_MCP_SERVER: &str = r##"#!/usr/bin/python3
import json
import sys
import time

MARKER = sys.argv[1]
MODE = sys.argv[2]

TOOLS = [
    {
        "name": "echo",
        "description": "Echo one bounded message",
        "inputSchema": {
            "type": "object",
            "properties": {"message": {"type": "string"}},
            "additionalProperties": False,
        },
    },
    {
        "name": "lookup",
        "description": "Look up one record",
        "inputSchema": {
            "type": "object",
            "properties": {"id": {"type": "string"}},
            "additionalProperties": False,
        },
    },
]


def send(payload):
    sys.stdout.write(json.dumps(payload) + "\n")
    sys.stdout.flush()


def read_message():
    line = sys.stdin.readline()
    if not line:
        return None
    line = line.strip()
    if not line:
        return {}
    return json.loads(line)


def advertised_tools():
    if MODE != "scope":
        return TOOLS
    return [
        dict(tool, requiredScopes=["admin"]) if tool["name"] == "lookup" else tool
        for tool in TOOLS
    ]


def main():
    while True:
        message = read_message()
        if message is None:
            break
        method = message.get("method")
        if method is None:
            continue
        if method == "initialize":
            if MODE == "silent":
                time.sleep(600)
                continue
            send({
                "jsonrpc": "2.0",
                "id": message["id"],
                "result": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "int10-fixture", "version": "1.0.0", "marker": MARKER},
                },
            })
        elif method == "notifications/initialized":
            continue
        elif method == "tools/list":
            if MODE == "exit":
                # Deterministic disconnect: the request was read, so the client observes EOF
                # while waiting for the response instead of racing a broken pipe write.
                sys.exit(0)
            send({"jsonrpc": "2.0", "id": message["id"], "result": {
                "tools": advertised_tools()}})
        else:
            send({
                "jsonrpc": "2.0",
                "id": message.get("id"),
                "error": {"code": -32601, "message": "unsupported"},
            })


main()
"##;

    #[cfg(target_os = "linux")]
    fn int10_temp_root() -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("kiana-int10-mcp-{stamp}"));
        std::fs::create_dir_all(&root).unwrap();
        // The bwrap plan and the confined client both canonicalize the project root.
        root.canonicalize().unwrap()
    }

    #[cfg(target_os = "linux")]
    fn int10_marker(role: &str) -> String {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("int10-{role}-{stamp}")
    }

    #[cfg(target_os = "linux")]
    fn int10_server(root: &Path) -> PathBuf {
        let path = root.join("int10_mcp_server.py");
        std::fs::write(&path, INT10_MCP_SERVER).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.canonicalize().unwrap()
    }

    #[cfg(target_os = "linux")]
    fn int10_config(script: &Path, marker: &str, mode: &str) -> McpServerConfig {
        McpServerConfig {
            name: "int10-fixture".to_owned(),
            transport: TransportType::Stdio,
            url: None,
            command: Some(script.to_string_lossy().into_owned()),
            args: Some(vec![marker.to_owned(), mode.to_owned()]),
            env: None,
            headers: None,
        }
    }

    /// The scope shape the production registry builds for a handshake: read-only project root,
    /// one descriptor-pinned executable and an explicit deadline.
    #[cfg(target_os = "linux")]
    fn int10_scope(root: &Path, script: &Path, timeout_ms: u64) -> Value {
        let bytes = std::fs::read(script).unwrap();
        json!({
            "project_trusted": true,
            "project_root": root.to_string_lossy(),
            "sandbox": "read-only",
            "path_allow": ["."],
            "timeout_ms": timeout_ms,
            "mcp_snapshot": {
                "server_id": "int10-fixture",
                "transport": "stdio",
                "env_keys": [],
                "credential_refs": [],
                "executable": {
                    "path": script.to_string_lossy(),
                    "bytes": bytes.len(),
                    "sha256": kiana_domain::journal_sha256(&bytes),
                },
                "argument_files": [],
            },
        })
    }

    #[cfg(target_os = "linux")]
    fn int10_binding(root: &Path, adapter: &str) -> ConnectorBindingSnapshot {
        ConnectorBindingSnapshot {
            definition: ConnectorDefinition {
                schema: "kiana.connector-definition.v1".to_owned(),
                connector_id: "connector-int10".to_owned(),
                version: "v1".to_owned(),
                provider_id: "provider-int10".to_owned(),
                adapter: adapter.to_owned(),
                operations: BTreeMap::from([(
                    "lookup".to_owned(),
                    ConnectorOperation {
                        effect: ConnectorEffect::ReadOnly,
                        required_scope: "read".to_owned(),
                        data_classes: BTreeSet::new(),
                    },
                )]),
                rate_limit_per_minute: 10,
                idempotency_required: true,
                reconciliation_required: true,
                data_processing: "local_only".to_owned(),
            },
            binding: AccountBinding {
                schema: "kiana.account-binding.v1".to_owned(),
                binding_id: "binding-int10".to_owned(),
                connector_id: "connector-int10".to_owned(),
                account_id: "account-int10".to_owned(),
                read_scopes: BTreeSet::from(["read".to_owned()]),
                write_scopes: BTreeSet::new(),
                fixture_path: "fixtures/handshake.json".to_owned(),
                fixture_sha256: kiana_domain::connector_fixture_sha256(b"{}"),
                credential_ref: None,
            },
            project_root: root.to_string_lossy().into_owned(),
            revision: 1,
            status: "active".to_owned(),
        }
    }

    #[cfg(target_os = "linux")]
    fn int10_request(
        server: &str,
        session_ref: &str,
        binding: &ConnectorBindingSnapshot,
    ) -> McpCapabilityHandshakeRequest {
        McpCapabilityHandshakeRequest {
            schema: kiana_ports::CONNECTOR_MCP_HANDSHAKE_REQUEST_SCHEMA.to_owned(),
            binding: binding.clone(),
            server: server.to_owned(),
            session_ref: session_ref.to_owned(),
        }
    }

    #[cfg(target_os = "linux")]
    fn int10_failure_code(error: PortError) -> String {
        match error {
            PortError::Failed(code) | PortError::Conflict(code) | PortError::Unavailable(code) => {
                code
            }
        }
    }

    /// PIDs whose command line still carries the fixture marker.  This is a bounded host
    /// observation of the current namespace, not a claim about unreachable descendants.
    #[cfg(target_os = "linux")]
    fn int10_marker_pids(marker: &str) -> Vec<u32> {
        let mut pids = Vec::new();
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return pids;
        };
        for entry in entries.flatten() {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
            else {
                continue;
            };
            let Ok(cmdline) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
                continue;
            };
            if cmdline
                .split(|byte| *byte == 0)
                .any(|part| part == marker.as_bytes())
            {
                pids.push(pid);
            }
        }
        pids
    }

    /// Positive control: the survivor check must be able to see a live fixture process, or every
    /// later "no survivor" assertion would be vacuous.
    #[cfg(target_os = "linux")]
    async fn int10_assert_marker_running(marker: &str) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while tokio::time::Instant::now() < deadline {
            if !int10_marker_pids(marker).is_empty() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("fixture process {marker} was never observed as a live descendant");
    }

    #[cfg(target_os = "linux")]
    async fn int10_assert_no_survivor(marker: &str, context: &str) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let survivors = int10_marker_pids(marker);
            if survivors.is_empty() {
                return;
            }
            if tokio::time::Instant::now() >= deadline {
                panic!("{context}: {marker} descendants survived the stop path: {survivors:?}");
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    /// Deny first: an HTTP/SSE/WS-shaped configuration and a binding that is not the stdio MCP
    /// adapter are refused before the registry pins an executable or a child exists.  The
    /// assertion is observed, not assumed: the fixture marker never appears in `/proc`.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn mcp_remote_transport_and_foreign_handshakes_are_refused_before_a_child_starts() {
        let root = int10_temp_root();
        let script = int10_server(&root);
        let marker = int10_marker("remote");
        let remote = |transport: TransportType,
                      url: Option<&str>,
                      headers: Option<HashMap<String, String>>| {
            McpServerConfig {
                name: "int10-fixture".to_owned(),
                transport,
                url: url.map(str::to_owned),
                command: Some(script.to_string_lossy().into_owned()),
                args: Some(vec![marker.clone(), "serve".to_owned()]),
                env: None,
                headers,
            }
        };
        for (label, config) in [
            (
                "http",
                remote(
                    TransportType::Http,
                    Some("https://example.invalid/mcp"),
                    None,
                ),
            ),
            (
                "sse",
                remote(
                    TransportType::Sse,
                    Some("https://example.invalid/sse"),
                    None,
                ),
            ),
            (
                "ws",
                remote(TransportType::Ws, Some("wss://example.invalid/ws"), None),
            ),
            (
                "stdio_with_url",
                remote(
                    TransportType::Stdio,
                    Some("https://example.invalid/mcp"),
                    None,
                ),
            ),
            (
                "stdio_with_headers",
                remote(
                    TransportType::Stdio,
                    None,
                    Some(HashMap::from([(
                        "authorization".to_owned(),
                        "Bearer int10-denied".to_owned(),
                    )])),
                ),
            ),
        ] {
            let refused = StdioMcpConnectorAdapter::new(
                config,
                int10_scope(&root, &script, 5_000),
                Vec::new(),
            );
            let error = match refused {
                Ok(_) => {
                    panic!("{label}: a remote transport must be refused before any child exists")
                }
                Err(error) => error,
            };
            assert_eq!(int10_failure_code(error), "mcp_http_unsupported", "{label}");
        }

        // A request for another server name, or one whose binding is not the stdio MCP adapter,
        // is refused at the adapter boundary instead of being resolved by the trusted registry.
        let adapter = StdioMcpConnectorAdapter::new(
            int10_config(&script, &marker, "serve"),
            int10_scope(&root, &script, 5_000),
            Vec::new(),
        )
        .expect("a stdio configuration is admitted");
        for (label, server, adapter_name, expected) in [
            (
                "foreign_server",
                "int10-other",
                "stdio_mcp",
                "mcp_server_binding_mismatch",
            ),
            (
                "foreign_adapter",
                "int10-fixture",
                "local_fixture",
                "mcp_connector_transport_unsupported",
            ),
        ] {
            let binding = int10_binding(&root, adapter_name);
            let error = adapter
                .capability_handshake_checked(int10_request(server, "session-denied", &binding))
                .await
                .expect_err("a foreign handshake must be refused");
            assert_eq!(int10_failure_code(error), expected, "{label}");
        }
        assert!(
            int10_marker_pids(&marker).is_empty(),
            "a refused handshake must not start a fixture process"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// Deny first: a server-declared scope is metadata, never authority.  The refusal happens
    /// after `tools/list` and still reclaims the child through the confirmed stop path.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn mcp_scope_advertisement_is_refused_and_the_child_is_reclaimed() {
        let root = int10_temp_root();
        let script = int10_server(&root);
        let marker = int10_marker("scope");
        let adapter = StdioMcpConnectorAdapter::new(
            int10_config(&script, &marker, "scope"),
            int10_scope(&root, &script, 10_000),
            Vec::new(),
        )
        .expect("a stdio configuration is admitted");
        let binding = int10_binding(&root, "stdio_mcp");
        let error = adapter
            .capability_handshake_checked(int10_request("int10-fixture", "session-scope", &binding))
            .await
            .expect_err("a server-requested scope must never become a grant");
        assert_eq!(int10_failure_code(error), "mcp_server_scope_not_authority");
        int10_assert_no_survivor(&marker, "scope refusal").await;
        let _ = std::fs::remove_dir_all(root);
    }

    /// Deny first: an untrusted project, a tampered executable pin and a missing snapshot all
    /// fail before a child exists and keep their own stable code instead of collapsing into a
    /// generic startup failure.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn mcp_handshake_requires_a_trusted_pinned_executable_and_scope() {
        let root = int10_temp_root();
        let script = int10_server(&root);
        let binding = int10_binding(&root, "stdio_mcp");
        let marker = int10_marker("preconditions");
        let adapter = StdioMcpConnectorAdapter::new(
            int10_config(&script, &marker, "serve"),
            int10_scope(&root, &script, 5_000),
            Vec::new(),
        )
        .expect("a stdio configuration is admitted");

        let mut untrusted = int10_scope(&root, &script, 5_000);
        untrusted["project_trusted"] = json!(false);
        let mut tampered = int10_scope(&root, &script, 5_000);
        tampered["mcp_snapshot"]["executable"]["sha256"] =
            json!(kiana_domain::journal_sha256(b"not the fixture server"));
        let mut missing = int10_scope(&root, &script, 5_000);
        missing["mcp_snapshot"] = json!({});

        for (label, scope, expected) in [
            ("untrusted", untrusted, "mcp_project_untrusted"),
            ("tampered_pin", tampered, "mcp_executable_changed"),
            ("missing_snapshot", missing, "mcp_snapshot_required"),
        ] {
            let guard = StdioMcpConnectorAdapter::new(
                int10_config(&script, &marker, "serve"),
                scope,
                Vec::new(),
            )
            .expect("the configuration is admitted before the scope is inspected");
            let error = guard
                .capability_handshake_checked(int10_request(
                    "int10-fixture",
                    "session-precondition",
                    &binding,
                ))
                .await
                .expect_err("an unverified precondition must be refused");
            assert_eq!(int10_failure_code(error), expected, "{label}");
        }
        // The config keeps the adapter's declared capability set explicit and single-purpose.
        let capabilities = adapter.capabilities();
        assert!(capabilities.supports(ConnectorAdapterCapability::CapabilityHandshake));
        assert_eq!(
            capabilities
                .capabilities
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            vec![ConnectorAdapterCapability::CapabilityHandshake]
        );
        int10_assert_no_survivor(&marker, "precondition refusals").await;
        let _ = std::fs::remove_dir_all(root);
    }

    /// Success and regression: a legal read-only handshake is bounded, auditable and reclaimed.
    /// A startup timeout and a disconnect are split into `mcp_startup_timeout`/`mcp_disconnected`,
    /// a closed session retracts every capability it can no longer promote, and the session
    /// reference never appears in the evidence.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn mcp_startup_timeout_disconnect_and_success_are_bounded_reclaimed_and_audited() {
        let root = int10_temp_root();
        let script = int10_server(&root);
        let binding = int10_binding(&root, "stdio_mcp");

        // Startup timeout: the server never answers.  The positive control runs first so the
        // survivor check is known to be able to see a live fixture process; the deadline has to
        // be long enough for that observation to beat the timeout on a loaded runner.
        let silent_marker = int10_marker("silent");
        let silent = StdioMcpConnectorAdapter::new(
            int10_config(&script, &silent_marker, "silent"),
            int10_scope(&root, &script, 3_000),
            Vec::new(),
        )
        .expect("a stdio configuration is admitted");
        let silent_binding = binding.clone();
        let task = tokio::spawn(async move {
            silent
                .capability_handshake_checked(int10_request(
                    "int10-fixture",
                    "session-silent",
                    &silent_binding,
                ))
                .await
        });
        int10_assert_marker_running(&silent_marker).await;
        let error = task
            .await
            .expect("fixture task")
            .expect_err("an unanswered startup must time out");
        assert_eq!(int10_failure_code(error), "mcp_startup_timeout");
        int10_assert_no_survivor(&silent_marker, "startup timeout").await;

        // Disconnect: the server answers initialize and exits, so the catalogue read sees EOF.
        let exit_marker = int10_marker("exit");
        let exit = StdioMcpConnectorAdapter::new(
            int10_config(&script, &exit_marker, "exit"),
            int10_scope(&root, &script, 10_000),
            Vec::new(),
        )
        .expect("a stdio configuration is admitted");
        let error = exit
            .capability_handshake_checked(int10_request("int10-fixture", "session-exit", &binding))
            .await
            .expect_err("a disconnected server must not yield a capability set");
        assert_eq!(int10_failure_code(error), "mcp_disconnected");
        int10_assert_no_survivor(&exit_marker, "disconnect").await;

        // Success: the catalogue is bounded and digest-bound, the session is terminal and every
        // unreachable capability is retracted instead of being published as invocable.
        let serve_marker = int10_marker("serve");
        let serve = StdioMcpConnectorAdapter::new(
            int10_config(&script, &serve_marker, "serve"),
            int10_scope(&root, &script, 10_000),
            Vec::new(),
        )
        .expect("a stdio configuration is admitted");
        let session_ref = "session-int10-serve";
        let handshake = serve
            .capability_handshake_checked(int10_request("int10-fixture", session_ref, &binding))
            .await
            .expect("a legal read-only handshake completes");
        assert_eq!(handshake.transport, MCP_STDIO_TRANSPORT);
        assert_eq!(handshake.state, McpSessionState::Closed);
        assert_eq!(handshake.protocol_version, "2025-06-18");
        assert_eq!(
            handshake
                .tools
                .iter()
                .map(|tool| tool.tool_name.as_str())
                .collect::<Vec<_>>(),
            vec!["echo", "lookup"]
        );
        for tool in &handshake.tools {
            assert_eq!(tool.operation_id, "mcp.unbound");
            assert!(!tool.available);
            assert!(tool.conditional);
            assert!(
                tool.retracted,
                "a closed session reclaims unreachable capabilities"
            );
        }
        handshake
            .validate()
            .expect("the audited handshake is consistent");
        let rendered = serde_json::to_string(&handshake).expect("handshake serializes");
        assert!(
            !rendered.contains(session_ref),
            "the session reference must stay outside the evidence: {rendered}"
        );
        assert!(rendered.contains("handshake_digest"));
        int10_assert_no_survivor(&serve_marker, "confirmed stop").await;
        let _ = std::fs::remove_dir_all(root);
    }
}
