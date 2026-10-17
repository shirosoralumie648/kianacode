//! INT-10 daemon behaviour fixture: the stdio MCP capability handshake.
//!
//! These fixtures drive the production `DaemonHost → ControlPlane → Broker → adapter → EventStore`
//! path with a real, bwrap confined stdio MCP child.  They prove deny-first behaviour (a remote
//! configuration, a foreign binding, a revoked binding, an untrusted project and caller-supplied
//! session/scope material never reach a child), that the audited handshake fact stays session-free
//! and digest-bound, and that the per-invocation session is reclaimed with every unreachable
//! capability retracted.
//!
//! The registry is process-wide configuration, so the whole fixture runs in one serial test that
//! sets `KIANA_MCP_SERVERS_JSON` only around `DaemonHost` construction.  The INT-10 lane and the
//! unified `ci.yml` daemon shard both run with `--test-threads=1`.
#![cfg(target_os = "linux")]

use kiana_daemon::{DaemonHost, ProjectTrustAuthority};
use kiana_domain::{
    connector_fixture_sha256, AccountBinding, ConnectorDefinition, ConnectorEffect,
    ConnectorOperation,
};
use kiana_protocol::{
    ApprovalChallenge, ApprovalDecision, ExecutionStatus, RequestEnvelope, RequestMetadata,
    ResponseEnvelope,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const FIXTURE_PATH: &str = "mcp/handshake-fixture.json";
const BINDING_ID: &str = "binding-int10";
const FIXTURE_ADAPTER_BINDING_ID: &str = "binding-int10-fixture";
const SERVER_NAME: &str = "int10-stdio";
const SCOPE_SERVER_NAME: &str = "int10-scope";
const REMOTE_SERVER_NAME: &str = "int10-http";
const MCP_SERVERS_ENV: &str = "KIANA_MCP_SERVERS_JSON";

struct TrustedProject;

impl ProjectTrustAuthority for TrustedProject {
    fn project_trusted(&self, _project_root: &Path) -> Result<bool, String> {
        Ok(true)
    }
}

struct UntrustedProject;

impl ProjectTrustAuthority for UntrustedProject {
    fn project_trusted(&self, _project_root: &Path) -> Result<bool, String> {
        Ok(false)
    }
}

/// A stdlib-only stdio MCP server.  `sys.argv[2]` selects the behaviour: `serve` answers the
/// handshake with two tools, `scope` additionally attaches a server-requested scope to one tool.
/// `sys.argv[1]` is a per-fixture marker so the host can prove the child is gone instead of
/// trusting the child's own report.
const INT10_MCP_SERVER: &str = r##"#!/usr/bin/python3
import json
import sys

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


def advertised_tools():
    if MODE != "scope":
        return TOOLS
    return [
        dict(tool, requiredScopes=["admin"]) if tool["name"] == "lookup" else tool
        for tool in TOOLS
    ]


while True:
    line = sys.stdin.readline()
    if not line:
        break
    line = line.strip()
    if not line:
        continue
    message = json.loads(line)
    method = message.get("method")
    if method == "initialize":
        send({
            "jsonrpc": "2.0",
            "id": message["id"],
            "result": {
                "protocolVersion": "2025-06-18",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "int10-fixture", "version": "1.0.0"},
            },
        })
    elif method == "notifications/initialized":
        continue
    elif method == "tools/list":
        send({"jsonrpc": "2.0", "id": message["id"], "result": {"tools": advertised_tools()}})
    else:
        send({
            "jsonrpc": "2.0",
            "id": message.get("id"),
            "error": {"code": -32601, "message": "unsupported"},
        })
"##;

fn fixture_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "kiana-int10-{label}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    root.canonicalize().unwrap()
}

fn marker(role: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("int10-{role}-{nanos}")
}

fn server_script(root: &Path) -> PathBuf {
    let path = root.join("int10_mcp_server.py");
    fs::write(&path, INT10_MCP_SERVER).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path.canonicalize().unwrap()
}

