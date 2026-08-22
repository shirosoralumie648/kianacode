# v0.4 Phase 3 Verification

Date: 2026-08-23
Proof: `local_behavior`
Verdict: pass
Requirement: CODE-02
PATH-03: extended to `shell` + `apply_patch` + `mcp`

## Demo contract

```text
trusted + workspace-write + KIANA_MCP_SERVERS_JSON stdio mock
cassette: mcp { tool: echo, arguments: { message: hello } }
→ result contains hello; harness remains kiana-harness
untrusted → project_untrusted, no child
reviewer → role_tool_denied
http transport → mcp_transport_unsupported
```

Same-host proof is in-process DaemonHost. Config is `KIANA_MCP_SERVERS_JSON`.
No new `kiana run` flag. Did not format `cli.rs`.

## Evidence

| Criterion | Result |
|---|---|
| Model-visible tool is `mcp` | `kiana-runner` `tool_schemas()` + Builder RoleSpec tools |
| Execution is `mcp.call` on the network broker | `capability_for_tool` → `CapabilityKind::Network` / `ExternalSideEffect`; `harness_mcp::register` |
| stdio echo through daemon | `trusted_builder_stdio_mcp_echoes_through_daemon`: tool result contains `hello` and `kiana.mcp-result.v1`; harness `kiana-harness` |
| Untrusted deny, no product MCP path | `untrusted_mcp_does_not_spawn_a_server` → tool result `project_untrusted`; run still Completed |
| Reviewer cannot call mcp | `reviewer_cannot_call_mcp` → `Failed` / `role_tool_denied`; sandbox stays `read-only` |
| HTTP/SSE/WS fail-closed | `http_mcp_is_unsupported_this_slice` → `mcp_transport_unsupported` |
| Policy: trusted Builder + Balanced allows | `trusted_builder_workspace_write_allows_mcp_call` |
| Policy: Safe/read-only still Ask | `read_only_mcp_call_still_asks` |
| Not legacy MCP server / kiana-tools | Did not mark `kiana-entrypoints/src/mcp.rs` or `kiana-tools/mcp_tool.rs` complete |

## Commands run

```
cargo fmt -p kiana-domain -p kiana-policy -p kiana-runner -p kiana-daemon
cargo test -p kiana-domain --locked --lib -- --test-threads=1
cargo test -p kiana-policy --locked --lib -- --test-threads=1
cargo test -p kiana-runner --locked --lib -- --test-threads=1
cargo test -p kiana-daemon --locked --lib -- --test-threads=1
cargo test -p kiana-daemon --test daemon_host --locked -- --test-threads=1
```

All listed targets passed (10 / 12 / 22 / 24 / 26). Did not `cargo fmt --all`, did not format `kiana-entrypoints/src/cli.rs`, did not compile the CLI crate, did not run `scripts/release-smoke.sh` or live provider.

## Not claimed

- HTTP / SSE / WS MCP (fail-closed this slice)
- MCP marketplace / SSO / remote auth
- exploding one MCP server into N model-visible tools
- skills/hooks on harness (CODE-03)
- provider explicit degrade as a `kiana run` product proof (CODE-04)
- structured Read/Grep/Glob
- five departments / six-layer RAG / JointSymposium
- TeamCreate / SendMessage
- migrating TUI
- live provider / physical readiness
- v1.0 REL-03 (P0-SKILL/HOOK/PROV still open)
