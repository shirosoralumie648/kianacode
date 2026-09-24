# INT-10 stdio MCP connector and capability handshake baseline

## Scope

INT-10 adds a stdio-only MCP connector adapter and a typed capability handshake. The adapter uses
the existing `DaemonHost → ControlPlane → Broker` path and reuses the confined per-invocation
`ConfinedMcpClient`; it does not add an HTTP client, a remote session, or a second model loop.

## Implemented source slice

- `McpCapabilityHandshake`, `McpToolAdvertisement`, `McpToolCapability` and `McpSessionState` are
  domain-owned bounded contracts. Initialization and `tools/list` metadata are digest-bound, and
  the session state records ready, timeout, disconnect and close transitions.
- Server tool schemas are treated as untrusted metadata. A non-empty server scope request is
  rejected with `mcp_server_scope_not_authority`; a newly advertised tool is retained only as an
  unavailable `conditional`/`mcp.unbound` capability. Available tools require a server-owned
  operation mapping and binding scope intersection.
- `StdioMcpConnectorAdapter` rejects HTTP/SSE/WebSocket configuration before process start, pins
  the executable through the existing sandbox snapshot, performs initialize/tools-list, bounds
  all schemas, and always stops the child. Startup timeout, EOF/disconnect and unconfirmed stop
  are mapped to stable failure or `result_unknown` outcomes.
- `connector.mcp_handshake` is normalized by ControlPlane as a read-only operator command, binds
  the active project snapshot, dispatches through Broker, and appends a binding-scoped handshake
  fact. Session references and server capability data are digest-bound; no raw secret, URL header
  or server scope enters the event.

## CI-only evidence

GitHub Actions runs the domain deny/success handshake fixtures, Core and daemon source guards and a
targeted compile. Local Cargo tests, builds, checks, clippy and smoke commands are intentionally
not run and CI is not awaited.

## Evidence boundary and limitations

```text
feature_status: implemented
proof_level: source
```

The handshake proves only source-level contracts and a bounded local child protocol path. It does
not prove a live provider, durable cross-process session recovery, external business effect,
provider receipt correctness or physical/live evidence. HTTP MCP remains unsupported and later
INT-11 owns HTTPS/SSRF/egress policy.
