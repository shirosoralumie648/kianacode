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

## Reclaim and audit closing slice (2026-10-17)

The earlier source slice reported a negotiation result even when the child was never stopped
(a refused advertisement returned through `?` while the process was only dropped), and it
published the handshake with the session state still `ready`. The closing slice makes the
reclaim path unconditional and auditable:

- `McpCapabilityHandshake::retract_unavailable_tools()` is the domain-owned terminal reclaim. It
  refuses a live session with `mcp_session_not_terminal`, marks every unreachable capability
  (`available == false`, e.g. unbound or outside the binding-scope intersection) with
  `retracted: true`, keeps the advertised name and schema digests as evidence, recomputes the
  catalog and handshake digests, and is idempotent. `McpToolCapability::validate` rejects
  `retracted && available`, and the handshake rejects a retracted catalogue in a non-terminal
  session, so a reclaim can never be undone by a later transition or a replayed payload.
- `StdioMcpConnectorAdapter::run_handshake` now keeps the negotiation outcome in a value and
  always performs the confirmed `client.stop()` before reporting: a refused advertisement, a
  schema drift or a broken catalogue can no longer return through `?` and leave the child to be
  killed asynchronously by `Drop`. After the stop it records `McpSessionState::Closed` and calls
  the terminal reclaim, so the audited fact never implies a live session or an invocable tool.
- `map_handshake_error` keeps the timeout/disconnect/stop-unconfirmed splits and otherwise
  preserves a bounded stable code (`mcp_project_untrusted`, `mcp_executable_changed`,
  `mcp_snapshot_required`, `mcp_server_scope_not_authority`, ...) instead of collapsing every
  failure into one generic startup code; free-form or unbounded text still falls back to the
  caller's split.

## Behavioural fixtures (2026-10-17, CI-only)

Domain contract fixtures stay in `kiana-domain/tests/int10_stdio_mcp.rs`:

| Fixture | Proves |
|---|---|
| `server_tool_schema_cannot_grant_scope` | a non-empty `server_requested_scopes` set is `mcp_server_scope_not_authority` |
| `unbound_tool_is_conditional_and_scope_free` | discovery-only metadata stays `conditional`/`mcp.unbound`, and the session reference is digest-only |
| `conditional_tool_can_be_retracted_after_disconnect` | disconnect is terminal and cannot reopen |
| `declared_schema_drift_is_denied_before_capability_projection` | a declared operation whose schema digest drifted is `mcp_tool_schema_changed` |
| `handshake_bounds_protocol_session_catalog_and_unknown_fields` | unsupported/oversized protocol versions, empty or control-bearing session references, non-object server metadata, oversized catalogues, ambiguous tool/declaration names, unsupported server schemas and unknown JSON fields all fail closed; an SSE-shaped payload can never validate as stdio |
| `declared_scope_intersection_decides_availability` | availability comes from the binding-scope intersection, never from the server; an unbound advertisement stays conditional and no scope string reaches the projection |
| `terminal_session_reclaims_conditional_tools_and_cannot_reopen` | the reclaim refuses a live session, is idempotent and digest-bound, cannot be flipped back to `available`, and cannot belong to a live session |
| `handshake_evidence_binds_digests_without_session_or_scope_material` | the audited field set is pinned, the session reference and scope values stay outside it, and the digests bind the server metadata, catalogue and session |

Daemon fixtures drive the production `StdioMcpConnectorAdapter` against a real, bwrap-confined
stdio child (the same `ConfinedMcpClient` the `McpHandler` path uses) and a stdlib-only Python
server; the host observes the child through `/proc/*/cmdline` marker arguments instead of
trusting the child's own report:

| Fixture | Proves |
|---|---|
| `mcp_remote_transport_and_foreign_handshakes_are_refused_before_a_child_starts` | http/sse/ws transports, a stdio config with a URL or headers, a foreign server name and a non-`stdio_mcp` binding are refused before a child exists; the declared capability set is exactly `capability_handshake`; the fixture marker never appears in `/proc` |
| `mcp_scope_advertisement_is_refused_and_the_child_is_reclaimed` | a server-declared scope is refused after `tools/list` with `mcp_server_scope_not_authority`, and the child is still reclaimed through the confirmed stop |
| `mcp_handshake_requires_a_trusted_pinned_executable_and_scope` | an untrusted project, a tampered executable pin and a missing snapshot fail before a child exists and keep their own stable codes |
| `mcp_startup_timeout_disconnect_and_success_are_bounded_reclaimed_and_audited` | positive-control marker observation, `mcp_startup_timeout` for an unanswered initialize, `mcp_disconnected` for an EOF after initialize, and a legal handshake that is `closed`, retracts both unbound tools and never contains the session reference |
| `handshake_is_deny_first_audited_and_reclaims_the_scope_free_session` (daemon integration, real `DaemonHost → ControlPlane → Broker → EventStore`) | a remote configuration with process fields fails registry startup; an untrusted project, a remote endpoint registered in the same registry, an unknown server, a foreign adapter, a missing binding, a scope advertisement and caller-supplied session/scope/authority fields are all refused with zero facts appended; one legal handshake is appended once with `source_cursor` 3 and replayed by request id, a new request id appends a second fact, a revoked binding is refused, and no fixture descendant survives |

### CI wiring

`.github/workflows/int10-stdio-mcp.yml` is a `workflow_dispatch`-only, `contents: read` lane that
installs and verifies `bubblewrap` and `python3`, runs `cargo fetch --locked`,
`cargo fmt --all --check`, the domain/core/daemon guard targets, the daemon module fixtures, each
named daemon fixture with `--exact --test-threads=1`, and `--no-run` compiles for every target.
The unified `ci.yml` covers the same fixtures automatically: `kiana-domain-s3/4` runs
`int10_stdio_mcp`, `kiana-core-s4/6` runs `int10_stdio_mcp_guard`, and the whole-crate
`kiana-daemon` shard (which installs `bubblewrap`) runs the library fixtures and
`kiana-daemon/tests/int10_stdio_mcp.rs`. `kiana-core/tests/int10_stdio_mcp_guard.rs` contains
`int10_behavioural_fixtures_are_wired_and_deny_first`, which binds every named fixture to the
adapter/domain sources, this baseline and the lane, and forbids ignore or unsandboxed fallbacks.

## CI-only evidence

GitHub Actions runs the domain deny/success handshake fixtures, Core and daemon source guards, the
daemon library fixtures and the daemon integration fixture. Local Cargo tests, builds, checks,
clippy and smoke commands are intentionally not run and CI is not awaited.

## Evidence boundary and limitations

```text
feature_status: implemented
proof_level: source
```

The handshake proves only source-level contracts and a bounded local child protocol path. It does
not prove a live provider, durable cross-process session recovery, external business effect,
provider receipt correctness or physical/live evidence. HTTP MCP remains unsupported and later
INT-11 owns HTTPS/SSRF/egress policy.

Open limitations kept explicit:

- The daemon path still passes no server-owned operation mapping (`declared` is empty), so every
  advertised tool is `mcp.unbound`/conditional and is retracted when the session closes; mapping
  tool names to connector operations belongs to INT-25, and until then no server tool can become
  available.
- The `AccountBinding` fixture/hash contract is still required for a `stdio_mcp` binding (the
  registry parses the local fixture at bind time); the handshake itself never reads it.
- Reclaim is in-process and per invocation: no durable cross-process session ledger, no
  reconciliation of a handshake interrupted by process death (INT-29 owns restart recovery).
- A replay of the same request id still re-runs the read-only observation before the idempotent
  append returns the original fact; a write-path connector must not inherit that shape.