/// The trusted startup configuration: two stdio servers backed by the same fixture script plus
/// one remote endpoint that the connector handshake must refuse.  Every shape here is a legal
/// startup shape, so the later refusals are about the handshake boundary, not about parsing.
fn servers_config(script: &Path, serve_marker: &str, scope_marker: &str) -> Value {
    json!([
        {
            "name": SERVER_NAME,
            "transport": "stdio",
            "command": script.to_string_lossy(),
            "args": [serve_marker, "serve"],
        },
        {
            "name": SCOPE_SERVER_NAME,
            "transport": "stdio",
            "command": script.to_string_lossy(),
            "args": [scope_marker, "scope"],
        },
        {
            "name": REMOTE_SERVER_NAME,
            "transport": "http",
            "url": "https://mcp.int10.invalid/mcp",
        },
    ])
}

/// Set the registry configuration only for the duration of the host construction: `McpRegistry`
/// captures the parsed servers, so command execution no longer depends on the environment.
fn with_mcp_servers<T>(config: &Value, build: impl FnOnce() -> T) -> T {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let previous = std::env::var_os(MCP_SERVERS_ENV);
    std::env::set_var(MCP_SERVERS_ENV, config.to_string());
    let outcome = build();
    match previous {
        Some(value) => std::env::set_var(MCP_SERVERS_ENV, value),
        None => std::env::remove_var(MCP_SERVERS_ENV),
    }
    drop(guard);
    outcome
}

fn definition(adapter: &str) -> ConnectorDefinition {
    ConnectorDefinition {
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
    }
}

fn account_binding(binding_id: &str, fixture_sha256: &str) -> AccountBinding {
    AccountBinding {
        schema: "kiana.account-binding.v1".to_owned(),
        binding_id: binding_id.to_owned(),
        connector_id: "connector-int10".to_owned(),
        account_id: "account-int10".to_owned(),
        read_scopes: BTreeSet::from(["read".to_owned()]),
        write_scopes: BTreeSet::new(),
        fixture_path: FIXTURE_PATH.to_owned(),
        fixture_sha256: fixture_sha256.to_owned(),
        credential_ref: None,
    }
}

/// The binding still needs the pinned local fixture contract (the registry parses it at bind
/// time); the handshake itself never reads it.
fn fixture_bytes() -> Vec<u8> {
    serde_json::to_vec(&json!({
        "schema": "kiana.connector-fixture.v1",
        "connector_id": "connector-int10",
        "account_id": "account-int10",
        "operations": {
            "lookup": [{
                "payload": {"id": "one"},
                "outcome": "succeeded",
                "receipt_id": "receipt-one",
                "result": {"value": "fixture"},
            }],
        },
    }))
    .unwrap()
}

fn trusted_metadata(root: &Path, session: &str) -> RequestMetadata {
    let mut metadata = RequestMetadata::local(session, root.to_string_lossy());
    metadata.project_trusted = true;
    metadata
}

async fn command(
    host: &DaemonHost,
    metadata: RequestMetadata,
    name: &str,
    arguments: Value,
) -> ResponseEnvelope {
    host.handle(RequestEnvelope::command(metadata, name, arguments))
        .await
}

async fn approve(
    host: &DaemonHost,
    metadata: RequestMetadata,
    response: &ResponseEnvelope,
) -> ResponseEnvelope {
    assert_eq!(
        response.status,
        ExecutionStatus::AwaitingApproval,
        "command was not staged for approval: {:?}",
        response.error
    );
    let challenge: ApprovalChallenge =
        serde_json::from_value(response.output["approval"].clone()).unwrap();
    host.handle(RequestEnvelope::approval_decision_with_proof(
        metadata,
        challenge.approval_id,
        ApprovalDecision::Approve,
        Some(challenge.request_hash.clone()),
        Some(challenge.nonce.clone()),
    ))
    .await
}

async fn bind(
    host: &DaemonHost,
    root: &Path,
    session: &str,
    binding_id: &str,
    adapter: &str,
    expected_registry_version: u64,
    key: &str,
) -> ResponseEnvelope {
    let bytes = fixture_bytes();
    let metadata = trusted_metadata(root, session);
    let pending = command(
        host,
        metadata.clone(),
        "connector.manage",
        json!({
            "action": "bind",
            "definition": definition(adapter),
            "binding": account_binding(binding_id, &connector_fixture_sha256(&bytes)),
            "expected_registry_version": expected_registry_version,
            "reason": "INT-10 handshake acceptance",
            "idempotency_key": key,
        }),
    )
    .await;
    approve(host, metadata, &pending).await
}

async fn revoke(
    host: &DaemonHost,
    root: &Path,
    session: &str,
    binding_id: &str,
    expected_registry_version: u64,
    key: &str,
) -> ResponseEnvelope {
    let metadata = trusted_metadata(root, session);
    let pending = command(
        host,
        metadata.clone(),
        "connector.manage",
        json!({
            "action": "revoke",
            "binding_id": binding_id,
            "expected_registry_version": expected_registry_version,
            "reason": "INT-10 handshake acceptance",
            "idempotency_key": key,
        }),
    )
    .await;
    approve(host, metadata, &pending).await
}

async fn handshake(
    host: &DaemonHost,
    root: &Path,
    session: &str,
    binding_id: &str,
    server: &str,
) -> ResponseEnvelope {
    command(
        host,
        trusted_metadata(root, session),
        "connector.mcp_handshake",
        json!({"binding_id": binding_id, "server": server}),
    )
    .await
}

async fn registry_version(host: &DaemonHost, root: &Path, session: &str) -> u64 {
    let listed = command(
        host,
        trusted_metadata(root, session),
        "connector.manage",
        json!({"action": "list"}),
    )
    .await;
    assert_eq!(
        listed.status,
        ExecutionStatus::Completed,
        "registry projection failed: {:?}",
        listed.error
    );
    listed.output["registry_version"].as_u64().unwrap()
}

fn assert_denied(response: &ResponseEnvelope, marker: &str) {
    assert_ne!(
        response.status,
        ExecutionStatus::Completed,
        "deny path unexpectedly completed: {:?}",
        response.error
    );
    let error = response.error.as_deref().unwrap_or_default();
    assert!(
        error.contains(marker) || response.output.to_string().contains(marker),
        "expected {marker} in error {:?} / output {}",
        response.error,
        response.output
    );
}

/// PIDs whose command line still carries the fixture marker: a bounded host observation of the
/// current namespace, not a claim about unreachable descendants.
fn marker_pids(marker: &str) -> Vec<u32> {
    let mut pids = Vec::new();
    let Ok(entries) = fs::read_dir("/proc") else {
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
        let Ok(cmdline) = fs::read(format!("/proc/{pid}/cmdline")) else {
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

async fn assert_no_survivor(marker: &str, context: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let survivors = marker_pids(marker);
        if survivors.is_empty() {
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("{context}: {marker} descendants survived the stop path: {survivors:?}");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

/// Deny first, then success: remote and foreign handshakes never start a child and never append a
/// fact, and a legal read-only handshake is appended once, is session-free and digest-bound, and
/// reclaims the child with every unreachable capability retracted.
#[tokio::test]
async fn handshake_is_deny_first_audited_and_reclaims_the_scope_free_session() {
    let root = fixture_dir("handshake");
    let fixture = root.join(FIXTURE_PATH);
    fs::create_dir_all(fixture.parent().unwrap()).unwrap();
    fs::write(&fixture, fixture_bytes()).unwrap();
    let script = server_script(&root);
    let serve_marker = marker("serve");
    let scope_marker = marker("scope");
    let session = "session-int10";

    // A remote configuration that also carries process fields is refused when the registry
    // starts: there is no shape where an HTTP/SSE endpoint inherits a pinned executable.
    let smuggled = json!([{
        "name": REMOTE_SERVER_NAME,
        "transport": "http",
        "url": "https://mcp.int10.invalid/mcp",
        "command": "/bin/sh",
    }]);
    let refused = with_mcp_servers(&smuggled, || {
        DaemonHost::with_env_harness_and_project_authority(Arc::new(TrustedProject))
    });
    let refused = match refused {
        Ok(_) => panic!("an HTTP configuration with process fields must fail startup"),
        Err(error) => error,
    };
    assert!(
        refused
            .to_string()
            .contains("mcp_http_process_fields_invalid"),
        "{refused}"
    );

    let config = servers_config(&script, &serve_marker, &scope_marker);
    let host = with_mcp_servers(&config, || {
        DaemonHost::with_env_harness_and_project_authority(Arc::new(TrustedProject))
    })
    .expect("the mixed stdio/HTTP registry is a legal startup configuration");
    let untrusted_host = with_mcp_servers(&config, || {
        DaemonHost::with_env_harness_and_project_authority(Arc::new(UntrustedProject))
    })
    .expect("the registry is trusted configuration even for an untrusted project");

    // An untrusted project is refused before the binding is resolved.
    let untrusted = handshake(&untrusted_host, &root, session, BINDING_ID, SERVER_NAME).await;
    assert_denied(&untrusted, "project_untrusted");

    let bound = bind(
        &host,
        &root,
        session,
        BINDING_ID,
        "stdio_mcp",
        0,
        "bind-int10",
    )
    .await;
    assert_eq!(
        bound.status,
        ExecutionStatus::Completed,
        "bind failed: {:?}",
        bound.error
    );
    let fixture_bound = bind(
        &host,
        &root,
        session,
        FIXTURE_ADAPTER_BINDING_ID,
        "local_fixture",
        1,
        "bind-int10-fixture",
    )
    .await;
    assert_eq!(
        fixture_bound.status,
        ExecutionStatus::Completed,
        "second bind failed: {:?}",
        fixture_bound.error
    );

    for (label, binding_id, server, expected) in [
        // A remote endpoint registered under the same trusted registry is never a handshake
        // target, even though the connector binding itself is the stdio adapter.
        (
            "remote_server",
            BINDING_ID,
            REMOTE_SERVER_NAME,
            "mcp_http_unsupported",
        ),
        (
            "unknown_server",
            BINDING_ID,
            "int10-absent",
            "mcp_server_unknown",
        ),
        // The adapter is server-owned: a binding that is not the stdio MCP adapter is refused
        // before the executable is pinned or a child exists.
        (
            "foreign_adapter",
            FIXTURE_ADAPTER_BINDING_ID,
            SERVER_NAME,
            "mcp_http_unsupported",
        ),
        (
            "missing_binding",
            "binding-absent",
            SERVER_NAME,
            "connector_binding_missing",
        ),
        // A server tool that asks for a scope is refused after `tools/list` and the child is still
        // reclaimed.
        (
            "scope_advertisement",
            BINDING_ID,
            SCOPE_SERVER_NAME,
            "mcp_server_scope_not_authority",
        ),
    ] {
        let denied = handshake(&host, &root, session, binding_id, server).await;
        assert_denied(&denied, expected);
        assert!(
            marker_pids(&serve_marker).is_empty(),
            "{label}: a refused handshake must not leave a live session"
        );
    }

    // Caller-supplied session or scope material is not part of the command surface at all.
    for arguments in [
        json!({"binding_id": BINDING_ID, "server": SERVER_NAME, "session_ref": "caller-chosen"}),
        json!({"binding_id": BINDING_ID, "server": SERVER_NAME, "scope": "admin"}),
        json!({"binding_id": BINDING_ID, "server": SERVER_NAME, "binding_authorized": true}),
    ] {
        let denied = command(
            &host,
            trusted_metadata(&root, session),
            "connector.mcp_handshake",
            arguments,
        )
        .await;
        assert_denied(&denied, "connector_mcp_handshake_arguments_invalid");
    }

    // No deny path appended a fact: the stream still holds exactly the two binds.
    assert_eq!(registry_version(&host, &root, session).await, 2);

    // Success: one audited, session-free handshake fact.
    let request = RequestEnvelope::command(
        trusted_metadata(&root, session),
        "connector.mcp_handshake",
        json!({"binding_id": BINDING_ID, "server": SERVER_NAME}),
    );
    let first = host.handle(request.clone()).await;
    assert_eq!(
        first.status,
        ExecutionStatus::Completed,
        "handshake failed: {:?}",
        first.error
    );
    assert_eq!(
        first.output["schema"],
        "kiana.connector-mcp-handshake-result.v1"
    );
    assert_eq!(first.output["binding_id"], BINDING_ID);
    assert_eq!(first.output["server"], SERVER_NAME);
    assert_eq!(first.output["proof_level"], "source");
    assert_eq!(first.output["replayed"], false);
    assert_eq!(first.output["source_cursor"], 3);
    assert!(first.output["event_id"].is_string());
    let capability_fact = &first.output["handshake"];
    assert_eq!(
        capability_fact["schema"],
        "kiana.mcp-capability-handshake.v1"
    );
    assert_eq!(capability_fact["transport"], "stdio");
    assert_eq!(capability_fact["protocol_version"], "2025-06-18");
    assert_eq!(
        capability_fact["state"], "closed",
        "the per-invocation session is already reclaimed when the fact is written"
    );
    for digest in [
        "server_info_digest",
        "catalog_digest",
        "session_digest",
        "handshake_digest",
    ] {
        let value = capability_fact[digest].as_str().unwrap_or_default();
        assert_eq!(value.len(), "sha256:".len() + 64, "{digest}: {value}");
        assert!(value.starts_with("sha256:"), "{digest}: {value}");
    }
    let tools = capability_fact["tools"]
        .as_array()
        .expect("bounded tool catalogue");
    assert_eq!(
        tools
            .iter()
            .map(|tool| tool["tool_name"].as_str().unwrap_or_default())
            .collect::<Vec<_>>(),
        vec!["echo", "lookup"]
    );
    for tool in tools {
        assert_eq!(tool["operation_id"], "mcp.unbound");
        assert_eq!(tool["available"], false);
        assert_eq!(tool["conditional"], true);
        assert_eq!(
            tool["retracted"], true,
            "a closed session reclaims every capability it cannot promote"
        );
    }
    let rendered = first.output.to_string();
    for forbidden in [
        session,
        "requiredScopes",
        "admin",
        "session_ref",
        script.to_str().unwrap(),
        FIXTURE_PATH,
        "stderr",
    ] {
        assert!(
            !rendered.contains(forbidden),
            "the handshake result leaked {forbidden}: {rendered}"
        );
    }

    // The same request id is a retry: it returns the original fact and appends nothing.
    let replayed = host.handle(request).await;
    assert_eq!(replayed.status, ExecutionStatus::Completed);
    assert_eq!(replayed.output["replayed"], true);
    assert_eq!(replayed.output["event_id"], first.output["event_id"]);
    assert_eq!(replayed.output["handshake"], first.output["handshake"]);

    // A new request id is a new observation and appends exactly one more fact.
    let second = handshake(&host, &root, session, BINDING_ID, SERVER_NAME).await;
    assert_eq!(
        second.status,
        ExecutionStatus::Completed,
        "second handshake failed: {:?}",
        second.error
    );
    assert_eq!(second.output["replayed"], false);
    assert_eq!(second.output["source_cursor"], 4);
    assert_ne!(second.output["event_id"], first.output["event_id"]);
    assert_eq!(registry_version(&host, &root, session).await, 4);

    // A revoked binding is refused before the adapter is reached, and the refusal appends no fact.
    let revoked = revoke(&host, &root, session, BINDING_ID, 4, "revoke-int10").await;
    assert_eq!(
        revoked.status,
        ExecutionStatus::Completed,
        "revoke failed: {:?}",
        revoked.error
    );
    let after_revoke = handshake(&host, &root, session, BINDING_ID, SERVER_NAME).await;
    assert_denied(&after_revoke, "connector_binding_revoked");
    assert_eq!(registry_version(&host, &root, session).await, 5);

    // The confined child never survives the handshake, on any path.
    assert_no_survivor(&serve_marker, "handshake session").await;
    assert_no_survivor(&scope_marker, "refused scope session").await;

    let _ = fs::remove_dir_all(&root);
}
